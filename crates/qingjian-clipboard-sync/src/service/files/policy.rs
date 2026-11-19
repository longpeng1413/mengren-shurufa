//! 用户明确选择的文件设备；运行中的传输用修订号检测撤销。
use crate::SyncError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[derive(Default)]
pub(in crate::service) struct Policy {
    pub directory: PathBuf,
    pub enabled: bool,
    pub peers: BTreeSet<String>,
    pub revision: u64,
    #[cfg(test)]
    pub test_loopback: bool,
}

#[derive(Default, Deserialize, Serialize)]
struct Preferences {
    enabled: bool,
    peers: BTreeSet<String>,
}

impl Policy {
    pub fn load(directory: PathBuf) -> Self {
        let preferences: Preferences = std::fs::read(directory.join("lan-files.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        super::cache::clean(&directory.join("received-files"), true);
        Self {
            directory,
            enabled: cfg!(target_os = "android") && preferences.enabled,
            peers: preferences.peers,
            ..Self::default()
        }
    }

    pub fn update(&mut self, enabled: bool, peers: BTreeSet<String>) -> Result<(), SyncError> {
        if self.enabled == enabled && self.peers == peers {
            return Ok(());
        }
        save(
            &self.directory,
            &Preferences {
                enabled,
                peers: peers.clone(),
            },
        )?;
        self.enabled = enabled;
        self.peers = peers;
        self.revision += 1;
        Ok(())
    }

    pub fn stop(&mut self) {
        self.enabled = false;
        self.revision += 1;
    }
}

fn save(directory: &Path, preferences: &Preferences) -> Result<(), SyncError> {
    let file = tempfile::NamedTempFile::new_in(directory)?;
    serde_json::to_writer(file.as_file(), preferences)?;
    file.as_file().sync_all()?;
    file.persist(directory.join("lan-files.json"))
        .map_err(|e| e.error)?;
    Ok(())
}
