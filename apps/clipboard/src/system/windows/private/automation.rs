//! 用户显式开启的密码框自动填入；普通输入只消费已经收到的内容。

use super::super::{key_down, listener};
use super::{com::Com, focus::Focus};
use qingjian_clipboard_sync::private::PrivateVault;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

static GENERATION: AtomicU64 = AtomicU64::new(0);

pub(super) fn cancel() {
    GENERATION.fetch_add(1, Ordering::SeqCst);
}

pub(super) fn generation() -> u64 {
    GENERATION.load(Ordering::SeqCst)
}

pub(super) fn received(automatic: bool) {
    if !automatic {
        listener::notice("private.received");
        return;
    }
    let Some(vault) = super::current() else {
        return;
    };
    let Some(permit) = super::INPUT_GATE.acquire() else {
        return;
    };
    let generation = GENERATION.load(Ordering::SeqCst);
    std::thread::spawn(move || {
        let _permit = permit;
        let handled = (|| {
            let _com = Com::new()?;
            let focus = Focus::capture(true)?;
            if GENERATION.load(Ordering::SeqCst) != generation {
                return Some(true);
            }
            Some(fill(&focus, &vault, generation))
        })()
        .unwrap_or(false);
        if !handled {
            listener::notice("private.received");
        }
    });
}

fn fill(focus: &Focus, vault: &PrivateVault, generation: u64) -> bool {
    let deadline = Instant::now() + Duration::from_secs(2);
    while [0x10, 0x11, 0x12, 0x5B, 0x5C]
        .iter()
        .any(|key| key_down(*key))
    {
        if Instant::now() >= deadline
            || super::blocked()
            || !focus.unchanged()
            || self::generation() != generation
        {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if super::blocked() || !focus.unchanged() || self::generation() != generation {
        return false;
    }
    let Some(secret) = vault.take() else {
        return true;
    };
    if !super::type_into(&secret, focus.password()) {
        listener::notice("private.failed");
    }
    true
}
