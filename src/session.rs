//! 会话模块：tower-sessions 登录会话、CSRF token、后台鉴权与登录限流。
//!
//! 会话存储于 SQLite 表 `hancic_sessions`（`tower-sessions-sqlx-store` 0.15，
//! 与 `tower-sessions` 0.14 同族：二者都基于 `tower-sessions-core` 0.14）。

use crate::auth;
use crate::db::Db;
use crate::error::AppError;
use rand::Rng;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tower_sessions::{Expiry, Session, SessionManagerLayer};
use tower_sessions_sqlx_store::SqliteStore;

/// 会话存储表名。
pub const SESSION_TABLE: &str = "hancic_sessions";
/// 会话 cookie 名。
pub const SESSION_COOKIE: &str = "hancic_session";
/// 会话中管理员用户标识的键。
pub const USER_ID_KEY: &str = "user_id";
/// 管理员用户 id。
pub const ADMIN_ID: i64 = 1;

const CSRF_KEY: &str = "csrf";

/// 登录限流窗口：3 分钟。
const LIMIT_WINDOW_SECS: i64 = 180;
/// 窗口内允许的最大失败次数。
const LIMIT_MAX_FAILURES: i64 = 5;

/// 构建会话中间件层：cookie 名 `hancic_session`，httpOnly + SameSite=Lax，
/// Secure 由部署侧 TLS 处理，会话随浏览器关闭过期（OnSessionEnd）。
pub fn session_layer(pool: &Db) -> SessionManagerLayer<SqliteStore> {
    let store = SqliteStore::new(pool.clone())
        .with_table_name(SESSION_TABLE)
        .expect("hancic_sessions 为合法表名");
    SessionManagerLayer::new(store)
        .with_name(SESSION_COOKIE)
        .with_http_only(true)
        .with_same_site(tower_sessions::cookie::SameSite::Lax)
        .with_secure(false)
        .with_expiry(Expiry::OnSessionEnd)
}

/// 创建会话表（幂等），启动时调用。
pub async fn migrate(pool: &Db) -> Result<(), AppError> {
    let store = SqliteStore::new(pool.clone())
        .with_table_name(SESSION_TABLE)
        .map_err(internal)?;
    store.migrate().await.map_err(internal)
}

/// 要求会话为已登录管理员（`user_id == ADMIN_ID`），否则 Unauthorized。
pub async fn require_admin(session: &Session) -> Result<(), AppError> {
    match session.get::<i64>(USER_ID_KEY).await {
        Ok(Some(ADMIN_ID)) => Ok(()),
        _ => Err(AppError::Unauthorized("请先登录".into())),
    }
}

/// 校验密码并写入登录会话。argon2 校验为 CPU 密集操作，走 `spawn_blocking` 避免阻塞 executor。
pub async fn login(db: &Db, session: &Session, password: &str) -> Result<(), AppError> {
    let Some(hash) = auth::get_password_hash(db).await? else {
        return Err(AppError::BadRequest("尚未设置管理员密码".into()));
    };
    let password = password.to_string();
    let ok = tokio::task::spawn_blocking(move || auth::verify_password(&password, &hash))
        .await
        .map_err(internal)?;
    if !ok {
        return Err(AppError::Unauthorized("密码错误".into()));
    }
    session
        .insert(USER_ID_KEY, ADMIN_ID)
        .await
        .map_err(internal)?;
    Ok(())
}

/// 登出：清空会话数据并删除会话记录（中间件随后向浏览器下发清除 cookie）。
///
/// 使用 `flush` 而非 `delete`：`delete` 只删存储记录，内存数据仍在，会被中间件重新保存；
/// `flush` 同时清空数据与 session id，触发中间件移除 cookie 分支。
pub async fn logout(session: &Session) -> Result<(), AppError> {
    session.flush().await.map_err(internal)
}

/// 惰性生成/复用 32 字节随机 hex 的 CSRF token，并存入会话。
pub async fn csrf_token(session: &Session) -> Result<String, AppError> {
    if let Some(token) = session.get::<String>(CSRF_KEY).await.map_err(internal)? {
        return Ok(token);
    }
    let token = random_hex(32);
    session.insert(CSRF_KEY, &token).await.map_err(internal)?;
    Ok(token)
}

/// 校验表单提交的 CSRF token 与会话中存储的一致，否则 Forbidden。
pub async fn verify_csrf(session: &Session, provided: Option<&str>) -> Result<(), AppError> {
    let expected = csrf_token(session).await?;
    if provided == Some(expected.as_str()) {
        Ok(())
    } else {
        Err(AppError::Forbidden("CSRF 校验失败".into()))
    }
}

/// 登录限流器：同 IP 3 分钟窗口内失败 ≥5 次则拒绝后续尝试。
///
/// 记录结构 `ip -> (失败次数, 窗口内首次失败时间戳)`，仅存于内存（进程级）。
#[derive(Clone)]
pub struct LoginLimiter {
    inner: Arc<Mutex<HashMap<String, (i64, i64)>>>,
}

impl LoginLimiter {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn now_secs() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }

    /// 该 IP 本次是否允许尝试登录。
    ///
    /// `allowed = true`（密码校验已通过）时清除失败记录并放行；
    /// 否则在 3 分钟窗口内失败次数 ≥5 即拒绝（窗口过期自动放行并清除记录）。
    pub fn check(&self, ip: &str, allowed: bool) -> bool {
        let mut map = self.inner.lock().expect("login limiter 锁可用");
        if allowed {
            map.remove(ip);
            return true;
        }
        match map.get(ip) {
            None => true,
            Some(&(_, first)) if Self::now_secs() - first > LIMIT_WINDOW_SECS => {
                map.remove(ip);
                true
            }
            Some(&(count, _)) => count < LIMIT_MAX_FAILURES,
        }
    }

    /// 记录一次失败，返回该 IP 在窗口内的累计失败次数。
    pub fn record_failure(&self, ip: &str) -> u32 {
        let mut map = self.inner.lock().expect("login limiter 锁可用");
        let now = Self::now_secs();
        let entry = map.entry(ip.to_string()).or_insert((0, now));
        if now - entry.1 > LIMIT_WINDOW_SECS {
            *entry = (1, now);
        } else {
            entry.0 += 1;
        }
        entry.0 as u32
    }
}

impl Default for LoginLimiter {
    fn default() -> Self {
        Self::new()
    }
}

fn internal(e: impl std::fmt::Display) -> AppError {
    AppError::Internal(e.to_string())
}

fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::rng().fill_bytes(&mut buf);
    buf.iter().map(|b| format!("{b:02x}")).collect()
}
