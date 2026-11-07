//! [`InputGate`]：只有持有许可的任务可以消费私密槽。
use super::InputPermit;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Default)]
pub struct InputGate {
    pub(super) active: AtomicBool,
}

impl InputGate {
    pub const fn new() -> Self {
        Self {
            active: AtomicBool::new(false),
        }
    }

    pub fn acquire(&self) -> Option<InputPermit<'_>> {
        self.active
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .ok()
            .map(|_| InputPermit(self))
    }

    pub fn active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }
}
