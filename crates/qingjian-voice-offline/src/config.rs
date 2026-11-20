//! 离线识别要用的：模型在哪、引擎库在哪、用几个线程。

use std::path::PathBuf;

use crate::{OfflineError, SpeechModel, library};

#[derive(Debug, Clone)]
pub struct OfflineConfig {
    pub model: SpeechModel,

    /// 引擎库所在目录；`None` 时按库名交给系统找（安卓的库在 APK 里，按名字找即可）。
    pub library_dir: Option<PathBuf>,

    /// 解码线程数。
    pub threads: i32,
}

impl OfflineConfig {
    /// 线程数取 CPU 核数，最多 4 个（再多手机上反而因调度到小核变慢）。
    pub fn new(model: SpeechModel, library_dir: Option<PathBuf>) -> Self {
        let cores = std::thread::available_parallelism().map_or(1, usize::from);
        Self {
            model,
            library_dir,
            threads: cores.clamp(1, 4) as i32,
        }
    }

    /// 不能识别的原因：模型没装好或引擎库载不进来；能用时回 `None`。壳据此给用户具体提示。
    pub fn problem(&self) -> Option<OfflineError> {
        if !self.model.installed() {
            return Some(OfflineError::ModelMissing);
        }
        library::api(self.library_dir.as_deref()).err()
    }
}
