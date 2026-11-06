//! 有道配置，应用密钥只在运行时注入，不序列化到普通配置文件。

use serde::{Deserialize, Serialize};

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct YoudaoConfig {
    pub enabled: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub app_key: String,
    #[serde(skip)]
    pub secret: Option<String>,
}

impl std::fmt::Debug for YoudaoConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("YoudaoConfig")
            .field("enabled", &self.enabled)
            .field("app_key", &self.app_key)
            .finish_non_exhaustive()
    }
}
