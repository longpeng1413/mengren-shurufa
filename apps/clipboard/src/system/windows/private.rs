//! 私密输入只用 Unicode SendInput，绝不写系统剪贴板；锁屏、休眠和退出立即撤销。

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
static LOCKED: AtomicBool = AtomicBool::new(true);
static SUSPENDED: AtomicBool = AtomicBool::new(false);

pub(crate) fn attach(vault: Arc<PrivateVault>) {
    if LOCKED.load(Ordering::SeqCst) || SUSPENDED.load(Ordering::SeqCst) {
        vault.suspend();
    }
    *VAULT.lock().unwrap_or_else(PoisonError::into_inner) = Arc::downgrade(&vault);
}

pub(crate) fn lock() {
    LOCKED.store(true, Ordering::SeqCst);
    if let Some(vault) = current() {
        vault.suspend();
    }
}

pub(crate) fn unlock() {
    LOCKED.store(false, Ordering::SeqCst);
    if !SUSPENDED.load(Ordering::SeqCst)
        && let Some(vault) = current()
    {
        vault.resume();
    }
}

pub(crate) fn suspend() {
    SUSPENDED.store(true, Ordering::SeqCst);
    if let Some(vault) = current() {
        vault.suspend();
    }
}

pub(crate) fn resume() {
    SUSPENDED.store(false, Ordering::SeqCst);
    if !LOCKED.load(Ordering::SeqCst)
        && let Some(vault) = current()
    {
        vault.resume();
    }
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
    let Some(vault) = current() else {
        return false;
    };
    if !vault.pending() {
        return false;
    }
    let target = unsafe { GetForegroundWindow() }.0 as isize;
    std::thread::spawn(move || {
        let until = Instant::now() + Duration::from_secs(2);
        while [0x10, 0x11, 0x12, 0x5B, 0x5C]
            .iter()
            .any(|key| super::key_down(*key))
        {
            if Instant::now() >= until {
                vault.clear();
                super::listener::notice("private.failed");
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        if LOCKED.load(Ordering::SeqCst)
            || SUSPENDED.load(Ordering::SeqCst)
            || target == 0
            || unsafe { GetForegroundWindow() }.0 as isize != target
        {
            vault.clear();
            super::listener::notice("private.failed");
            return;
        }
        let Some(secret) = vault.take() else {
            super::listener::notice("private.empty");
            return;
        };
        let units = Zeroizing::new(secret.as_str().encode_utf16().collect::<Vec<_>>());
        let mut keys = Vec::with_capacity(units.len() * 2);
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
        if sent != keys.len() {
            super::listener::notice("private.failed");
        }
    });
    true
}
