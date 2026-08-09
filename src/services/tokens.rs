//! API Token 服务：生成 / 校验 / 列表 / 吊销，以及明文一次性暂存。
//!
//! 明文形如 `hc_` + 32 字节 base64url（`hc_` 3 字符 + base64 43 字符 = 46 字符），
//! 仅生成瞬间返回给调用方（后台 created 页展示一次），库中只存 sha256 hex
//! （64 字符）。`verify` 遍历未吊销行的哈希做 `subtle::ConstantTimeEq`
//! 比对，避免按响应耗时区分命中与未命中。

use crate::db::Db;
use crate::error::AppError;
use crate::models::ApiToken;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use chrono::{DateTime, Utc};
use rand::rngs::SysRng;
use rand::TryRng;
use sha2::{Digest, Sha256};
use sqlx::FromRow;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use subtle::ConstantTimeEq;

/// 明文前缀（识别来源；校验时也要求该前缀）。
pub const TOKEN_PREFIX: &str = "hc_";
/// 随机段字节数：32 字节 → base64url 43 字符。
const TOKEN_RANDOM_BYTES: usize = 32;
/// 明文暂存 TTL：5 分钟内未展示（created 页）自动失效。
const PLAIN_TTL: Duration = Duration::from_secs(300);

/// 明文 sha256 hex。
pub fn hash(raw: &str) -> String {
    let digest = Sha256::digest(raw.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// 生成 Token：明文（`hc_` + 32 字节 base64url）仅在本函数返回，库存哈希。
pub async fn generate(db: &Db, name: &str) -> Result<(ApiToken, String), AppError> {
    let mut bytes = [0u8; TOKEN_RANDOM_BYTES];
    SysRng
        .try_fill_bytes(&mut bytes)
        .map_err(|e| AppError::Internal(format!("系统随机数生成失败: {e}")))?;
    let raw = format!("{TOKEN_PREFIX}{}", URL_SAFE_NO_PAD.encode(bytes));
    let token_hash = hash(&raw);
    let id = sqlx::query("INSERT INTO api_tokens(token_hash, name) VALUES (?, ?)")
        .bind(&token_hash)
        .bind(name)
        .execute(db)
        .await?
        .last_insert_rowid();
    let token = get(db, id)
        .await?
        .ok_or_else(|| AppError::Internal("Token 创建后读取失败".into()))?;
    Ok((token, raw))
}

/// 校验明文：存在未吊销且哈希匹配的 Token 即为有效。
///
/// 全量比对未吊销行的哈希（`ConstantTimeEq`），个人博客 Token 数量级小，
/// 代价可忽略；任一命中即有效。
pub async fn verify(db: &Db, raw: &str) -> bool {
    let computed = hash(raw);
    let Ok(rows) = sqlx::query_scalar::<_, String>(
        "SELECT token_hash FROM api_tokens WHERE revoked_at IS NULL",
    )
    .fetch_all(db)
    .await
    else {
        return false;
    };
    let computed_bytes = computed.as_bytes();
    let mut matched = false;
    for stored in rows {
        matched |= bool::from(stored.as_bytes().ct_eq(computed_bytes));
    }
    matched
}

/// 列出全部 Token（倒序：最新在前）。
pub async fn list(db: &Db) -> Result<Vec<ApiToken>, AppError> {
    let rows = sqlx::query_as::<_, TokenRow>(
        "SELECT id, token_hash, name, created_at, revoked_at FROM api_tokens \
         ORDER BY created_at DESC, id DESC",
    )
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().map(ApiToken::from).collect())
}

/// 按 id 读取 Token（created 展示页显示名称用）。
pub async fn get_by_id(db: &Db, id: i64) -> Result<Option<ApiToken>, AppError> {
    get(db, id).await
}

/// 吊销：置 revoked_at；已吊销或不存在返回 NotFound。
pub async fn revoke(db: &Db, id: i64) -> Result<(), AppError> {
    let r = sqlx::query(
        "UPDATE api_tokens SET revoked_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') \
         WHERE id = ? AND revoked_at IS NULL",
    )
    .bind(id)
    .execute(db)
    .await?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound("Token 不存在或已吊销".into()));
    }
    Ok(())
}

async fn get(db: &Db, id: i64) -> Result<Option<ApiToken>, AppError> {
    let row = sqlx::query_as::<_, TokenRow>(
        "SELECT id, token_hash, name, created_at, revoked_at FROM api_tokens WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(db)
    .await?;
    Ok(row.map(ApiToken::from))
}

#[derive(FromRow)]
struct TokenRow {
    id: i64,
    token_hash: String,
    name: String,
    created_at: DateTime<Utc>,
    revoked_at: Option<String>,
}

impl From<TokenRow> for ApiToken {
    fn from(r: TokenRow) -> Self {
        ApiToken {
            id: r.id,
            token_hash: r.token_hash,
            name: r.name,
            created_at: r.created_at,
            revoked_at: r.revoked_at,
        }
    }
}

/// 明文 Token 一次性暂存：生成后写入、created 页读取即删（仅显示一次）。
///
/// 进程内存级（重启即失），TTL 5 分钟兜底清理过期项；明文不落库不落日志。
#[derive(Clone, Default)]
pub struct PlainStore {
    inner: Arc<Mutex<HashMap<i64, (String, Instant)>>>,
}

impl PlainStore {
    /// 暂存明文（顺带清理过期项）。
    pub fn put(&self, id: i64, raw: String) {
        let mut map = self.inner.lock().expect("明文暂存锁可用");
        Self::sweep(&mut map);
        map.insert(id, (raw, Instant::now()));
    }

    /// 取出并删除明文（展示页读后即删）；过期或不存在返回 None。
    pub fn take(&self, id: i64) -> Option<String> {
        let mut map = self.inner.lock().expect("明文暂存锁可用");
        Self::sweep(&mut map);
        map.remove(&id).map(|(raw, _)| raw)
    }

    fn sweep(map: &mut HashMap<i64, (String, Instant)>) {
        map.retain(|_, (_, at)| at.elapsed() < PLAIN_TTL);
    }
}
