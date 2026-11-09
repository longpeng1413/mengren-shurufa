//! 一段识别的后台线程：先拿到（必要时加载）识别器，再一边收录音一边每隔 [`PARTIAL_EVERY`] 把当前段整段识别一遍
//! 交回中间结果（SenseVoice 不是流式模型，靠重识别模拟边说边出字），说完识别最后一遍交回最终结果。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use qingjian_core::TranscriptEvent;

use super::command::Command;
use super::segmenter::Segmenter;
use crate::recognizer::Recognizer;
use crate::{OfflineConfig, shared};

pub fn run(
    config: OfflineConfig,
    sample_rate: u32,
    commands: Receiver<Command>,
    events: Sender<TranscriptEvent>,
    cancelled: Arc<AtomicBool>,
) {
    let recognizer = match shared::acquire(&config) {
        Ok(recognizer) => recognizer,
        Err(error) => {
            tracing::warn!(%error, "offline recognizer unavailable");
            if !cancelled.load(Ordering::Relaxed) {
                let _ = events.send(TranscriptEvent::Error(error.describe()));
            }
            return;
        }
    };
    let mut session = Session {
        recognizer,
        audio: Segmenter::new(sample_rate),
        events,
        last_partial: String::new(),
        decoded_samples: 0,
        decoded_at: Instant::now(),
    };
    session.run(&commands, &cancelled);
    drop(session);
    shared::touch();
}

struct Session {
    recognizer: Arc<Recognizer>,

    audio: Segmenter,

    events: Sender<TranscriptEvent>,

    /// 上次交回的中间结果，没变就不重复交。
    last_partial: String,

    /// 上次识别时当前段有多少采样；没新录音就不重识别。
    decoded_samples: usize,

    decoded_at: Instant,
}

impl Session {
    fn run(&mut self, commands: &Receiver<Command>, cancelled: &AtomicBool) {
        loop {
            let mut finished = false;
            match commands.recv_timeout(PARTIAL_EVERY) {
                Ok(command) => finished |= self.apply(command),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            // 加载模型或识别期间攒下的录音一次收完
            while let Ok(command) = commands.try_recv() {
                finished |= self.apply(command);
            }
            if cancelled.load(Ordering::Relaxed) {
                return;
            }
            while let Some(head) = self.audio.split_off() {
                match self.decode(&head) {
                    Some(text) => self.audio.append_done(&text),
                    None => return,
                }
                self.decoded_samples = 0;
            }
            if finished {
                let current = self.audio.current().to_vec();
                if let Some(tail) = self.decode(&current) {
                    let text = self.audio.text_with(&tail);
                    if !cancelled.load(Ordering::Relaxed) {
                        let _ = self.events.send(TranscriptEvent::Final(text));
                    }
                }
                return;
            }
            self.partial(cancelled);
        }
    }

    /// 收一条指令，回是不是说完了。
    fn apply(&mut self, command: Command) -> bool {
        match command {
            Command::Audio(samples) => {
                self.audio.push(&samples);
                false
            }
            Command::Finish => true,
        }
    }

    fn partial(&mut self, cancelled: &AtomicBool) {
        let samples = self.audio.current().len();
        if samples == self.decoded_samples
            || self.audio.seconds() < MIN_PARTIAL_SECONDS
            || self.decoded_at.elapsed() < PARTIAL_EVERY
        {
            return;
        }
        let current = self.audio.current().to_vec();
        let Ok(tail) = self.recognizer.decode(self.audio.sample_rate(), &current) else {
            // 中间结果出错不打断，说完时最后一遍再报
            return;
        };
        self.decoded_samples = samples;
        self.decoded_at = Instant::now();
        let text = self.audio.text_with(&tail);
        if text != self.last_partial && !cancelled.load(Ordering::Relaxed) {
            self.last_partial.clone_from(&text);
            let _ = self.events.send(TranscriptEvent::Partial(text));
        }
    }

    /// 识别一块；出错时交回出错并回 `None`（这段到此为止）。
    fn decode(&self, samples: &[f32]) -> Option<String> {
        match self.recognizer.decode(self.audio.sample_rate(), samples) {
            Ok(text) => Some(text),
            Err(error) => {
                tracing::warn!(%error, "offline decode failed");
                let _ = self.events.send(TranscriptEvent::Error(error.describe()));
                None
            }
        }
    }
}

/// 说话途中每隔这么久重识别一次。
const PARTIAL_EVERY: Duration = Duration::from_millis(900);

/// 当前段不到这么长不出中间结果（太短的识别不准，白耗电）。
const MIN_PARTIAL_SECONDS: f32 = 0.6;
