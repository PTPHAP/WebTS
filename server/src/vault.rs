use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, Payload},
};
use anyhow::{Result, bail};
use zeroize::{Zeroize, Zeroizing};

pub struct Vault {
    key: Zeroizing<[u8; 32]>,
}

impl Vault {
    pub fn new(key: [u8; 32]) -> Self {
        Self {
            key: Zeroizing::new(key),
        }
    }
    pub fn seal(&self, owner: i64, id: &str, uid: &str, plaintext: &[u8]) -> Result<Vec<u8>> {
        let mut nonce = [0; 12];
        getrandom::fill(&mut nonce).map_err(|_| anyhow::anyhow!("安全随机数不可用"))?;
        let aad = format!("web-ts:identity:v1:{owner}:{id}:{uid}");
        let cipher = Aes256Gcm::new_from_slice(self.key.as_ref()).unwrap();
        let encrypted = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: plaintext,
                    aad: aad.as_bytes(),
                },
            )
            .map_err(|_| anyhow::anyhow!("身份加密失败"))?;
        let mut record = vec![1];
        record.extend_from_slice(&nonce);
        record.extend(encrypted);
        Ok(record)
    }
    pub fn open(
        &self,
        owner: i64,
        id: &str,
        uid: &str,
        record: &[u8],
    ) -> Result<Zeroizing<Vec<u8>>> {
        if record.len() < 29 || record[0] != 1 {
            bail!("身份密文格式无效");
        }
        let aad = format!("web-ts:identity:v1:{owner}:{id}:{uid}");
        let cipher = Aes256Gcm::new_from_slice(self.key.as_ref()).unwrap();
        let plaintext = cipher
            .decrypt(
                Nonce::from_slice(&record[1..13]),
                Payload {
                    msg: &record[13..],
                    aad: aad.as_bytes(),
                },
            )
            .map_err(|_| anyhow::anyhow!("身份解密失败：密钥或数据不匹配"))?;
        Ok(Zeroizing::new(plaintext))
    }
}

pub fn token() -> Result<String> {
    let mut bytes = [0; 32];
    getrandom::fill(&mut bytes).map_err(|_| anyhow::anyhow!("安全随机数不可用"))?;
    let token = hex::encode(bytes);
    bytes.zeroize();
    Ok(token)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tampering_wrong_key_and_cross_account_substitution_are_rejected() {
        let vault = Vault::new([7; 32]);
        let plaintext = b"private-key-fixture";
        let mut sealed = vault.seal(1, "a", "uid", plaintext).unwrap();
        assert!(!sealed.windows(plaintext.len()).any(|w| w == plaintext));
        assert_eq!(
            vault.open(1, "a", "uid", &sealed).unwrap().as_slice(),
            plaintext
        );
        assert!(vault.open(2, "a", "uid", &sealed).is_err());
        assert!(vault.open(1, "b", "uid", &sealed).is_err());
        assert!(vault.open(1, "a", "other", &sealed).is_err());
        assert!(Vault::new([8; 32]).open(1, "a", "uid", &sealed).is_err());
        sealed[13] ^= 1;
        assert!(vault.open(1, "a", "uid", &sealed).is_err());
    }
    #[test]
    fn same_input_uses_unique_nonces() {
        let vault = Vault::new([7; 32]);
        assert_ne!(
            vault.seal(1, "a", "uid", b"key").unwrap(),
            vault.seal(1, "a", "uid", b"key").unwrap()
        );
    }
}
