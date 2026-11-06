mod channel;

pub use channel::UpdateChannel;

use serde::{Deserialize, Serialize};

/// 配置文件 `[update]` 分节：检查更新。
///
/// 每天向猛人 GitHub 读签名版本索引，有新版在「关于」页提示；安装由用户点击更新发起。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdateConfig {
    /// 开着就每天检查一次。
    pub check: bool,

    /// 看哪个渠道的版本。
    pub channel: UpdateChannel,
}

// 猛人使用自己的签名更新源，默认开启检查。
#[allow(clippy::derivable_impls)]
impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            check: true,
            channel: UpdateChannel::default(),
        }
    }
}
