//! 取词（`[lookup]` 分节，Windows）：划词后弹「译」图标、悬停取词、弹出卡片时自动朗读外文。翻译窗口的「取词」菜单改这里。

use serde::{Deserialize, Serialize};

use super::HoverMode;

/// `[lookup]`：划词翻译与查词发音。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LookupConfig {
    /// 用鼠标拖选或双击选中文字后，在鼠标旁显示「译」图标，点它弹出词典卡片或译文。默认关。
    pub selection: bool,

    /// 卡片弹出时自动朗读外文：英文单词与外文句子读原文，中文读译文。默认关。
    pub auto_speak: bool,

    /// 悬停取词：鼠标停在词上弹词典卡片。默认关。
    pub hover: HoverMode,
}
