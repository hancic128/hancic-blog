//! 核心数据模型与枚举。
//!
//! 枚举以 sqlite TEXT 直接存取：服务层通过 `to_str`/`from_str` 与数据库值互转，
//! 不手写 sqlx 编解码。`Post`/`Attachment` 的公共字段为枚举类型，便于 API 层与
//! 测试直接使用；数据库读写经内部行结构（String 字段）转换。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PostStatus {
    #[default]
    Draft,
    Published,
}

impl PostStatus {
    pub fn to_str(&self) -> &'static str {
        match self {
            PostStatus::Draft => "draft",
            PostStatus::Published => "published",
        }
    }
}

impl std::str::FromStr for PostStatus {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "published" => Self::Published,
            _ => Self::Draft,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PostType {
    #[default]
    Post,
    Page,
}

impl PostType {
    pub fn to_str(&self) -> &'static str {
        match self {
            PostType::Post => "post",
            PostType::Page => "page",
        }
    }
}

impl std::str::FromStr for PostType {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "page" => Self::Page,
            _ => Self::Post,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AttachmentKind {
    #[default]
    Image,
    Video,
    File,
}

impl AttachmentKind {
    pub fn to_str(&self) -> &'static str {
        match self {
            AttachmentKind::Image => "image",
            AttachmentKind::Video => "video",
            AttachmentKind::File => "file",
        }
    }
}

impl std::str::FromStr for AttachmentKind {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "video" => Self::Video,
            "file" => Self::File,
            _ => Self::Image,
        })
    }
}

#[derive(Debug, Clone)]
pub struct Post {
    pub id: i64,
    pub slug: String,
    pub title: String,
    pub content_md: String,
    pub excerpt: String,
    pub status: PostStatus,
    pub post_type: PostType,
    pub published_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub views: i64,
    pub category_id: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct Moment {
    pub id: i64,
    pub content: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct Category {
    pub id: i64,
    pub slug: String,
    pub name: String,
    pub sort_order: i64,
}

#[derive(Debug, Clone, FromRow)]
pub struct Tag {
    pub id: i64,
    pub slug: String,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct Attachment {
    pub id: i64,
    pub uuid_name: String,
    pub orig_name: String,
    pub mime: String,
    pub size: i64,
    pub kind: AttachmentKind,
    pub path: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct ApiToken {
    pub id: i64,
    pub token_hash: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub revoked_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PageView {
    pub id: i64,
    pub post_id: i64,
    pub ip: String,
    pub ua: String,
    pub referer: String,
    pub country: String,
    pub province: String,
    pub city: String,
    pub created_at: DateTime<Utc>,
}
