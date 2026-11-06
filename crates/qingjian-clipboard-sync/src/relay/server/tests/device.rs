//! 带真实同步服务的测试设备，只使用临时目录与合成记录。

use super::memory::Memory;
use crate::{Record, SyncEvent, SyncOptions, SyncService};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};

pub(super) struct Device {
    pub service: Option<SyncService>,

    pub memory: Memory,

    pub events: Receiver<SyncEvent>,

    directory: PathBuf,
}

impl Device {
    pub fn new(url: &str, token: &str) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "mengren-relay-test-{}",
            crate::Identity::generate().unwrap().device_id()
        ));
        let memory = Memory::default();
        let (sender, events) = mpsc::channel();
        let service = SyncService::start(
            SyncOptions {
                directory: directory.clone(),
                name: "test-device".into(),
                model: String::new(),
                own_discovery: false,
                relay: crate::relay::RelayConfig {
                    enabled: true,
                    url: url.into(),
                    token: token.into(),
                },
            },
            Box::new(memory.clone()),
            sender,
        )
        .unwrap();
        Self {
            service: Some(service),
            memory,
            events,
            directory,
        }
    }
    pub fn service(&self) -> &SyncService {
        self.service.as_ref().unwrap()
    }
    pub fn add(&self, record: Record) {
        self.memory
            .0
            .lock()
            .unwrap()
            .insert(record.stamp.id.clone(), record);
        self.service().notify_changed();
    }
    pub fn wait(&self, test: impl Fn(&SyncEvent) -> bool) -> SyncEvent {
        let end = std::time::Instant::now() + std::time::Duration::from_secs(15);
        loop {
            let event = self
                .events
                .recv_timeout(end.saturating_duration_since(std::time::Instant::now()))
                .expect("sync event in time");
            if test(&event) {
                return event;
            }
        }
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        self.service.take();
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}
