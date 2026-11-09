//! 离线识别的 [`Transcriber`] 实现：每段识别开一个后台线程（[`session`]），这边只往通道里放录音、取事件。

mod command;
mod running;
mod segmenter;
mod session;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, TryRecvError};
use std::thread;

use qingjian_core::{Transcriber, TranscriptEvent};

use crate::{OfflineConfig, OfflineError, library};
use command::Command;
use running::Running;

pub struct OfflineTranscriber {
    config: OfflineConfig,

    current: Option<Running>,
}

impl OfflineTranscriber {
    pub fn new(config: OfflineConfig) -> Self {
        Self {
            config,
            current: None,
        }
    }
}

impl Transcriber for OfflineTranscriber {
    /// 模型下好了、引擎库载得进来。
    fn available(&self) -> bool {
        self.config.model.installed() && library::api(self.config.library_dir.as_deref()).is_ok()
    }

    fn start(&mut self, sample_rate: u32) {
        self.cancel();
        let (commands, command_receiver) = mpsc::channel();
        let (event_sender, events) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let config = self.config.clone();
        let flag = Arc::clone(&cancelled);
        let spawned = thread::Builder::new()
            .name("mengren-voice-offline".to_owned())
            .spawn(move || {
                session::run(config, sample_rate, command_receiver, event_sender, flag);
            });
        match spawned {
            Ok(_) => {
                self.current = Some(Running {
                    commands,
                    events,
                    cancelled,
                });
            }
            Err(error) => tracing::error!(%error, "cannot spawn offline voice thread"),
        }
    }

    fn feed(&mut self, samples: &[i16]) {
        if let Some(running) = &self.current {
            let _ = running.commands.send(Command::Audio(samples.to_vec()));
        }
    }

    fn finish(&mut self) {
        if let Some(running) = &self.current {
            let _ = running.commands.send(Command::Finish);
        }
    }

    fn cancel(&mut self) {
        if let Some(running) = self.current.take() {
            running.cancelled.store(true, Ordering::Relaxed);
        }
    }

    fn poll(&mut self) -> Option<TranscriptEvent> {
        let running = self.current.as_ref()?;
        match running.events.try_recv() {
            Ok(event) => {
                if !matches!(event, TranscriptEvent::Partial(_)) {
                    self.current = None;
                }
                Some(event)
            }
            Err(TryRecvError::Empty) => None,
            // 线程没交结果就走了（不该发生）：当出错收尾，壳好关面板
            Err(TryRecvError::Disconnected) => {
                self.current = None;
                Some(TranscriptEvent::Error(
                    OfflineError::Decode("worker ended".to_owned()).describe(),
                ))
            }
        }
    }
}

impl Drop for OfflineTranscriber {
    fn drop(&mut self) {
        self.cancel();
    }
}
