//! 可用的新版本以及签名索引中对应的安装包。

use serde::{Deserialize, Serialize};

/// 查到的新版本，够菜单和「关于」页显示用。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Available {
    pub version: String,

    /// 索引里的渠道：`alpha` / `beta` / `rc` / `stable`。
    pub channel: String,

    pub date: String,

    /// 更新日志，一行一条。
    pub notes: Vec<String>,

    /// 签名索引中的本机安装包。旧缓存可缺省，安装前会重新验签查询。
    #[serde(default)]
    pub asset: Option<crate::Asset>,
}
