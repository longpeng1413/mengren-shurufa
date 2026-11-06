//! 公网同步文字和图片，单条内容限制为 100 MB；过滤清单不修改本地记录。

use crate::record::Stamp;
use crate::{Record, RecordKind, Replica, SyncError};

pub(crate) struct BoundedReplica<'a>(pub &'a mut dyn Replica);

impl Replica for BoundedReplica<'_> {
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
        Ok(self.0.record(id)?.filter(allowed))
    }
    fn apply(&mut self, record: Record) -> Result<(), SyncError> {
        if !allowed(&record) {
            return Err(super::invalid());
        }
        self.0.apply(record)
    }
}

fn allowed(record: &Record) -> bool {
    match record.kind {
        RecordKind::Text => record.image.is_empty() && record.text.len() < super::MAX_CONTENT_BYTES,
        RecordKind::Image => {
            record.text.is_empty() && record.image.len() < super::MAX_CONTENT_BYTES
        }
    }
}

#[cfg(test)]
mod tests {
    use super::allowed;
    use crate::Record;

    #[test]
    fn image_boundary_is_decimal_one_hundred_megabytes() {
        let mut record = Record::image(
            vec![7; crate::relay::MAX_CONTENT_BYTES - 1],
            "png".into(),
            1,
        );
        assert!(allowed(&record));
        record.image.push(7);
        assert!(!allowed(&record));
    }
}
