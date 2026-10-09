use crate::{
    identity,
    vault::{Vault, token},
};
use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::sync::Mutex;

pub struct Db {
    pub connection: Mutex<Connection>,
}
#[derive(Serialize, Clone)]
pub struct User {
    pub id: i64,
    pub email: String,
    pub is_admin: bool,
}
#[derive(Serialize)]
pub struct IdentityRow {
    pub id: String,
    pub name: String,
    pub uid: String,
    pub is_default: bool,
}
pub struct IdentitySecret {
    pub id: String,
    pub name: String,
    pub uid: String,
    pub ciphertext: Vec<u8>,
}
pub struct Session {
    pub user: User,
    pub hash: String,
}
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}
pub fn digest(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

impl Db {
    pub fn open(path: &str) -> Result<Self> {
        let connection = Connection::open(path)?;
        connection.busy_timeout(std::time::Duration::from_secs(3))?;
        connection.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL;
          CREATE TABLE IF NOT EXISTS users(id INTEGER PRIMARY KEY, email TEXT UNIQUE NOT NULL, password_hash TEXT NOT NULL, verified INTEGER NOT NULL DEFAULT 0);
          CREATE TABLE IF NOT EXISTS sessions(hash TEXT PRIMARY KEY, user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE, expires INTEGER NOT NULL);
          CREATE TABLE IF NOT EXISTS email_tokens(hash TEXT PRIMARY KEY, user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE, purpose TEXT NOT NULL, expires INTEGER NOT NULL);
          CREATE TABLE IF NOT EXISTS identities(id TEXT PRIMARY KEY, user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE, name TEXT NOT NULL, uid TEXT NOT NULL, ciphertext BLOB NOT NULL, is_default INTEGER NOT NULL DEFAULT 0, UNIQUE(user_id,uid));
          CREATE UNIQUE INDEX IF NOT EXISTS one_default_identity ON identities(user_id) WHERE is_default=1;")?;
        let has_admin: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('users') WHERE name='is_admin')",
            [],
            |r| r.get(0),
        )?;
        if !has_admin {
            connection.execute_batch(
                "ALTER TABLE users ADD COLUMN is_admin INTEGER NOT NULL DEFAULT 0",
            )?;
        }
        for (column, definition) in [
            ("created_at", "INTEGER NOT NULL DEFAULT 0"),
            ("last_login", "INTEGER NOT NULL DEFAULT 0"),
            ("banned_until", "INTEGER NOT NULL DEFAULT 0"),
            ("ban_reason", "TEXT NOT NULL DEFAULT ''"),
            ("admin_note", "TEXT NOT NULL DEFAULT ''"),
        ] {
            let exists: bool = connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info('users') WHERE name=?)",
                [column],
                |r| r.get(0),
            )?;
            if !exists {
                connection.execute_batch(&format!(
                    "ALTER TABLE users ADD COLUMN {column} {definition}"
                ))?;
            }
        }
        connection.execute_batch("CREATE TABLE IF NOT EXISTS account_audit(id INTEGER PRIMARY KEY, actor INTEGER NOT NULL REFERENCES users(id), target INTEGER NOT NULL REFERENCES users(id), action TEXT NOT NULL, detail TEXT NOT NULL, created_at INTEGER NOT NULL); CREATE INDEX IF NOT EXISTS account_audit_target ON account_audit(target,id)")?;
        connection.execute_batch("CREATE TABLE IF NOT EXISTS profiles(user_id INTEGER PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE, data TEXT NOT NULL); CREATE TABLE IF NOT EXISTS site_settings(id INTEGER PRIMARY KEY CHECK(id=1), ciphertext BLOB NOT NULL)")?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }
    pub fn grant_admin(&self, email: &str) -> Result<()> {
        let changed = self.connection.lock().unwrap().execute(
            "UPDATE users SET is_admin=1 WHERE email=? AND verified=1 AND banned_until!=-1 AND banned_until<=?",
            params![email, now()],
        )?;
        if changed != 1 {
            bail!("管理员必须是已验证的现有账号");
        }
        Ok(())
    }
    pub fn settings(&self) -> Result<Option<Vec<u8>>> {
        Ok(self
            .connection
            .lock()
            .unwrap()
            .query_row("SELECT ciphertext FROM site_settings WHERE id=1", [], |r| {
                r.get(0)
            })
            .optional()?)
    }
    pub fn save_settings(&self, owner: i64, session: &str, ciphertext: &[u8]) -> Result<()> {
        let changed = self.connection.lock().unwrap().execute("INSERT INTO site_settings(id,ciphertext) SELECT 1,? WHERE EXISTS(SELECT 1 FROM sessions s JOIN users u ON s.user_id=u.id WHERE s.hash=? AND u.id=? AND s.expires>? AND u.verified=1 AND u.is_admin=1) ON CONFLICT(id) DO UPDATE SET ciphertext=excluded.ciphertext", params![ciphertext,session,owner,now()])?;
        if changed != 1 {
            bail!("管理员会话已失效");
        }
        Ok(())
    }
    // Local deployment operator already controls the database and vault key.
    // HTTP callers must continue using session-checked save_settings above.
    pub fn configure_locally(&self, ciphertext: &[u8]) -> Result<()> {
        self.connection.lock().unwrap().execute("INSERT INTO site_settings(id,ciphertext) VALUES(1,?) ON CONFLICT(id) DO UPDATE SET ciphertext=excluded.ciphertext", [ciphertext])?;
        Ok(())
    }
    pub fn password(&self, email: &str) -> Result<Option<(i64, String, bool)>> {
        Ok(self
            .connection
            .lock()
            .unwrap()
            .query_row(
                "SELECT id,password_hash,(verified=1 AND banned_until!=-1 AND banned_until<=?) FROM users WHERE email=?",
                params![now(),email],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?)
    }
    pub fn password_by_id(&self, user_id: i64) -> Result<String> {
        Ok(self.connection.lock().unwrap().query_row(
            "SELECT password_hash FROM users WHERE id=?",
            [user_id],
            |r| r.get(0),
        )?)
    }
    pub fn register(&self, email: &str, hash: &str) -> Result<(i64, bool, String)> {
        let value = token()?;
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction()?;
        let id = if let Some((id, verified)) = tx
            .query_row(
                "SELECT id,verified FROM users WHERE email=?",
                [email],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, bool>(1)?)),
            )
            .optional()?
        {
            if verified {
                return Ok((id, true, String::new()));
            }
            tx.execute(
                "UPDATE users SET password_hash=? WHERE id=?",
                params![hash, id],
            )?;
            id
        } else {
            tx.execute(
                "INSERT INTO users(email,password_hash,created_at) VALUES(?,?,?)",
                params![email, hash, now()],
            )?;
            tx.last_insert_rowid()
        };
        // Pending registration and its link change together: an older link must
        // never activate a password submitted by a different registration.
        tx.execute("DELETE FROM email_tokens WHERE user_id=?", [id])?;
        tx.execute(
            "INSERT INTO email_tokens VALUES(?,?,?,?)",
            params![digest(&value), id, "verify", now() + 900],
        )?;
        tx.commit()?;
        Ok((id, false, value))
    }
    pub fn email_token(&self, owner: i64, purpose: &str) -> Result<String> {
        if !matches!(purpose, "verify" | "reset") {
            bail!("令牌用途无效");
        }
        let value = token()?;
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction()?;
        if purpose == "reset" {
            let verified: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM users WHERE id=? AND verified=1)",
                [owner],
                |r| r.get(0),
            )?;
            if !verified {
                bail!("账号尚未验证");
            }
        }
        tx.execute(
            "DELETE FROM email_tokens WHERE user_id=? AND purpose=?",
            params![owner, purpose],
        )?;
        tx.execute("DELETE FROM email_tokens WHERE expires<?", [now()])?;
        tx.execute(
            "INSERT INTO email_tokens VALUES(?,?,?,?)",
            params![digest(&value), owner, purpose, now() + 900],
        )?;
        tx.commit()?;
        Ok(value)
    }
    pub fn resend_verification(&self, email: &str) -> Result<Option<String>> {
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction()?;
        let owner: Option<i64> = tx
            .query_row(
                "SELECT id FROM users WHERE email=? AND verified=0",
                [email],
                |r| r.get(0),
            )
            .optional()?;
        let Some(owner) = owner else {
            return Ok(None);
        };
        let recent: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM email_tokens WHERE user_id=? AND purpose='verify' AND expires>?)", params![owner, now()+840], |r| r.get(0))?;
        if recent {
            return Ok(None);
        }
        let value = token()?;
        tx.execute(
            "DELETE FROM email_tokens WHERE user_id=? AND purpose='verify'",
            [owner],
        )?;
        tx.execute(
            "INSERT INTO email_tokens VALUES(?,?,?,?)",
            params![digest(&value), owner, "verify", now() + 900],
        )?;
        tx.commit()?;
        Ok(Some(value))
    }
    pub fn consume_email_token(
        &self,
        value: &str,
        purpose: &str,
        new_password: Option<&str>,
    ) -> Result<i64> {
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction()?;
        let owner: i64 = tx
            .query_row(
                "SELECT user_id FROM email_tokens WHERE hash=? AND purpose=? AND expires>?",
                params![digest(value), purpose, now()],
                |r| r.get(0),
            )
            .optional()?
            .context("链接无效或已过期")?;
        if purpose == "verify" {
            tx.execute("UPDATE users SET verified=1 WHERE id=?", [owner])?;
        } else if purpose == "reset" {
            let password = new_password.context("缺少新密码")?;
            let changed = tx.execute(
                "UPDATE users SET password_hash=? WHERE id=? AND verified=1",
                params![password, owner],
            )?;
            if changed != 1 {
                bail!("账号尚未验证");
            }
            tx.execute("DELETE FROM sessions WHERE user_id=?", [owner])?;
        } else {
            bail!("令牌用途无效");
        }
        tx.execute("DELETE FROM email_tokens WHERE user_id=?", [owner])?;
        tx.commit()?;
        Ok(owner)
    }
    pub fn create_session(
        &self,
        owner: i64,
        remembered: bool,
        verified_hash: &str,
    ) -> Result<String> {
        let value = token()?;
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction()?;
        let valid: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM users WHERE id=? AND verified=1 AND password_hash=? AND banned_until!=-1 AND banned_until<=?)",
            params![owner, verified_hash,now()],
            |r| r.get(0),
        )?;
        if !valid {
            bail!("登录凭据已失效，请重新登录");
        }
        tx.execute("DELETE FROM sessions WHERE expires<?", [now()])?;
        tx.execute(
            "UPDATE users SET last_login=? WHERE id=?",
            params![now(), owner],
        )?;
        tx.execute(
            "INSERT INTO sessions VALUES(?,?,?)",
            params![
                digest(&value),
                owner,
                now() + if remembered { 30 * 86400 } else { 12 * 3600 }
            ],
        )?;
        tx.commit()?;
        Ok(value)
    }
    pub fn session(&self, value: &str) -> Result<Option<Session>> {
        if value.len() != 64 {
            return Ok(None);
        }
        let hash = digest(value);
        let c = self.connection.lock().unwrap();
        let user = c.query_row("SELECT u.id,u.email,u.is_admin FROM sessions s JOIN users u ON u.id=s.user_id WHERE s.hash=? AND s.expires>? AND u.verified=1 AND u.banned_until!=-1 AND u.banned_until<=?", params![hash,now(),now()], |r| Ok(User {id:r.get(0)?,email:r.get(1)?,is_admin:r.get(2)?})).optional()?;
        Ok(user.map(|user| Session { user, hash }))
    }
    pub fn revoke(&self, owner: i64, hash: Option<&str>) -> Result<()> {
        let c = self.connection.lock().unwrap();
        if let Some(hash) = hash {
            c.execute(
                "DELETE FROM sessions WHERE user_id=? AND hash=?",
                params![owner, hash],
            )?;
        } else {
            c.execute("DELETE FROM sessions WHERE user_id=?", [owner])?;
        }
        Ok(())
    }
    pub fn identities(&self, owner: i64) -> Result<Vec<IdentityRow>> {
        let c = self.connection.lock().unwrap();
        let mut stmt = c.prepare("SELECT id,name,uid,is_default FROM identities WHERE user_id=? ORDER BY is_default DESC,name,id")?;
        Ok(stmt
            .query_map([owner], |r| {
                Ok(IdentityRow {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    uid: r.get(2)?,
                    is_default: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }
    pub fn add_identity(
        &self,
        owner: i64,
        name: &str,
        identity: &tsclientlib::Identity,
        vault: &Vault,
    ) -> Result<String> {
        let uid = identity::uid(identity);
        let id = token()?;
        let canonical = identity::canonical(identity);
        let ciphertext = vault.seal(owner, &id, &uid, canonical.as_bytes())?;
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction()?;
        if let Some(existing) = tx
            .query_row(
                "SELECT id FROM identities WHERE user_id=? AND uid=?",
                params![owner, uid],
                |r| r.get::<_, String>(0),
            )
            .optional()?
        {
            return Ok(existing);
        }
        let count: i64 = tx.query_row(
            "SELECT COUNT(*) FROM identities WHERE user_id=?",
            [owner],
            |r| r.get(0),
        )?;
        if count >= 100 {
            bail!("单账号最多保存100个身份");
        }
        tx.execute(
            "INSERT INTO identities VALUES(?,?,?,?,?,?)",
            params![id, owner, name, uid, ciphertext, count == 0],
        )?;
        tx.commit()?;
        Ok(id)
    }
    pub fn identity(&self, owner: i64, id: &str) -> Result<IdentitySecret> {
        self.connection
            .lock()
            .unwrap()
            .query_row(
                "SELECT id,name,uid,ciphertext FROM identities WHERE user_id=? AND id=?",
                params![owner, id],
                |r| {
                    Ok(IdentitySecret {
                        id: r.get(0)?,
                        name: r.get(1)?,
                        uid: r.get(2)?,
                        ciphertext: r.get(3)?,
                    })
                },
            )
            .optional()?
            .context("身份不存在")
    }
    pub fn edit_identity(
        &self,
        owner: i64,
        id: &str,
        name: &str,
        make_default: bool,
    ) -> Result<()> {
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction()?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM identities WHERE user_id=? AND id=?)",
            params![owner, id],
            |r| r.get(0),
        )?;
        if !exists {
            bail!("身份不存在");
        }
        if make_default {
            tx.execute(
                "UPDATE identities SET is_default=0 WHERE user_id=?",
                [owner],
            )?;
        }
        tx.execute("UPDATE identities SET name=?,is_default=CASE WHEN ? THEN 1 ELSE is_default END WHERE user_id=? AND id=?", params![name,make_default,owner,id])?;
        tx.commit()?;
        Ok(())
    }
    pub fn delete_identity(&self, owner: i64, id: &str) -> Result<()> {
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction()?;
        if tx.execute(
            "DELETE FROM identities WHERE user_id=? AND id=?",
            params![owner, id],
        )? == 0
        {
            bail!("身份不存在");
        }
        let default_exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM identities WHERE user_id=? AND is_default=1)",
            [owner],
            |r| r.get(0),
        )?;
        if !default_exists {
            tx.execute("UPDATE identities SET is_default=1 WHERE id=(SELECT id FROM identities WHERE user_id=? ORDER BY name,id LIMIT 1)",[owner])?;
        }
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn accounts() -> Db {
        let db = Db::open(":memory:").unwrap();
        for email in ["a@example.com", "b@example.com"] {
            let (_, _, t) = db.register(email, "hash").unwrap();
            db.consume_email_token(&t, "verify", None).unwrap();
        }
        db
    }
    #[test]
    fn timed_ban_expiry_and_password_recovery_never_restore_old_sessions() {
        let db = accounts();
        let original = db.create_session(1, true, "hash").unwrap();
        let reset = db.email_token(1, "reset").unwrap();
        db.connection
            .lock()
            .unwrap()
            .execute("UPDATE users SET banned_until=? WHERE id=1", [now() + 60])
            .unwrap();
        assert!(db.session(&original).unwrap().is_none());
        db.consume_email_token(&reset, "reset", Some("new_hash"))
            .unwrap();
        assert!(db.create_session(1, false, "new_hash").is_err());
        db.connection
            .lock()
            .unwrap()
            .execute("UPDATE users SET banned_until=? WHERE id=1", [now() - 1])
            .unwrap();
        assert!(db.session(&original).unwrap().is_none());
        assert!(db.create_session(1, false, "new_hash").is_ok());
    }
    #[test]
    fn recovery_is_single_use_revokes_sessions_and_keeps_decryptable_identities() {
        let db = accounts();
        let vault = Vault::new([1; 32]);
        let id = db
            .add_identity(1, "原身份", &tsclientlib::Identity::create(), &vault)
            .unwrap();
        let s = db.create_session(1, true, "hash").unwrap();
        let reset = db.email_token(1, "reset").unwrap();
        assert!(db.consume_email_token(&reset, "verify", None).is_err());
        db.consume_email_token(&reset, "reset", Some("new_hash"))
            .unwrap();
        assert!(db.session(&s).unwrap().is_none());
        assert!(db.consume_email_token(&reset, "reset", Some("x")).is_err());
        assert!(db.create_session(1, true, "hash").is_err());
        let record = db.identity(1, &id).unwrap();
        assert!(vault.open(1, &id, &record.uid, &record.ciphertext).is_ok());
    }
    #[test]
    fn owners_are_isolated_and_duplicates_are_deduplicated() {
        let db = accounts();
        let vault = Vault::new([1; 32]);
        let original = tsclientlib::Identity::create();
        let id = db.add_identity(1, "一", &original, &vault).unwrap();
        assert_eq!(id, db.add_identity(1, "重复", &original, &vault).unwrap());
        assert!(db.identity(2, &id).is_err());
        assert!(db.edit_identity(2, &id, "盗用", true).is_err());
        assert!(db.delete_identity(2, &id).is_err());
        let second = db
            .add_identity(1, "二", &tsclientlib::Identity::create(), &vault)
            .unwrap();
        db.edit_identity(1, &second, "二", true).unwrap();
        assert_eq!(
            db.identities(1)
                .unwrap()
                .iter()
                .filter(|i| i.is_default)
                .count(),
            1
        );
        db.delete_identity(1, &second).unwrap();
        assert!(db.identities(1).unwrap()[0].is_default);
    }
    #[test]
    fn expired_tokens_and_unverified_sessions_are_rejected() {
        let db = Db::open(":memory:").unwrap();
        let (id, _, _) = db.register("a@example.com", "hash").unwrap();
        assert!(db.create_session(id, false, "hash").is_err());
        let t = db.email_token(id, "verify").unwrap();
        db.connection
            .lock()
            .unwrap()
            .execute("UPDATE email_tokens SET expires=0", [])
            .unwrap();
        assert!(db.consume_email_token(&t, "verify", None).is_err());
    }
    #[test]
    fn pending_registration_cannot_activate_an_older_password() {
        let db = Db::open(":memory:").unwrap();
        let (_, _, old) = db.register("a@example.com", "attacker_hash").unwrap();
        let (id, _, new) = db.register("a@example.com", "owner_hash").unwrap();
        assert!(db.consume_email_token(&old, "verify", None).is_err());
        db.consume_email_token(&new, "verify", None).unwrap();
        assert_eq!(db.password_by_id(id).unwrap(), "owner_hash");
        assert!(db.create_session(id, false, "attacker_hash").is_err());
        assert!(db.create_session(id, false, "owner_hash").is_ok());
    }
    #[test]
    fn resend_preserves_password_and_limits_mail_without_reviving_old_links() {
        let db = Db::open(":memory:").unwrap();
        let (owner, _, old) = db.register("one@example.com", "original_hash").unwrap();
        assert!(
            db.resend_verification("missing@example.com")
                .unwrap()
                .is_none()
        );
        assert!(db.resend_verification("one@example.com").unwrap().is_none());
        db.connection
            .lock()
            .unwrap()
            .execute("UPDATE email_tokens SET expires=?", [now() + 800])
            .unwrap();
        let new = db.resend_verification("one@example.com").unwrap().unwrap();
        assert_eq!(db.password_by_id(owner).unwrap(), "original_hash");
        assert!(db.consume_email_token(&old, "verify", None).is_err());
        assert!(db.resend_verification("one@example.com").unwrap().is_none());
        db.consume_email_token(&new, "verify", None).unwrap();
        assert!(db.resend_verification("one@example.com").unwrap().is_none());
    }
}
