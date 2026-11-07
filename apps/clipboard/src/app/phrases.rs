//! 自定义短语面板的数据来自用户库，不混进剪贴板历史。

use super::ClipboardApp;
use crate::system::{self, Payload};

impl ClipboardApp {
    pub(super) fn reload_phrases(&mut self) {
        #[cfg(windows)]
        {
            let Some(path) = &self.config else {
                return;
            };
            let Some(dir) = path.parent() else {
                return;
            };
            let Ok(config) = qingjian_platform::Config::load(path) else {
                return;
            };
            if let Ok(phrases) =
                qingjian_learning::SyncedPhrases::new(dir).from_custom(&config.custom_phrases)
            {
                self.phrases = phrases;
            }
        }
    }

    pub(super) fn paste_phrase(&mut self, index: usize) {
        if let Some(phrase) = self.phrases.get(index).filter(|p| p.enabled) {
            system::paste(
                Payload {
                    kind: qingjian_clipboard::ClipKind::Text,
                    text: phrase.text.clone(),
                    formats: Vec::new(),
                },
                self.target,
            );
            self.hide();
        }
    }
}
