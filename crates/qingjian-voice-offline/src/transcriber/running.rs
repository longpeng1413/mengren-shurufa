//! 正在进行的一段识别：与它的后台线程之间的通道。

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{Receiver, Sender};

use qingjian_core::TranscriptEvent;

use super::command::Command;

pub struct Running {
    pub commands: Sender<Command>,

    pub events: Receiver<TranscriptEvent>,

    /// 置位后线程尽快退出、不再交回任何事。
    pub cancelled: Arc<AtomicBool>,
}
