//! 一条常用语。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Phrase {
    pub text: String,

    /// 所在分组（「目录」）。
    pub group: String,

    /// 输入码：小写字母或纯数字（数字短语），空为没有。
    #[serde(default)]
    pub code: String,

    #[serde(default = "default_weight")]
    pub weight: u32,

    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_weight() -> u32 {
    10
}
fn default_enabled() -> bool {
    true
}

impl Phrase {
    pub fn from_custom(phrase: &qingjian_core::CustomPhrase) -> Self {
        Self {
            text: phrase.text.clone(),
            code: phrase.code.clone(),
            group: super::DEFAULT_GROUP.into(),
            weight: phrase.weight,
            enabled: phrase.enabled,
        }
    }
}
