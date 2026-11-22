//! 翻译相关的全局触发配置：组合键、「双击右 Ctrl」与划词翻译，配置热加载时整体比较、整体交给钩子。

use qingjian_platform::{Config, HoverMode, KeyCombo};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TranslatorKeys {
    /// `[shortcut] translator`，`None` 是关掉组合键。
    pub combo: Option<KeyCombo>,

    /// `[shortcut] translator_double_ctrl`。
    pub double_ctrl: bool,

    /// `[shortcut] translate_double_shift`：双击右 Shift 原地翻译。
    pub double_shift: bool,

    /// `[lookup] selection`：拖选或双击选中文字后显示「译」图标。
    pub selection: bool,

    /// `[lookup] hover`：鼠标停在词上弹词典卡片。
    pub hover: HoverMode,
}

impl TranslatorKeys {
    pub fn from_config(config: &Config) -> Self {
        Self {
            combo: config.shortcut.translator,
            double_ctrl: config.shortcut.translator_double_ctrl,
            double_shift: config.shortcut.translate_double_shift,
            selection: config.lookup.selection,
            hover: config.lookup.hover,
        }
    }
}

impl Default for TranslatorKeys {
    fn default() -> Self {
        Self::from_config(&Config::default())
    }
}
