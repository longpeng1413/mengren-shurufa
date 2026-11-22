//! 悬停取词的触发方式（`[lookup] hover`，Windows）。

use serde::{Deserialize, Serialize};

/// 鼠标停在词上时怎样弹词典卡片。按字符串读写，写错的值按关闭处理，不让整份配置读不出来。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum HoverMode {
    /// 不悬停取词。
    #[default]
    Off,

    /// 按住 Ctrl、鼠标停在词上才取（不会被随手一停打扰）。
    Ctrl,

    /// 鼠标停在词上一会儿就取。
    Always,
}

impl HoverMode {
    /// 写进配置文件的值。
    pub fn key(self) -> &'static str {
        match self {
            HoverMode::Off => "off",
            HoverMode::Ctrl => "ctrl",
            HoverMode::Always => "always",
        }
    }
}

impl From<String> for HoverMode {
    fn from(value: String) -> Self {
        match value.trim() {
            "ctrl" => HoverMode::Ctrl,
            "always" => HoverMode::Always,
            _ => HoverMode::Off,
        }
    }
}

impl From<HoverMode> for String {
    fn from(mode: HoverMode) -> Self {
        mode.key().to_owned()
    }
}
