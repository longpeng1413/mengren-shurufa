//! 打开翻译窗口的全局快捷键配置：组合键与「双击右 Ctrl」，配置热加载时整体比较、整体交给钩子。

use qingjian_platform::{Config, KeyCombo};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TranslatorKeys {
    /// `[shortcut] translator`，`None` 是关掉组合键。
    pub combo: Option<KeyCombo>,

    /// `[shortcut] translator_double_ctrl`。
    pub double_ctrl: bool,

    /// `[shortcut] translate_double_shift`：双击右 Shift 原地翻译。
    pub double_shift: bool,
}

impl TranslatorKeys {
    pub fn from_config(config: &Config) -> Self {
        Self {
            combo: config.shortcut.translator,
            double_ctrl: config.shortcut.translator_double_ctrl,
            double_shift: config.shortcut.translate_double_shift,
        }
    }
}

impl Default for TranslatorKeys {
    fn default() -> Self {
        Self::from_config(&Config::default())
    }
}
