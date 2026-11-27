//! 「开机默认使用猛人输入法」（`[general] default_input_method`）：把系统的默认输入法设成猛人，
//! 等同 Windows「高级键盘设置 → 替代默认输入法」；关掉时恢复打开前系统原来的默认。
//!
//! 系统把这项记在 `HKCU\Control Panel\International\User Profile` 的 `InputMethodOverride`；
//! 设置走 `input.dll` 公开的 `SetDefaultLayoutOrTip`（与系统设置页同一条路），清除时删这个值。
//! 打开前的原值存在 `%APPDATA%\Qingjian\default-ime-previous`，恢复后删掉。Server 启动与配置改动后各调一次，
//! 状态已经对了就什么都不做。

use std::path::PathBuf;

use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW,
};
use windows::core::{HSTRING, PCWSTR, s, w};

/// 猛人的输入法编号：简体中文 + 文本服务 CLSID + 语言 profile GUID（见 TSF DLL 的 `com/mod.rs`）。
const MENGREN_TIP: &str =
    "0804:{4FDCA82D-E923-49BF-9E75-BB906B93B8BB}{8119F8E0-CF81-423B-9189-C0D7374324B3}";

const PROFILE_KEY: PCWSTR = w!("Control Panel\\International\\User Profile");
const OVERRIDE_VALUE: PCWSTR = w!("InputMethodOverride");

/// `SetDefaultLayoutOrTip(psz, dwFlags)`。
type SetDefault = unsafe extern "system" fn(PCWSTR, u32) -> i32;

/// 按开关把系统默认输入法设成猛人或恢复原样。
pub fn apply(on: bool) {
    let current = current_override().unwrap_or_default();
    let ours = current.eq_ignore_ascii_case(MENGREN_TIP);
    match (on, ours) {
        (true, false) => {
            if let Some(path) = previous_file()
                && let Err(error) = std::fs::write(&path, &current)
            {
                tracing::warn!(%error, "previous default input method could not be saved");
            }
            if set_default(MENGREN_TIP) {
                tracing::info!("default input method set to Mengren");
            }
        }
        (false, true) => {
            let previous = previous_file()
                .and_then(|path| std::fs::read_to_string(path).ok())
                .map(|text| text.trim().to_owned())
                .unwrap_or_default();
            let restored = if previous.is_empty() {
                clear_override()
            } else {
                set_default(&previous)
            };
            if restored {
                tracing::info!(
                    cleared = previous.is_empty(),
                    "default input method restored"
                );
                if let Some(path) = previous_file() {
                    let _ = std::fs::remove_file(path);
                }
            }
        }
        _ => {}
    }
}

/// 系统现在的「替代默认输入法」；没设回 `None`。
fn current_override() -> Option<String> {
    let mut buffer = [0u16; 256];
    let mut size = (buffer.len() * 2) as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PROFILE_KEY,
            OVERRIDE_VALUE,
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if status != ERROR_SUCCESS {
        return None;
    }
    let len = (size as usize / 2).saturating_sub(1).min(buffer.len());
    Some(String::from_utf16_lossy(&buffer[..len]).trim().to_owned())
}

fn set_default(tip: &str) -> bool {
    let ok = unsafe {
        let Ok(module) = LoadLibraryW(w!("input.dll")) else {
            tracing::warn!("input.dll unavailable");
            return false;
        };
        let Some(proc) = GetProcAddress(module, s!("SetDefaultLayoutOrTip")) else {
            tracing::warn!("SetDefaultLayoutOrTip unavailable");
            return false;
        };
        let set: SetDefault = std::mem::transmute(proc);
        set(PCWSTR(HSTRING::from(tip).as_ptr()), 0) != 0
    };
    if !ok {
        tracing::warn!("SetDefaultLayoutOrTip failed");
    }
    ok
}

fn clear_override() -> bool {
    let status = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, PROFILE_KEY, OVERRIDE_VALUE) };
    status == ERROR_SUCCESS || status.0 == 2
}

fn previous_file() -> Option<PathBuf> {
    qingjian_platform::dirs::user_dir().map(|dir| dir.join("default-ime-previous"))
}
