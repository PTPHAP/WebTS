//! Standard AES-256-GCM envelopes for explicitly server-readable messages.
use crate::{
    app::{Api, Error},
    db::now,
};
use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, Payload},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Deserialize;
use zeroize::{Zeroize, Zeroizing};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Packet {
    version: u8,
    mode: String,
    epoch: i64,
    key: String,
    iv: String,
    content: String,
}
impl Drop for Packet {
    fn drop(&mut self) {
        self.key.zeroize();
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    version: u8,
    site: String,
    id: String,
    sender: i64,
    recipient: i64,
    expires: i64,
    text: String,
    image: Option<String>,
    sticker: Option<String>,
    burn: bool,
}
impl Drop for Envelope {
    fn drop(&mut self) {
        self.text.zeroize();
        if let Some(v) = &mut self.image {
            v.zeroize();
        }
    }
}
struct Context<'a> {
    site: &'a str,
    id: &'a str,
    sender: i64,
    recipient: i64,
    burn: bool,
    epoch: i64,
}
pub fn validate(
    text: &str,
    site: &str,
    id: &str,
    sender: i64,
    recipient: i64,
    burn: bool,
    epoch: i64,
) -> Api<()> {
    validate_at(
        text,
        &Context {
            site,
            id,
            sender,
            recipient,
            burn,
            epoch,
        },
        now(),
    )
}
fn validate_at(text: &str, expected: &Context<'_>, clock: i64) -> Api<()> {
    let Context {
        site,
        id,
        sender,
        recipient,
        burn,
        epoch,
    } = *expected;
    let bad = || Error::bad("服务器可解密消息必须为绑定当前会话的AES-256-GCM密文");
    let packet: Packet = serde_json::from_str(text).map_err(|_| bad())?;
    if packet.version != 1 || packet.mode != "server" || packet.epoch != epoch || epoch < 1 {
        return Err(bad());
    }
    let key = Zeroizing::new(STANDARD.decode(&packet.key).map_err(|_| bad())?);
    let iv = STANDARD.decode(&packet.iv).map_err(|_| bad())?;
    let content = STANDARD.decode(&packet.content).map_err(|_| bad())?;
    if key.len() != 32 || iv.len() != 12 || content.len() > 192 * 1024 + 16 {
        return Err(bad());
    }
    let aad = format!("webts-server-content-v1:{site}:{id}:{sender}:{recipient}:{burn}:{epoch}");
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|_| bad())?;
    let plain = Zeroizing::new(
        cipher
            .decrypt(
                Nonce::from_slice(&iv),
                Payload {
                    msg: &content,
                    aad: aad.as_bytes(),
                },
            )
            .map_err(|_| bad())?,
    );
    let value: Envelope = serde_json::from_slice(&plain).map_err(|_| bad())?;
    if value.version != 1
        || value.site != site
        || value.id != id
        || value.sender != sender
        || value.recipient != recipient
        || value.burn != burn
        || value.expires <= clock
        || value.expires > clock + 31 * 86400
        || value.text.encode_utf16().count() > 4096
        || value.sticker.as_ref().is_some_and(|s| s.len() > 32)
        || value.image.as_ref().is_some_and(|s| {
            s.len() > 170000
                || s.strip_prefix("data:image/jpeg;base64,")
                    .is_none_or(|data| STANDARD.decode(data).is_err())
        })
    {
        return Err(bad());
    }
    Ok(())
}
pub fn object_context(peer: i64, mode: &str, epoch: i64) -> String {
    if mode == "server" {
        format!("{peer}:server:{epoch}")
    } else {
        peer.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    fn fixture() -> Value {
        serde_json::from_str(include_str!("../tests/fixtures/friend-server-content.json")).unwrap()
    }
    fn expected() -> Context<'static> {
        Context {
            site: "https://fixture.example",
            id: "a041b157-319f-4bf3-970e-7a04e30bd070",
            sender: 1,
            recipient: 2,
            burn: false,
            epoch: 4,
        }
    }
    #[test]
    fn browser_webcrypto_fixture_authenticates_and_cannot_be_copied_to_other_contexts() {
        let f = fixture();
        let packet = f["packet"].to_string();
        let clock = f["clock"].as_i64().unwrap();
        assert!(validate_at(&packet, &expected(), clock).is_ok());
        for e in [
            Context {
                site: "https://other.example",
                ..expected()
            },
            Context {
                id: "b041b157-319f-4bf3-970e-7a04e30bd070",
                ..expected()
            },
            Context {
                sender: 3,
                ..expected()
            },
            Context {
                recipient: 3,
                ..expected()
            },
            Context {
                burn: true,
                ..expected()
            },
            Context {
                epoch: 5,
                ..expected()
            },
        ] {
            assert!(validate_at(&packet, &e, clock).is_err());
        }
        for (field, bad) in [
            ("key", json!(STANDARD.encode([0; 16]))),
            ("iv", json!(STANDARD.encode([0; 11]))),
            ("content", json!(STANDARD.encode([0; 32]))),
            ("mode", json!("e2ee")),
            ("epoch", json!(0)),
            ("version", json!(2)),
        ] {
            let mut v = f["packet"].clone();
            v[field] = bad;
            assert!(validate_at(&v.to_string(), &expected(), clock).is_err());
        }
        assert_ne!(object_context(2, "server", 4), object_context(2, "e2ee", 4));
        assert_eq!(object_context(2, "e2ee", 0), "2");
    }
    fn authenticated_packet(value: &Value) -> String {
        let f = fixture();
        let mut p = f["packet"].clone();
        let key = STANDARD.decode(p["key"].as_str().unwrap()).unwrap();
        let iv = STANDARD.decode(p["iv"].as_str().unwrap()).unwrap();
        let cipher = Aes256Gcm::new_from_slice(&key).unwrap();
        let aad = "webts-server-content-v1:https://fixture.example:a041b157-319f-4bf3-970e-7a04e30bd070:1:2:false:4";
        let ciphertext = cipher
            .encrypt(
                Nonce::from_slice(&iv),
                Payload {
                    msg: value.to_string().as_bytes(),
                    aad: aad.as_bytes(),
                },
            )
            .unwrap();
        p["content"] = json!(STANDARD.encode(ciphertext));
        p.to_string()
    }
    #[test]
    fn even_authenticated_plaintext_must_match_the_bound_identity_and_size_policy() {
        let f = fixture();
        let clock = f["clock"].as_i64().unwrap();
        for (field, bad) in [
            ("sender", json!(3)),
            ("recipient", json!(3)),
            ("burn", json!(true)),
            ("site", json!("https://other.example")),
            ("expires", json!(clock)),
            ("expires", json!(clock + 32 * 86400)),
            ("text", json!("x".repeat(4097))),
            ("image", json!("https://tracker.example/image")),
            ("image", json!("data:image/svg+xml;base64,PHN2Zz4=")),
            ("unknown", json!(true)),
        ] {
            let mut v = f["envelope"].clone();
            v[field] = bad;
            assert!(validate_at(&authenticated_packet(&v), &expected(), clock).is_err());
        }
    }
}
