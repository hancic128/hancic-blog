//! 核心数据模型与枚举。
//!
//! 枚举以 sqlite TEXT 直接存取：服务层通过 `to_str`/`from_str` 与数据库值互转，
//! 不手写 sqlx 编解码。`Post`/`Attachment` 的公共字段为枚举类型，便于 API 层与
//! 测试直接使用；数据库读写经内部行结构（String 字段）转换。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::sqlite::SqliteRow;
use sqlx::{FromRow, Row};

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

#[derive(Debug, Clone, Serialize)]
pub struct Post {
    pub id: i64,
    pub uuid: String,
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
    pub like_count: i64,
    pub category_id: Option<i64>,
    pub column_id: Option<i64>,
    pub column_sort: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Moment {
    pub id: i64,
    pub content: String,
    pub created_at: DateTime<Utc>,
    pub like_count: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LikeContentType {
    Post,
    Moment,
}

impl LikeContentType {
    pub fn to_str(&self) -> &'static str {
        match self {
            Self::Post => "post",
            Self::Moment => "moment",
        }
    }
}

impl std::str::FromStr for LikeContentType {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "moment" => Self::Moment,
            _ => Self::Post,
        })
    }
}

#[derive(Debug, Clone)]
pub struct ContentLike {
    pub id: i64,
    pub content_type: LikeContentType,
    pub content_id: i64,
    pub visitor_id: String,
    pub ip_hash: String,
    pub ua_hash: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl<'r> FromRow<'r, SqliteRow> for ContentLike {
    fn from_row(row: &'r SqliteRow) -> Result<Self, sqlx::Error> {
        let content_type: String = row.try_get("content_type")?;
        Ok(Self {
            id: row.try_get("id")?,
            content_type: content_type.parse().unwrap_or(LikeContentType::Post),
            content_id: row.try_get("content_id")?,
            visitor_id: row.try_get("visitor_id")?,
            ip_hash: row.try_get("ip_hash")?,
            ua_hash: row.try_get("ua_hash")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
        })
    }
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Category {
    pub id: i64,
    pub slug: String,
    pub name: String,
    pub sort_order: i64,
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Column {
    pub id: i64,
    pub slug: String,
    pub name: String,
    pub sort_order: i64,
    pub description: String,
}

/// 徒步轨迹（`trails` 表）：GPX 导入后存元数据与统计，坐标 JSON 在运行时目录。
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Trail {
    pub id: i64,
    pub name: String,
    pub description: String,
    /// GPX 相对路径（data/trails/xxx.gpx）
    pub file_path: String,
    /// 开始时间（轨迹首点时间，ISO RFC3339）
    pub started_at: Option<DateTime<Utc>>,
    /// 里程（米）
    pub distance_m: Option<f64>,
    /// 累计爬升（米）
    pub elevation_gain_m: Option<f64>,
    /// 累计下降（米）
    pub elevation_loss_m: Option<f64>,
    /// 运动时长（秒）
    pub moving_seconds: Option<i64>,
    /// 平均速度（km/h）
    pub avg_speed_kmh: Option<f64>,
    /// 最高海拔（米）
    pub max_elevation_m: Option<f64>,
    /// 最低海拔（米）
    pub min_elevation_m: Option<f64>,
    /// 起点坐标
    pub start_lat: Option<f64>,
    pub start_lon: Option<f64>,
    /// 终点坐标
    pub end_lat: Option<f64>,
    pub end_lon: Option<f64>,
    /// 抽稀后坐标 JSON `[[lat,lon],...]`（总览地图用）
    pub simplified: String,
    /// 原始轨迹点数
    pub point_count: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Tag {
    pub id: i64,
    pub slug: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
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
