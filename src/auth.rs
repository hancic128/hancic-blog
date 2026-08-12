//! 认证模块：管理员密码的 argon2id 哈希与校验，哈希存于 settings 表。

use crate::db::Db;
use crate::error::AppError;
use crate::services::settings;
use argon2::password_hash::{PasswordHash, SaltString, rand_core::OsRng};
use argon2::{Argon2, PasswordHasher, PasswordVerifier};

pub const PASSWORD_MIN_LEN: usize = 8;
/// 密码长度上限（防超长输入拖慢 argon2 哈希）。
pub const PASSWORD_MAX_LEN: usize = 64;
const SETTINGS_KEY: &str = "admin_password_hash";

/// 密码强度校验：长度 8–64，且同时包含字母与数字。
pub fn validate_password_strength(pw: &str) -> Result<(), String> {
    let len = pw.chars().count();
    if len < PASSWORD_MIN_LEN {
        return Err(format!("密码至少 {PASSWORD_MIN_LEN} 个字符"));
    }
    if len > PASSWORD_MAX_LEN {
        return Err(format!("密码最长 {PASSWORD_MAX_LEN} 个字符"));
    }
    let has_alpha = pw.chars().any(|c| c.is_ascii_alphabetic());
    let has_digit = pw.chars().any(|c| c.is_ascii_digit());
    if !has_alpha || !has_digit {
        return Err("密码需同时包含字母和数字".into());
    }
    Ok(())
}

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

/// 读取已存储的密码哈希（未设置密码时返回 None）。
pub async fn get_password_hash(db: &Db) -> Result<Option<String>, AppError> {
    settings::get(db, SETTINGS_KEY).await
}
