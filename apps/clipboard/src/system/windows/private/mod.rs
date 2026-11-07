//! 私密输入只用 Unicode SendInput，绝不写系统剪贴板；锁屏、休眠和退出立即撤销。

mod automation;
mod com;
mod focus;

use qingjian_clipboard_sync::private::PrivateVault;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::time::{Duration, Instant};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, SendInput,
};
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
use zeroize::{Zeroize, Zeroizing};

static VAULT: Mutex<Weak<PrivateVault>> = Mutex::new(Weak::new());
static SENDER: Mutex<Option<qingjian_clipboard_sync::service::PrivateSender>> = Mutex::new(None);
static INPUT_GATE: qingjian_clipboard_sync::private::input::InputGate =
    qingjian_clipboard_sync::private::input::InputGate::new();
static RECEIVE: AtomicBool = AtomicBool::new(false);
static LOCKED: AtomicBool = AtomicBool::new(true);
static SUSPENDED: AtomicBool = AtomicBool::new(false);

pub(crate) fn attach(
    vault: Arc<PrivateVault>,
    sender: qingjian_clipboard_sync::service::PrivateSender,
    enabled: bool,
) {
    automation::cancel();
    let mut current = VAULT.lock().unwrap_or_else(PoisonError::into_inner);
    RECEIVE.store(enabled, Ordering::SeqCst);
    if blocked() {
        vault.suspend();
    }
    *current = Arc::downgrade(&vault);
    *SENDER.lock().unwrap_or_else(PoisonError::into_inner) = Some(sender);
}

pub(crate) fn send_current(only_sensitive: bool) {
    if LOCKED.load(Ordering::SeqCst) || SUSPENDED.load(Ordering::SeqCst) {
        return;
    }
    let Some(secret) = super::clipboard::read_private(only_sensitive) else {
        if !only_sensitive {
            super::listener::notice("private.send_empty");
        }
        return;
    };
    if !SENDER
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .is_some_and(|sender| sender.send(secret))
    {
        super::listener::notice("private.send_failed");
    }
}

fn refresh(locked: Option<bool>, suspended: Option<bool>) {
    let guard = VAULT.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(value) = locked {
        LOCKED.store(value, Ordering::SeqCst);
    }
    if let Some(value) = suspended {
        SUSPENDED.store(value, Ordering::SeqCst);
    }
    if let Some(vault) = guard.upgrade() {
        if blocked() {
            vault.suspend();
        } else {
            vault.resume();
        }
    }
}

pub(crate) fn lock() {
    automation::cancel();
    refresh(Some(true), None);
}

pub(crate) fn unlock() {
    refresh(Some(false), None);
}

pub(crate) fn suspend() {
    automation::cancel();
    refresh(None, Some(true));
}

pub(crate) fn resume() {
    refresh(None, Some(false));
}

pub(crate) fn session_unlocked() -> bool {
    use windows::Win32::System::RemoteDesktop::{
        WTS_CURRENT_SESSION, WTSFreeMemory, WTSINFOEXW, WTSQuerySessionInformationW,
        WTSSessionInfoEx,
    };
    let mut buffer = windows::core::PWSTR::null();
    let mut size = 0;
    unsafe {
        if WTSQuerySessionInformationW(
            None,
            WTS_CURRENT_SESSION,
            WTSSessionInfoEx,
            &mut buffer,
            &mut size,
        )
        .is_err()
        {
            return false;
        }
        let unlocked = size as usize >= size_of::<WTSINFOEXW>() && {
            let info = &*buffer.0.cast::<WTSINFOEXW>();
            info.Level == 1 && info.Data.WTSInfoExLevel1.SessionFlags == 1
        };
        WTSFreeMemory(buffer.0.cast());
        unlocked
    }
}

fn current() -> Option<Arc<PrivateVault>> {
    VAULT
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .upgrade()
}

/// 有私密内容时吞掉快捷键，即使输入失败也不退回普通粘贴。
pub(crate) fn input() -> bool {
    if INPUT_GATE.active() {
        return true;
    }
    let Some(vault) = current() else {
        return false;
    };
    if !vault.pending() {
        return false;
    }
    input_at(None)
}

fn input_at(target: Option<isize>) -> bool {
    let Some(permit) = INPUT_GATE.acquire() else {
        return true;
    };
    let Some(vault) = current().filter(|vault| vault.pending()) else {
        return false;
    };
    let Some(secret) = vault.take() else {
        return false;
    };
    automation::cancel();
    let target = target.unwrap_or_else(|| unsafe { GetForegroundWindow() }.0 as isize);
    let generation = automation::generation();
    std::thread::spawn(move || {
        let _permit = permit;
        let _com = com::Com::new();
        let focus = (unsafe { GetForegroundWindow() }.0 as isize == target)
            .then(|| focus::Focus::capture(false))
            .flatten();
        let until = Instant::now() + Duration::from_secs(2);
        while unsafe { GetForegroundWindow() }.0 as isize != target
            || [0x10, 0x11, 0x12, 0x5B, 0x5C]
                .iter()
                .any(|key| super::key_down(*key))
        {
            if Instant::now() >= until || automation::generation() != generation || blocked() {
                super::listener::notice("private.failed");
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        if blocked()
            || automation::generation() != generation
            || target == 0
            || unsafe { GetForegroundWindow() }.0 as isize != target
            || focus.as_ref().is_some_and(|focus| !focus.unchanged())
        {
            super::listener::notice("private.failed");
            return;
        }
        let committed = type_into(&secret, false);
        tracing::info!(committed, "Private input completed");
        if !committed {
            super::listener::notice("private.failed");
        }
    });
    true
}

fn blocked() -> bool {
    LOCKED.load(Ordering::SeqCst)
        || SUSPENDED.load(Ordering::SeqCst)
        || !RECEIVE.load(Ordering::SeqCst)
}

pub(crate) fn received(automatic: bool) {
    automation::received(automatic);
}

pub(crate) fn input_target(target: isize) {
    if !input_at(Some(target)) {
        super::listener::notice("private.empty");
    }
}

pub(crate) fn input_current() {
    if !input() {
        super::listener::notice("private.empty");
    }
}

fn type_into(secret: &qingjian_clipboard_sync::private::Secret, replace: bool) -> bool {
    let units = Zeroizing::new(secret.as_str().encode_utf16().collect::<Vec<_>>());
    let mut keys = Vec::with_capacity(units.len() * 2 + 4);
    if replace {
        use windows::Win32::UI::Input::KeyboardAndMouse::{KEYBD_EVENT_FLAGS, VK_A, VK_CONTROL};
        for (key, up) in [
            (VK_CONTROL, false),
            (VK_A, false),
            (VK_A, true),
            (VK_CONTROL, true),
        ] {
            keys.push(INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: key,
                        dwFlags: if up {
                            KEYEVENTF_KEYUP
                        } else {
                            KEYBD_EVENT_FLAGS(0)
                        },
                        ..KEYBDINPUT::default()
                    },
                },
            });
        }
    }
    for unit in units.iter() {
        for flags in [KEYEVENTF_UNICODE, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP] {
            keys.push(INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wScan: *unit,
                        dwFlags: flags,
                        ..KEYBDINPUT::default()
                    },
                },
            });
        }
    }
    let sent = unsafe { SendInput(&keys, size_of::<INPUT>() as i32) } as usize;
    // INPUT 是无析构的 Win32 POD；覆盖编码后的键盘事件副本。
    unsafe {
        std::slice::from_raw_parts_mut(
            keys.as_mut_ptr().cast::<u8>(),
            keys.len() * size_of::<INPUT>(),
        )
    }
    .zeroize();

    sent == keys.len()
}
