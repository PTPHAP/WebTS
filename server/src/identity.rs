use anyhow::{Context, Result, bail};
use tsclientlib::Identity;
use zeroize::Zeroizing;

pub fn parse(text: &str) -> Result<Identity> {
    if text.len() > 16_384 {
        bail!("身份文件过大");
    }
    let text = text.trim_start_matches('\u{feff}').trim();
    let encoded = if text.starts_with('[') {
        let mut key = None;
        for line in text.lines() {
            let Some((name, value)) = line.split_once('=') else {
                continue;
            };
            if name.trim().eq_ignore_ascii_case("identity") {
                if key.is_some() {
                    bail!("每次只能导入一个身份");
                }
                let value = value.trim();
                key = Some(
                    value
                        .strip_prefix('"')
                        .and_then(|v| v.strip_suffix('"'))
                        .unwrap_or(value),
                );
            }
        }
        key.context("文件中没有身份私钥")?
    } else {
        text
    };
    Identity::new_from_str(encoded)
        .map_err(|_| anyhow::anyhow!("身份格式无效，需要包含私钥的 TeamSpeak 导出文件"))
}

pub fn uid(identity: &Identity) -> String {
    identity.key().to_pub().get_uid()
}

pub fn canonical(identity: &Identity) -> Zeroizing<String> {
    Zeroizing::new(format!("{}V{}", identity.counter(), identity.key().to_ts()))
}

pub fn export(identity: &Identity, name: &str) -> Zeroizing<String> {
    let name: String = name
        .chars()
        .filter(|c| !c.is_control() && *c != '"' && *c != '\\')
        .take(80)
        .collect();
    Zeroizing::new(format!(
        "[Identity]\nidentity=\"{}V{}\"\nnickname=\"{}\"\n",
        identity.counter(),
        identity.key().to_ts_obfuscated(),
        name
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generated_identity_round_trips_ini_and_canonical_without_changing_uid() {
        let original = Identity::create();
        let ini = export(&original, "测试\"\n用户");
        let imported = parse(&ini).unwrap();
        assert_eq!(uid(&original), uid(&imported));
        assert_eq!(original.counter(), imported.counter());
        assert_eq!(uid(&original), uid(&parse(&canonical(&original)).unwrap()));
        assert!(!ini.contains("测试\"\n"));
    }
    #[test]
    fn malformed_public_or_multiple_identities_do_not_generate_new_keys() {
        let public = Identity::create().key().to_pub().to_ts();
        assert!(parse(&public).is_err());
        assert!(parse("this-is-not-a-private-key").is_err());
        assert!(parse("[Identity]\nnickname=hello").is_err());
        assert!(parse("[Identity]\nidentity=a\nidentity=b").is_err());
        assert!(parse(&"x".repeat(16_385)).is_err());
    }
}
