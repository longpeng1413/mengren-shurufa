//! 猛人更新：读取 GitHub 签名索引，按平台挑选新版，下载并校验安装包。
//! 桌面调度与安卓共用同一份版本、验签和内容校验逻辑；系统安装由平台壳负责。

#![forbid(unsafe_code)]

mod available;
#[cfg(feature = "desktop")]
mod checker;
mod download;
mod error;
mod http;
mod index;
mod target;
mod version;

pub use available::Available;
#[cfg(feature = "desktop")]
pub use checker::{Checker, UpdateState};
pub use download::download_release;
pub use error::UpdateError;
pub use index::{Asset, Index, PUBLIC_KEYS, Release, SCHEMA_VERSION, verify};
pub use target::Target;
pub use version::Version;

/// 下载页，提示里点开的就是它。
pub const DOWNLOAD_URL: &str = "https://github.com/longpeng1413/mengren-shurufa/releases/latest";

/// 显式检查本机版本，索引必须通过猛人发版公钥验签。
pub fn check(current: &str, target: Target, beta: bool) -> Result<Option<Available>, UpdateError> {
    check_with_proxy(current, target, beta, "")
}

/// 安卓从系统网络设置传入代理；留空使用客户端缺省代理。
pub fn check_with_proxy(
    current: &str,
    target: Target,
    beta: bool,
    proxy: &str,
) -> Result<Option<Available>, UpdateError> {
    let version = Version::parse(current).ok_or(UpdateError::InvalidPackage)?;
    Ok(index::fetch_index_with_proxy(current, proxy)?.for_target(&version, target, beta))
}
