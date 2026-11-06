//! 密码框自动填入，以及绑定具体控件的单次等待；不对任意前台窗口盲目输入。

use super::super::{key_down, listener};
use super::{com::Com, focus::Focus};
use qingjian_clipboard_sync::private::PrivateVault;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

static WAITING: AtomicBool = AtomicBool::new(false);
static GENERATION: AtomicU64 = AtomicU64::new(0);
static AUTO_ACTIVE: AtomicBool = AtomicBool::new(false);

pub(super) fn cancel() {
    GENERATION.fetch_add(1, Ordering::SeqCst);
    WAITING.store(false, Ordering::SeqCst);
}

fn end_wait(generation: u64) {
    if GENERATION.load(Ordering::SeqCst) == generation {
        WAITING.store(false, Ordering::SeqCst);
    }
}

pub(super) fn received(automatic: bool) {
    if WAITING.load(Ordering::SeqCst) {
        return;
    }
    if !automatic {
        listener::notice("private.received");
        return;
    }
    let Some(vault) = super::current() else {
        return;
    };
    if AUTO_ACTIVE.swap(true, Ordering::SeqCst) {
        listener::notice("private.received");
        return;
    }
    let generation = GENERATION.load(Ordering::SeqCst);
    std::thread::spawn(move || {
        let handled = (|| {
            let _com = Com::new()?;
            let focus = Focus::capture(true)?;
            if GENERATION.load(Ordering::SeqCst) != generation {
                return Some(true);
            }
            Some(fill(&focus, &vault))
        })()
        .unwrap_or(false);
        AUTO_ACTIVE.store(false, Ordering::SeqCst);
        if !handled {
            listener::notice("private.received");
        }
    });
}

pub(super) fn prepare(target: Option<isize>) {
    if WAITING.load(Ordering::SeqCst) {
        cancel();
        listener::notice("private.wait_cancelled");
        return;
    }
    let Some(vault) = super::current().filter(|vault| vault.accepting()) else {
        listener::notice("private.wait_unavailable");
        return;
    };
    let target = target.unwrap_or_else(|| unsafe { GetForegroundWindow() }.0 as isize);
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    WAITING.store(true, Ordering::SeqCst);
    std::thread::spawn(move || {
        let Some(_com) = Com::new() else {
            end_wait(generation);
            listener::notice("private.wait_unavailable");
            return;
        };
        let focus_deadline = Instant::now() + Duration::from_secs(2);
        while unsafe { GetForegroundWindow() }.0 as isize != target {
            if Instant::now() >= focus_deadline || GENERATION.load(Ordering::SeqCst) != generation {
                end_wait(generation);
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let Some(focus) = Focus::capture(false) else {
            end_wait(generation);
            listener::notice("private.wait_unavailable");
            return;
        };
        listener::notice("private.wait_started");
        let expires = Instant::now() + Duration::from_secs(120);
        loop {
            if GENERATION.load(Ordering::SeqCst) != generation {
                return;
            }
            if Instant::now() >= expires || super::blocked() || !focus.unchanged() {
                end_wait(generation);
                listener::notice("private.wait_cancelled");
                return;
            }
            if vault.pending() {
                end_wait(generation);
                let _ = fill(&focus, &vault);
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    });
}

fn fill(focus: &Focus, vault: &PrivateVault) -> bool {
    let deadline = Instant::now() + Duration::from_secs(2);
    while [0x10, 0x11, 0x12, 0x5B, 0x5C]
        .iter()
        .any(|key| key_down(*key))
    {
        if Instant::now() >= deadline || super::blocked() || !focus.unchanged() {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if super::blocked() || !focus.unchanged() {
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
