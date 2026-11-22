//! 词典卡片表的后台加载：开着划词翻译时进程一起就在后台读，第一次查词时多半已读好；没读好就等它读完。

use std::thread::JoinHandle;

use qingjian_platform::resources::bundled_resource;
use qingjian_translate::WordCards;

/// 随包的英文词典卡片表（`assets/wordcard/README.md`）。
const TABLE: &str = "assets/wordcard/wordcard-en.tsv";

#[derive(Default)]
pub(super) struct Words {
    loading: Option<JoinHandle<Option<WordCards>>>,

    table: Option<WordCards>,

    /// 读过但没读成（文件缺了或坏了），不再反复读。
    failed: bool,
}

impl Words {
    /// 还没读就在后台开始读。
    pub(super) fn preload(&mut self) {
        if self.table.is_some() || self.loading.is_some() || self.failed {
            return;
        }
        self.loading = std::thread::Builder::new()
            .name("translator-wordcards".to_owned())
            .spawn(|| {
                let path = bundled_resource(TABLE)?;
                WordCards::from_path(&path)
                    .inspect_err(|error| tracing::warn!(%error, "word card table unreadable"))
                    .ok()
            })
            .ok();
    }

    /// 查词表；后台还在读就等它读完。
    pub(super) fn get(&mut self) -> Option<&WordCards> {
        self.preload();
        if let Some(loading) = self.loading.take() {
            match loading.join() {
                Ok(Some(table)) => {
                    tracing::info!(entries = table.len(), "word card table loaded");
                    self.table = Some(table);
                }
                _ => self.failed = true,
            }
        }
        self.table.as_ref()
    }
}
