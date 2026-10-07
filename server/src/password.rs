//! CPU/memory intensive: HTTP callers must use a bounded blocking worker pool.
use anyhow::{Result, bail};
use argon2::{
    Algorithm, Argon2, Params, Version,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
};
use zeroize::Zeroizing;

fn engine() -> Argon2<'static> {
    Argon2::new(
        Algorithm::Argon2id,
        Version::V0x13,
        Params::new(19_456, 2, 1, Some(32)).unwrap(),
    )
}

pub fn hash(password: &str) -> Result<String> {
    if !(12..=128).contains(&password.len()) {
        bail!("密码需要12至128字节");
    }
    let mut salt_bytes = Zeroizing::new([0u8; 16]);
    getrandom::fill(salt_bytes.as_mut()).map_err(|_| anyhow::anyhow!("安全随机源不可用"))?;
    let salt = SaltString::encode_b64(salt_bytes.as_ref())
        .map_err(|_| anyhow::anyhow!("密码盐生成失败"))?;
    engine()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|_| anyhow::anyhow!("密码哈希失败"))
}

pub fn verify(password: &str, stored_hash: &str) -> bool {
    if password.len() > 128 {
        return false;
    }
    let Ok(parsed) = PasswordHash::new(stored_hash) else {
        return false;
    };
    // Accept only this project's bounded parameters; a corrupted database must
    // not request arbitrary memory or switch to a weaker hash algorithm.
    if parsed.algorithm.as_str() != "argon2id"
        || parsed.version != Some(19)
        || parsed.params.get_decimal("m") != Some(19_456)
        || parsed.params.get_decimal("t") != Some(2)
        || parsed.params.get_decimal("p") != Some(1)
    {
        return false;
    }
    engine()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn passwords_use_salted_argon2id_and_reject_wrong_or_invalid_inputs() {
        let password = "correct-password-123";
        let first = hash(password).unwrap();
        let second = hash(password).unwrap();
        assert!(first.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"));
        assert_ne!(first, second);
        assert!(verify(password, &first));
        assert!(!verify("incorrect-password", &first));
        assert!(!verify(password, "broken"));
        assert!(!verify(password, &first.replace("m=19456", "m=1073741824")));
        assert!(hash("short").is_err());
        assert!(hash(&"x".repeat(129)).is_err());
    }
}
