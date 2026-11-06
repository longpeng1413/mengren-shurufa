//! 公网只同步不超过 1 MiB 的文字；图片与大文件仍走快传。过滤清单不修改本地记录。

use crate::record::Stamp;
use crate::{Record, RecordKind, Replica, SyncError};

pub(crate) struct TextReplica<'a>(pub &'a mut dyn Replica);

impl Replica for TextReplica<'_> {
    fn stamps(&self) -> Result<Vec<Stamp>, SyncError> {
        self.0
            .stamps()?
            .into_iter()
            .filter_map(|stamp| match self.record(&stamp.id) {
                Ok(Some(_)) => Some(Ok(stamp)),
                Ok(None) => None,
                Err(error) => Some(Err(error)),
            })
            .collect()
    }
    fn record(&self, id: &str) -> Result<Option<Record>, SyncError> {
        Ok(self.0.record(id)?.filter(|record| {
            record.kind == RecordKind::Text
                && record.text.len() <= super::MAX_TEXT
                && record.image.is_empty()
        }))
    }
    fn apply(&mut self, record: Record) -> Result<(), SyncError> {
        if record.kind != RecordKind::Text
            || record.text.len() > super::MAX_TEXT
            || !record.image.is_empty()
        {
            return Err(super::invalid());
        }
        self.0.apply(record)
    }
}
