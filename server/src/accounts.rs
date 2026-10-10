use crate::{
    app::{Api, App, Error},
    db::{Db, Session, now},
    settings::admin,
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
};
use rusqlite::{OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use zeroize::Zeroizing;

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Filter {
    #[serde(default)]
    search: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    page: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Update {
    password: String,
    action: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    seconds: u32,
    #[serde(default)]
    is_admin: bool,
}
const COLUMNS: &str = "u.id,u.email,u.verified,u.is_admin,u.created_at,u.last_login,u.banned_until,u.ban_reason,u.admin_note,(SELECT COUNT(*) FROM identities i WHERE i.user_id=u.id),(SELECT COUNT(*) FROM sessions s WHERE s.user_id=u.id AND s.expires>?)";
fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    let until: i64 = r.get(6)?;
    Ok(
        json!({"id":r.get::<_,i64>(0)?,"email":r.get::<_,String>(1)?,"verified":r.get::<_,bool>(2)?,"is_admin":r.get::<_,bool>(3)?,"created_at":r.get::<_,i64>(4)?,"last_login":r.get::<_,i64>(5)?,"banned_until":until,"banned":until == -1 || until>now(),"ban_reason":r.get::<_,String>(7)?,"admin_note":r.get::<_,String>(8)?,"identity_count":r.get::<_,i64>(9)?,"session_count":r.get::<_,i64>(10)?}),
    )
}
impl Db {
    fn account_list(&self, f: &Filter) -> anyhow::Result<Value> {
        let c = self.connection.lock().unwrap();
        let condition = "instr(lower(u.email),?)>0 AND (?='' OR (?='banned' AND (u.banned_until=-1 OR u.banned_until>?)) OR (?='active' AND u.verified=1 AND u.banned_until!=-1 AND u.banned_until<=?) OR (?='pending' AND u.verified=0) OR (?='admin' AND u.is_admin=1))";
        let search = f.search.trim().to_ascii_lowercase();
        let total: i64 = c.query_row(
            &format!("SELECT COUNT(*) FROM users u WHERE {condition}"),
            params![
                search,
                f.status,
                f.status,
                now(),
                f.status,
                now(),
                f.status,
                f.status
            ],
            |r| r.get(0),
        )?;
        let mut q = c.prepare(&format!(
            "SELECT {COLUMNS} FROM users u WHERE {condition} ORDER BY u.id DESC LIMIT 50 OFFSET ?"
        ))?;
        let accounts = q
            .query_map(
                params![
                    now(),
                    search,
                    f.status,
                    f.status,
                    now(),
                    f.status,
                    now(),
                    f.status,
                    f.status,
                    i64::from(f.page) * 50
                ],
                row,
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let stats = c.query_row("SELECT COUNT(*),COALESCE(SUM(verified=1),0),COALESCE(SUM(banned_until=-1 OR banned_until>?),0),COALESCE(SUM(is_admin=1),0),(SELECT COUNT(*) FROM sessions WHERE expires>?) FROM users",params![now(),now()],|r|Ok(json!({"total":r.get::<_,i64>(0)?,"verified":r.get::<_,i64>(1)?,"banned":r.get::<_,i64>(2)?,"admins":r.get::<_,i64>(3)?,"sessions":r.get::<_,i64>(4)?})))?;
        Ok(json!({"accounts":accounts,"total":total,"page":f.page,"page_size":50,"stats":stats}))
    }
    fn account_detail(&self, id: i64) -> anyhow::Result<Option<Value>> {
        let c = self.connection.lock().unwrap();
        let Some(mut account) = c
            .query_row(
                &format!("SELECT {COLUMNS} FROM users u WHERE u.id=?"),
                params![now(), id],
                row,
            )
            .optional()?
        else {
            return Ok(None);
        };
        let mut q=c.prepare("SELECT a.action,a.detail,a.created_at,u.email FROM account_audit a JOIN users u ON u.id=a.actor WHERE a.target=? ORDER BY a.id DESC LIMIT 30")?;
        account["audit"]=json!(q.query_map([id],|r|Ok(json!({"action":r.get::<_,String>(0)?,"detail":r.get::<_,String>(1)?,"created_at":r.get::<_,i64>(2)?,"actor":r.get::<_,String>(3)?})))?.collect::<rusqlite::Result<Vec<_>>>()?);
        // Neither ciphertext nor credential/session tokens are exposed to administrators.
        let mut q=c.prepare("SELECT name,uid,is_default FROM identities WHERE user_id=? ORDER BY is_default DESC,name LIMIT 100")?;
        account["identities"]=json!(q.query_map([id],|r|Ok(json!({"name":r.get::<_,String>(0)?,"uid":r.get::<_,String>(1)?,"is_default":r.get::<_,bool>(2)?})))?.collect::<rusqlite::Result<Vec<_>>>()?);
        Ok(Some(account))
    }
    pub(crate) fn manage_account(
        &self,
        actor: &Session,
        id: i64,
        action: &str,
        text: &str,
        seconds: u32,
        role: bool,
    ) -> Api<()> {
        let mut c = self.connection.lock().unwrap();
        let tx = c.transaction().map_err(anyhow::Error::from)?;
        let authorized:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM sessions s JOIN users u ON u.id=s.user_id WHERE s.hash=? AND u.id=? AND s.expires>? AND u.verified=1 AND u.is_admin=1 AND u.banned_until!=-1 AND u.banned_until<=?)",params![actor.hash,actor.user.id,now(),now()],|r|r.get(0)).map_err(anyhow::Error::from)?;
        if !authorized {
            return Err(Error(StatusCode::FORBIDDEN, "管理员会话已失效"));
        }
        let target = tx
            .query_row(
                "SELECT is_admin,verified,banned_until FROM users WHERE id=?",
                [id],
                |r| {
                    Ok((
                        r.get::<_, bool>(0)?,
                        r.get::<_, bool>(1)?,
                        r.get::<_, i64>(2)?,
                    ))
                },
            )
            .optional()
            .map_err(anyhow::Error::from)?
            .ok_or(Error(StatusCode::NOT_FOUND, "账号不存在"))?;
        if id == actor.user.id && matches!(action, "ban" | "role" | "revoke") {
            return Err(Error::bad(
                "不能对当前管理员执行此操作；退出自己请使用账号菜单",
            ));
        }
        if target.0 && (action == "ban" || (action == "role" && !role)) {
            let others:i64=tx.query_row("SELECT COUNT(*) FROM users WHERE id!=? AND is_admin=1 AND verified=1 AND banned_until!=-1 AND banned_until<=?",params![id,now()],|r|r.get(0)).map_err(anyhow::Error::from)?;
            if others == 0 {
                return Err(Error::bad("必须保留至少一个未封禁的已验证管理员"));
            }
        }
        let detail = match action {
            "ban" => {
                let until = if seconds == 0 {
                    -1
                } else {
                    now() + i64::from(seconds)
                };
                tx.execute(
                    "UPDATE users SET banned_until=?,ban_reason=? WHERE id=?",
                    params![until, text, id],
                )
                .map_err(anyhow::Error::from)?;
                format!("{text}；到期：{until}")
            }
            "unban" => {
                tx.execute(
                    "UPDATE users SET banned_until=0,ban_reason='' WHERE id=?",
                    [id],
                )
                .map_err(anyhow::Error::from)?;
                text.to_owned()
            }
            "note" => {
                tx.execute(
                    "UPDATE users SET admin_note=? WHERE id=?",
                    params![text, id],
                )
                .map_err(anyhow::Error::from)?;
                text.to_owned()
            }
            "role" => {
                if role && (!target.1 || target.2 == -1 || target.2 > now()) {
                    return Err(Error::bad("仅未封禁的已验证账号可成为管理员"));
                }
                tx.execute("UPDATE users SET is_admin=? WHERE id=?", params![role, id])
                    .map_err(anyhow::Error::from)?;
                if role {
                    "授予网站管理员".into()
                } else {
                    "取消网站管理员".into()
                }
            }
            "revoke" => text.to_owned(),
            _ => return Err(Error::bad("不支持的账号操作")),
        };
        if matches!(action, "ban" | "revoke" | "role") {
            tx.execute("DELETE FROM sessions WHERE user_id=?", [id])
                .map_err(anyhow::Error::from)?;
        }
        tx.execute(
            "INSERT INTO account_audit(actor,target,action,detail,created_at) VALUES(?,?,?,?,?)",
            params![actor.user.id, id, action, detail, now()],
        )
        .map_err(anyhow::Error::from)?;
        tx.commit().map_err(anyhow::Error::from)?;
        Ok(())
    }
}
pub async fn list(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Query(f): Query<Filter>,
) -> Api<Json<Value>> {
    if f.search.len() > 254
        || f.page > 100000
        || !matches!(
            f.status.as_str(),
            "" | "active" | "pending" | "banned" | "admin"
        )
    {
        return Err(Error::bad("筛选条件无效"));
    }
    app.work(move |a| {
        admin(a, &headers)?;
        let mut result = a.db.account_list(&f)?;
        for account in result["accounts"].as_array_mut().unwrap() {
            account["connections"] = json!(a.connections.count(account["id"].as_i64().unwrap()));
        }
        Ok(Json(result))
    })
    .await
}
pub async fn detail(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Api<Json<Value>> {
    app.work(move |a| {
        admin(a, &headers)?;
        let mut value =
            a.db.account_detail(id)?
                .ok_or(Error(StatusCode::NOT_FOUND, "账号不存在"))?;
        value["connections"] = json!(a.connections.count(id));
        Ok(Json(value))
    })
    .await
}
pub async fn update(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(body): Json<Update>,
) -> Api<Json<Value>> {
    if body.text.chars().count() > 500
        || body.text.chars().any(|c| c.is_control() && c != '\n')
        || body.seconds > 366 * 86400
        || body.action == "ban" && body.text.trim().is_empty()
    {
        return Err(Error::bad("封禁需填写原因，文本最多500字，期限最长366天"));
    }
    app.expensive(move |a| {
        admin(a, &headers)?;
        let password = Zeroizing::new(body.password);
        let actor = a.reauthenticate(&headers, &password)?;
        a.db.manage_account(
            &actor,
            id,
            &body.action,
            body.text.trim(),
            body.seconds,
            body.is_admin,
        )?;
        if matches!(body.action.as_str(), "ban" | "revoke" | "role") {
            a.connections.cancel(id, None, None);
        }
        Ok(Json(
            json!({"message":"账号操作已生效；封禁、强制退出或角色变更会撤销旧登录与连接。"}),
        ))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transaction_rechecks_a_revoked_administrator_before_writing() {
        let db = Db::open(":memory:").unwrap();
        let (admin_id, _, token) = db.register("admin@example.com", "fixture hash").unwrap();
        db.consume_email_token(&token, "verify", None).unwrap();
        db.grant_admin("admin@example.com").unwrap();
        let token = db.create_session(admin_id, false, "fixture hash").unwrap();
        let actor = db.session(&token).unwrap().unwrap();
        let (target, _, _) = db.register("target@example.com", "fixture hash").unwrap();
        db.revoke(admin_id, None).unwrap();
        let error = db
            .manage_account(&actor, target, "ban", "must not apply", 0, false)
            .unwrap_err();
        assert_eq!(error.0, StatusCode::FORBIDDEN);
        let value = db.account_detail(target).unwrap().unwrap();
        assert_eq!(value["banned"], false);
        assert!(value["audit"].as_array().unwrap().is_empty());
    }
}
