//! A thin browser binding. Olm v1 performs all key agreement and Double Ratchet operations.
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use vodozemac::{
    Curve25519PublicKey, Ed25519PublicKey, Ed25519Signature,
    olm::{Account, AccountPickle, OlmMessage, Session, SessionConfig, SessionPickle},
};
use wasm_bindgen::prelude::*;

fn error(_: impl std::fmt::Display) -> JsValue {
    #[cfg(target_arch = "wasm32")]
    {
        JsValue::from_str("加密状态或消息验证失败")
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        JsValue::NULL
    }
}
#[derive(Serialize, Deserialize)]
struct Saved {
    account: AccountPickle,
    sessions: HashMap<String, SessionPickle>,
    peers: HashMap<String, String>,
    active: HashMap<String, String>,
    #[serde(default)]
    activity: HashMap<String, u64>,
    #[serde(default)]
    clock: u64,
}
#[wasm_bindgen]
pub struct Engine {
    account: Account,
    sessions: HashMap<String, Session>,
    peers: HashMap<String, String>,
    active: HashMap<String, String>,
    activity: HashMap<String, u64>,
    clock: u64,
    touched: HashSet<String>,
}
#[wasm_bindgen]
impl Engine {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            account: Account::new(),
            sessions: HashMap::new(),
            peers: HashMap::new(),
            active: HashMap::new(),
            activity: HashMap::new(),
            clock: 0,
            touched: HashSet::new(),
        }
    }
    pub fn restore(text: &str) -> Result<Engine, JsValue> {
        if text.len() > 8 * 1024 * 1024 {
            return Err(error("size"));
        }
        let s: Saved = serde_json::from_str(text).map_err(error)?;
        if s.sessions.len() > 256
            || s.peers.len() > 256
            || s.active.len() > 132
            || s.activity.len() > 256
        {
            return Err(error("size"));
        }
        Ok(Self {
            account: Account::from_pickle(s.account),
            sessions: s
                .sessions
                .into_iter()
                .map(|(k, v)| (k, Session::from_pickle(v)))
                .collect(),
            peers: s.peers,
            active: s.active,
            activity: s.activity,
            clock: s.clock,
            touched: HashSet::new(),
        })
    }
    pub fn save(&self) -> Result<String, JsValue> {
        let text = serde_json::to_string(&Saved {
            account: self.account.pickle(),
            sessions: self
                .sessions
                .iter()
                .map(|(k, v)| (k.clone(), v.pickle()))
                .collect(),
            peers: self.peers.clone(),
            active: self.active.clone(),
            activity: self.activity.clone(),
            clock: self.clock,
        })
        .map_err(error)?;
        if text.len() > 8 * 1024 * 1024 {
            return Err(error("size"));
        }
        Ok(text)
    }
    pub fn identity(&self) -> String {
        serde_json::json!({"version":1,"curve":self.account.curve25519_key().to_base64(),"ed":self.account.ed25519_key().to_base64()}).to_string()
    }
    pub fn sign(&self, text: &str) -> String {
        self.account.sign(text).to_base64()
    }
    pub fn verify(ed: &str, text: &str, signature: &str) -> bool {
        let Ok(key) = Ed25519PublicKey::from_base64(ed) else {
            return false;
        };
        let Ok(sig) = Ed25519Signature::from_base64(signature) else {
            return false;
        };
        key.verify(text.as_bytes(), &sig).is_ok()
    }
    pub fn prekeys(&mut self) -> Result<String, JsValue> {
        self.account.generate_one_time_keys(32);
        let mut keys: Vec<_> = self
            .account
            .one_time_keys()
            .values()
            .map(|v| v.to_base64())
            .collect();
        keys.sort();
        let payload = serde_json::json!({"identity":self.identity(),"keys":keys}).to_string();
        let signature = self.sign(&payload);
        self.account.mark_keys_as_published();
        Ok(serde_json::json!({"payload":payload,"signature":signature}).to_string())
    }
    pub fn has_session(&self, peer_curve: &str) -> bool {
        self.active.contains_key(peer_curve)
    }
    // Stored messages live at most 30 days. Leave a one-day grace period;
    // old pickles without timestamps start that grace only on first maintenance.
    pub fn maintenance(&mut self, now_seconds: f64, current_peers: &str) -> Result<(), JsValue> {
        if !now_seconds.is_finite() || !(0.0..=1e12).contains(&now_seconds) {
            return Err(error("clock"));
        }
        if current_peers.len() > 16384 {
            return Err(error("peers"));
        }
        let current: HashSet<String> = serde_json::from_str(current_peers).map_err(error)?;
        if current.len() > 128 {
            return Err(error("peers"));
        }
        self.clock = self.clock.max(now_seconds as u64);
        for id in std::mem::take(&mut self.touched) {
            self.activity.insert(id, self.clock);
        }
        for id in self.sessions.keys() {
            self.activity.entry(id.clone()).or_insert(self.clock);
        }
        let cutoff = self.clock.saturating_sub(31 * 86400);
        // A current peer can send fresh normal Olm messages after a long absence.
        // Expiry of stored messages alone does not obsolete its active ratchet.
        let live: HashSet<_> = self
            .active
            .iter()
            .filter(|(peer, _)| current.contains(*peer))
            .map(|(_, id)| id.clone())
            .collect();
        self.sessions.retain(|id, _| {
            live.contains(id) || self.activity.get(id).is_some_and(|at| *at >= cutoff)
        });
        self.peers.retain(|id, _| self.sessions.contains_key(id));
        self.activity.retain(|id, _| self.sessions.contains_key(id));
        self.active.retain(|_, id| self.sessions.contains_key(id));
        Ok(())
    }
    pub fn outbound(&mut self, peer_curve: &str, one_time_key: &str) -> Result<(), JsValue> {
        if self.sessions.len() >= 256 {
            return Err(error("sessions"));
        }
        let session = self
            .account
            .create_outbound_session(
                SessionConfig::version_1(),
                Curve25519PublicKey::from_base64(peer_curve).map_err(error)?,
                Curve25519PublicKey::from_base64(one_time_key).map_err(error)?,
            )
            .map_err(error)?;
        let id = session.session_id();
        self.peers.insert(id.clone(), peer_curve.into());
        self.active.insert(peer_curve.into(), id.clone());
        self.sessions.insert(id, session);
        self.touched.insert(self.active[peer_curve].clone());
        Ok(())
    }
    pub fn encrypt(&mut self, peer_curve: &str, text: &str) -> Result<String, JsValue> {
        if text.len() > 192 * 1024 {
            return Err(error("size"));
        }
        let id = self
            .active
            .get(peer_curve)
            .ok_or_else(|| error("session"))?;
        let session = self.sessions.get_mut(id).ok_or_else(|| error("session"))?;
        let message = session.encrypt(text).map_err(error)?;
        self.touched.insert(id.clone());
        serde_json::to_string(&serde_json::json!({"session":id,"message":message})).map_err(error)
    }
    pub fn decrypt(&mut self, peer_curve: &str, packet: &str) -> Result<String, JsValue> {
        if packet.len() > 384 * 1024 {
            return Err(error("size"));
        }
        #[derive(Deserialize)]
        struct Packet {
            session: String,
            message: OlmMessage,
        }
        let p: Packet = serde_json::from_str(packet).map_err(error)?;
        let plaintext = if let Some(session) = self.sessions.get_mut(&p.session) {
            if self.peers.get(&p.session).map(String::as_str) != Some(peer_curve) {
                return Err(error("peer"));
            }
            session.decrypt(&p.message).map_err(error)?
        } else if let OlmMessage::PreKey(message) = p.message {
            if self.sessions.len() >= 256 {
                return Err(error("sessions"));
            }
            let created = self
                .account
                .create_inbound_session(
                    SessionConfig::version_1(),
                    Curve25519PublicKey::from_base64(peer_curve).map_err(error)?,
                    &message,
                )
                .map_err(error)?;
            if created.session.session_id() != p.session {
                return Err(error("session"));
            }
            self.peers.insert(p.session.clone(), peer_curve.into());
            self.sessions.insert(p.session.clone(), created.session);
            created.plaintext
        } else {
            return Err(error("unknown session"));
        };
        self.touched.insert(p.session.clone());
        self.active.insert(peer_curve.into(), p.session);
        String::from_utf8(plaintext).map_err(error)
    }
}
impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn expired_ratchets_retire_without_erasing_live_or_legacy_sessions() {
        let mut a = Engine::new();
        let mut b = Engine::new();
        let peers = serde_json::to_string(&vec![b.account.curve25519_key().to_base64()]).unwrap();
        a.maintenance(1000.0, &peers).unwrap();
        session(&mut a, &mut b);
        a.maintenance(1000.0, &peers).unwrap();
        let original = a.active[&b.account.curve25519_key().to_base64()].clone();
        session(&mut a, &mut b);
        a.maintenance(2000.0, &peers).unwrap();
        a.maintenance((32 * 86400 + 2000) as f64, &peers).unwrap();
        assert!(!a.sessions.contains_key(&original));
        assert_eq!(a.sessions.len(), 1);
        let mut saved: serde_json::Value = serde_json::from_str(&a.save().unwrap()).unwrap();
        saved.as_object_mut().unwrap().remove("activity");
        saved.as_object_mut().unwrap().remove("clock");
        let mut restored = Engine::restore(&saved.to_string()).unwrap();
        restored.maintenance(1_900_000_000.0, &peers).unwrap();
        assert_eq!(
            restored.sessions.len(),
            1,
            "legacy state must not lose ratchets on upgrade"
        );
        assert!(restored.maintenance(f64::NAN, &peers).is_err());
        restored
            .maintenance((1_900_000_000 + 32 * 86400) as f64, "[]")
            .unwrap();
        assert!(
            restored.sessions.is_empty(),
            "removed peer ratchets eventually retire"
        );
    }
    fn session(a: &mut Engine, b: &mut Engine) {
        let pre: serde_json::Value = serde_json::from_str(&b.prekeys().unwrap()).unwrap();
        let payload = pre["payload"].as_str().unwrap();
        let value: serde_json::Value = serde_json::from_str(payload).unwrap();
        assert!(Engine::verify(
            &b.account.ed25519_key().to_base64(),
            payload,
            pre["signature"].as_str().unwrap()
        ));
        a.outbound(
            &b.account.curve25519_key().to_base64(),
            value["keys"][0].as_str().unwrap(),
        )
        .unwrap();
    }
    #[test]
    fn bidirectional_ratchet_out_of_order_and_restoration() {
        let mut a = Engine::new();
        let mut b = Engine::new();
        session(&mut a, &mut b);
        let ac = a.account.curve25519_key().to_base64();
        let bc = b.account.curve25519_key().to_base64();
        let one = a.encrypt(&bc, "first").unwrap();
        assert_eq!(b.decrypt(&ac, &one).unwrap(), "first");
        let reply = b.encrypt(&ac, "reply").unwrap();
        assert_eq!(a.decrypt(&bc, &reply).unwrap(), "reply");
        let mut a = Engine::restore(&a.save().unwrap()).unwrap();
        let mut b = Engine::restore(&b.save().unwrap()).unwrap();
        let two = a.encrypt(&bc, "second").unwrap();
        let three = a.encrypt(&bc, "third").unwrap();
        assert_eq!(b.decrypt(&ac, &three).unwrap(), "third");
        assert_eq!(b.decrypt(&ac, &two).unwrap(), "second");
        assert!(b.decrypt(&ac, &two).is_err());
        let mut bad: serde_json::Value = serde_json::from_str(&three).unwrap();
        bad["session"] = serde_json::json!("forged");
        assert!(b.decrypt(&ac, &bad.to_string()).is_err());
    }
}
