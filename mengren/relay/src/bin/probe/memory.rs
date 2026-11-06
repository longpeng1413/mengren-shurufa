//! 公网验收用合成副本，不读取设备的真实剪贴板。

use qingjian_clipboard_sync::{Record, Replica, Stamp, SyncError};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
pub(super) struct Memory(pub Arc<Mutex<BTreeMap<String, Record>>>);

impl Replica for Memory {
    fn stamps(&self) -> Result<Vec<Stamp>, SyncError> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .values()
            .map(|r| r.stamp.clone())
            .collect())
    }
    fn record(&self, id: &str) -> Result<Option<Record>, SyncError> {
        Ok(self.0.lock().unwrap().get(id).cloned())
    }
    fn apply(&mut self, r: Record) -> Result<(), SyncError> {
        self.0.lock().unwrap().insert(r.stamp.id.clone(), r);
        Ok(())
    }
}
