//! 短语适配器和已确认的远端摘要，受 Shared 的互斥锁保护。

use super::PhraseSource;
use std::{collections::HashMap, path::PathBuf};

pub(crate) struct PhraseState {
    pub directory: PathBuf,
    pub source: Box<dyn PhraseSource>,
    pub received: HashMap<String, Vec<u8>>,
}
