pub mod backup;
pub mod columns;
pub mod likes;
pub mod migrate;
pub mod moments;
pub mod posts;
pub mod settings;
pub mod stats;
pub mod taxonomy;
pub mod timezone;
pub mod tokens;
pub mod trails;
pub mod uploads;

/// 解压炸弹防护阈值（备份/迁移共用）：单条目 500MB、解压总量 2GB，超限报错。
pub const MAX_ENTRY_BYTES: u64 = 500 * 1024 * 1024;
pub const MAX_TOTAL_BYTES: u64 = 2 * 1024 * 1024 * 1024;
