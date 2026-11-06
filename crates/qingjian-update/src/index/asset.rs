//! 签名索引中的安装包地址、大小与内容摘要。

use serde::{Deserialize, Serialize};

/// 一个安装包：只看它是给哪个平台、哪种 CPU 的。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Asset {
    pub platform: String,

    /// `arm64` / `x86_64`；0.1.3 之前的索引没有这个字段。
    #[serde(default)]
    pub cpu: String,

    #[serde(default)]
    pub url: String,

    #[serde(default)]
    pub sha256: String,

    #[serde(default)]
    pub size: u64,
}
