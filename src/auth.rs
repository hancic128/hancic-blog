//! 认证模块：管理员密码的 argon2id 哈希与校验，哈希存于 settings 表。

use argon2::password_hash::{rand_core::OsRng, PasswordHash, SaltString};
use argon2::{Argon2, PasswordHasher, PasswordVerifier};
use crate::db::Db;
use crate::error::AppError;
use crate::services::settings;

pub const PASSWORD_MIN_LEN: usize = 8;
const SETTINGS_KEY: &str = "admin_password_hash";

pub fn hash_password(pw: &str) -> Result<String, AppError> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    argon2
        .hash_password(pw.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| AppError::Internal(format!("密码哈希失败: {e}")))
}

pub fn verify_password(pw: &str, hash: &str) -> bool {
    match PasswordHash::new(hash).and_then(|h| Argon2::default().verify_password(pw.as_bytes(), &h))
    {
        Ok(()) => true,
        Err(_) => false,
    }
}

pub async fn has_password(db: &Db) -> Result<bool, AppError> {
    Ok(settings::get(db, SETTINGS_KEY).await?.is_some())
}

pub async fn set_password(db: &Db, pw: &str) -> Result<(), AppError> {
    if pw.chars().count() < PASSWORD_MIN_LEN {
        return Err(AppError::BadRequest(format!(
            "密码至少 {PASSWORD_MIN_LEN} 个字符"
        )));
    }
    let hash = hash_password(pw)?;
    settings::set(db, SETTINGS_KEY, &hash).await
}
