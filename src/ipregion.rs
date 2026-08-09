//! IP 地区解析：封装 `ip2region` crate（xdb IPv4 库）。
//!
//! 对外只暴露 `Searcher::new(path)` + `lookup(&IpAddr) -> Region`，隔离底层
//! crate 的接口差异；私有/保留地址与解析失败统一归为「本地」。数据文件
//! `assets/ip2region.xdb`（约 11MB，见 `scripts/fetch-assets.sh`）经
//! `include_bytes!` 内嵌进二进制，由 `ensure_xdb` 在启动时写出到数据目录。

use ip2region::Searcher as RawSearcher;
use std::net::IpAddr;
use std::path::{Path, PathBuf};

/// 地区三元组：国家 / 省份 / 城市。
#[derive(Debug, Clone, Default)]
pub struct Region {
    pub country: String,
    pub province: String,
    pub city: String,
}

impl Region {
    /// 私有地址/解析失败的兜底地区。
    pub fn local() -> Self {
        Self {
            country: "本地".into(),
            province: String::new(),
            city: String::new(),
        }
    }
}

/// xdb 搜索器：持有完整数据副本（约 11MB），可跨线程共享（`AppState` 中 `Arc<Searcher>`）。
pub struct Searcher {
    raw: RawSearcher,
}

impl Searcher {
    pub fn new(xdb_path: &Path) -> Result<Self, String> {
        Ok(Self {
            raw: RawSearcher::new(xdb_path).map_err(|e| e.to_string())?,
        })
    }

    /// 解析 IP 地区；私有/保留地址与解析失败（含 xdb 未收录的 IPv6）返回「本地」。
    pub fn lookup(&self, ip: &IpAddr) -> Region {
        if is_local_address(ip) {
            return Region::local();
        }
        match self.raw.std_search(&ip.to_string()) {
            Ok(loc) => Region {
                country: loc.contry.unwrap_or_default(),
                province: loc.province.unwrap_or_default(),
                city: loc.city.unwrap_or_default(),
            },
            Err(_) => Region::local(),
        }
    }
}

/// 私有/保留地址判定：`IpAddr` 枚举本身只暴露 loopback/unspecified/multicast，
/// 私有网段（10/8、172.16/12、192.168/16、fc00::/7）与链路本地需按 v4/v6 具体判断。
fn is_local_address(ip: &IpAddr) -> bool {
    if ip.is_loopback() || ip.is_unspecified() || ip.is_multicast() {
        return true;
    }
    match ip {
        IpAddr::V4(v4) => v4.is_private() || v4.is_link_local(),
        IpAddr::V6(v6) => v6.is_unique_local() || v6.is_unicast_link_local(),
    }
}

/// 确保 `<data_dir>/ip2region.xdb` 存在（缺失时从内嵌资产写出），返回其路径。
pub fn ensure_xdb(data_dir: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(data_dir).map_err(|e| format!("创建数据目录失败: {e}"))?;
    let dest = data_dir.join("ip2region.xdb");
    if !dest.exists() {
        std::fs::write(&dest, include_bytes!("../assets/ip2region.xdb"))
            .map_err(|e| format!("写出 ip2region.xdb 失败: {e}"))?;
    }
    Ok(dest)
}
