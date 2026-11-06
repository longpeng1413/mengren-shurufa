//! 有界的单槽保险箱，取用即清空，不实现序列化。

use super::{Secret, Slot};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

pub struct PrivateVault {
    slot: Mutex<Option<Slot>>,

    accepting: std::sync::atomic::AtomicBool,
}

impl Default for PrivateVault {
    fn default() -> Self {
        Self {
            slot: Mutex::new(None),
            accepting: std::sync::atomic::AtomicBool::new(true),
        }
    }
}

impl PrivateVault {
    pub fn accepting(&self) -> bool {
        self.accepting.load(std::sync::atomic::Ordering::SeqCst)
    }
    pub(crate) fn store(&self, secret: Secret, lifetime: Duration) -> bool {
        let mut slot = self.slot.lock().unwrap_or_else(PoisonError::into_inner);
        if !self.accepting.load(std::sync::atomic::Ordering::SeqCst) {
            return false;
        }
        *slot = Some(Slot {
            secret,
            expires: Instant::now() + lifetime.min(super::LIFETIME),
        });
        true
    }

    pub fn clear(&self) {
        self.slot
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
    }

    pub fn suspend(&self) {
        self.accepting
            .store(false, std::sync::atomic::Ordering::SeqCst);
        self.clear();
    }

    pub fn resume(&self) {
        self.accepting
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn expire(&self) {
        let mut slot = self.slot.lock().unwrap_or_else(PoisonError::into_inner);
        if slot
            .as_ref()
            .is_some_and(|value| value.expires <= Instant::now())
        {
            slot.take();
        }
    }

    pub fn pending(&self) -> bool {
        self.expire();
        self.slot
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some()
    }

    pub fn take(&self) -> Option<Secret> {
        self.slot
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
            .filter(|value| value.expires > Instant::now())
            .map(|value| value.secret)
    }
}

impl std::fmt::Debug for PrivateVault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PrivateVault([REDACTED])")
    }
}
