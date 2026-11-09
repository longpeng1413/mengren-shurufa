//! 离线识别模型（SenseVoice Small int8，k2-fsa/sherpa-onnx 转换的 2024-07-17 官方版）在本机的那份：
//! 装没装好、占多大、删除，以及启动时清掉过期的半截下载与别的版本。下载在 [`download`]。
//! 模型与猛人快传同一份，放在快传 GitHub 的预发布页；换模型要换文件夹名与下面的哈希。

mod download;
mod file;

pub use download::DownloadProgress;
pub use file::ModelFile;

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// 本机的模型目录：`root/<FOLDER>/` 下放模型文件与校验通过的标记。
#[derive(Debug, Clone)]
pub struct SpeechModel {
    root: PathBuf,

    base_url: String,

    files: Vec<ModelFile>,
}

impl SpeechModel {
    /// 这一版模型的文件夹名；`root` 下别的文件夹是旧版本，启动时删掉。
    pub const FOLDER: &'static str = "sensevoice-int8-2024-07-17";

    /// 半截下载超过这么久不再续传，直接删掉。
    pub const PARTIAL_RETENTION: Duration = Duration::from_secs(7 * 24 * 3600);

    const BASE_URL: &'static str = "https://github.com/longpeng1413/mengren-kuaichuan/releases/download/asr-model-sensevoice-2024-07-17/";

    const VERIFIED: &'static str = ".verified";

    const FILES: [ModelFile; 2] = [
        ModelFile {
            name: "model.int8.onnx",
            size: 239_233_841,
            sha256: "c71f0ce00bec95b07744e116345e33d8cbbe08cef896382cf907bf4b51a2cd51",
        },
        ModelFile {
            name: "tokens.txt",
            size: 315_894,
            sha256: "f449eb28dc567533d7fa59be34e2abca8784f771850c78a47fb731a31429a1dc",
        },
    ];

    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            base_url: Self::BASE_URL.to_owned(),
            files: Self::FILES.to_vec(),
        }
    }

    /// 测试用：换下载地址与文件表。
    pub fn with_source(root: impl Into<PathBuf>, base_url: &str, files: Vec<ModelFile>) -> Self {
        Self {
            root: root.into(),
            base_url: base_url.to_owned(),
            files,
        }
    }

    /// 进程里是不是正有一份在下载。
    pub fn downloading() -> bool {
        download::running()
    }

    /// 要下载的总字节数。
    pub fn total_bytes(&self) -> u64 {
        self.files.iter().map(|file| file.size).sum()
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn dir(&self) -> PathBuf {
        self.root.join(Self::FOLDER)
    }

    pub fn model_path(&self) -> PathBuf {
        self.dir().join(self.files[0].name)
    }

    pub fn tokens_path(&self) -> PathBuf {
        self.dir().join(self.files[1].name)
    }

    /// 下载完并校验过（有标记、大小都对）。不重算哈希：标记只在核对通过后写。
    pub fn installed(&self) -> bool {
        let dir = self.dir();
        dir.join(Self::VERIFIED).is_file()
            && self.files.iter().all(|file| {
                std::fs::metadata(dir.join(file.name)).is_ok_and(|meta| meta.len() == file.size)
            })
    }

    /// 占用的磁盘空间，含半截下载与旧版本。
    pub fn size(&self) -> u64 {
        dir_size(&self.root)
    }

    /// 删掉整个模型目录。调用前要先 [`crate::release_shared`]，否则 Windows 上文件还被占着。
    pub fn delete(&self) -> std::io::Result<()> {
        match std::fs::remove_dir_all(&self.root) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        }
    }

    /// 启动时：删掉别的版本的文件夹、超过 [`Self::PARTIAL_RETENTION`] 的半截下载；正在下载时不动。
    pub fn cleanup(&self, now: SystemTime) {
        if download::running() {
            return;
        }
        let Ok(entries) = std::fs::read_dir(&self.root) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if entry.file_name() == Self::FOLDER {
                continue;
            }
            let removed = if path.is_dir() {
                std::fs::remove_dir_all(&path)
            } else {
                std::fs::remove_file(&path)
            };
            if let Err(error) = removed {
                tracing::warn!(%error, path = %path.display(), "cannot remove stale speech model entry");
            }
        }
        let Ok(entries) = std::fs::read_dir(self.dir()) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let partial = path.extension().is_some_and(|ext| ext == "part");
            let stale = entry
                .metadata()
                .and_then(|meta| meta.modified())
                .is_ok_and(|modified| {
                    now.duration_since(modified)
                        .is_ok_and(|age| age > Self::PARTIAL_RETENTION)
                });
            if partial && stale {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
}

fn dir_size(path: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| match entry.metadata() {
            Ok(meta) if meta.is_dir() => dir_size(&entry.path()),
            Ok(meta) => meta.len(),
            Err(_) => 0,
        })
        .sum()
}

#[cfg(test)]
mod tests;
