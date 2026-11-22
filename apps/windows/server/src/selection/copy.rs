//! 模拟 Ctrl+Insert 从剪贴板取选中文字，取完把原来的剪贴板内容放回去。
//!
//! 用 Ctrl+Insert 不用 Ctrl+C：终端和 VS Code 内置终端里 Ctrl+C 是中断正在跑的程序，Ctrl+Insert 在各处都只是复制。
//! 放回的原内容**不加任何标记**：猛人剪贴板把 `ExcludeClipboardContentFromMonitorProcessing`、
//! `CanIncludeInClipboardHistory = 0`、`CanUploadToCloudClipboard = 0` 都当成密码管理器复制的密码，
//! 会私密发给已配对设备（2026-11-20 两次实测踩到，第二次发出去的是用户真实的剪贴板内容）。
//! 不加标记的代价只是猛人剪贴板与 Win+V 把同一条原内容挪回历史最前。选中的那段文字由应用自己复制，
//! 会进历史（用户已同意）。剪贴板里内容太大（超过 [`MAX_SAVED_BYTES`]）时不做，免得放不回去。

use std::thread;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData,
    GetClipboardSequenceNumber, GetOpenClipboardWindow, OpenClipboard, RegisterClipboardFormatW,
    SetClipboardData,
};
use windows::Win32::System::Memory::{
    GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, MAPVK_VK_TO_VSC, MapVirtualKeyW, SendInput,
    VIRTUAL_KEY, VK_C, VK_CONTROL, VK_INSERT, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, HWND_MESSAGE, WINDOW_EX_STYLE, WINDOW_STYLE,
};
use windows::core::w;

/// Unicode 文本格式（`CF_UNICODETEXT`）。
const CF_UNICODETEXT: u32 = 13;

/// 剪贴板里保存下来以便放回的上限。
const MAX_SAVED_BYTES: usize = 64 * 1024 * 1024;

/// 等用户松开快捷键里的修饰键（Ctrl+Alt+F 按下时 Ctrl、Alt 还按着，直接发会变成别的组合）。
const MODIFIER_WAIT: Duration = Duration::from_millis(1500);

/// 等应用把复制内容放上剪贴板。
const COPY_WAIT: Duration = Duration::from_millis(600);

/// 复制前保存原内容时等剪贴板的上限：等不到就不模拟复制（剪贴板还没动，什么都不会丢）。
const CAPTURE_PATIENCE: Duration = Duration::from_millis(300);

/// 读复制到的文字时等剪贴板的上限。
const READ_PATIENCE: Duration = Duration::from_millis(1000);

/// 放回原内容时等剪贴板的上限：监听程序读大内容会占着一阵，这一步宁可多等。
const RESTORE_PATIENCE: Duration = Duration::from_millis(3000);

/// 是句柄而不是内存块的格式（位图、图元文件、调色板、自绘显示），没法按字节存回去；位图另有 `CF_DIB` 一份。
fn is_handle_format(format: u32) -> bool {
    matches!(format, 2 | 3 | 9 | 14 | 0x80 | 0x82 | 0x83 | 0x8E)
        || (0x300..=0x3FF).contains(&format)
}

/// 密码管理器标记「别记录」的剪贴板格式；猛人剪贴板见到其中任一（后两个值为 0 时）就当密码私密发送。
fn sensitive_formats() -> [u32; 4] {
    [
        w!("ExcludeClipboardContentFromMonitorProcessing"),
        w!("Clipboard Viewer Ignore"),
        w!("CanIncludeInClipboardHistory"),
        w!("CanUploadToCloudClipboard"),
    ]
    .map(|name| unsafe { RegisterClipboardFormatW(name) })
}

/// 取选中文字；没复制到、修饰键一直按着或剪贴板打不开时回 `None`。
pub(super) fn copied_text(max_chars: usize) -> Option<String> {
    if !wait_for_modifiers() {
        tracing::info!("selection copy skipped: modifiers still held");
        return None;
    }
    let owner = MessageWindow::new()?;
    let saved = Saved::capture(owner.0)?;
    let before = unsafe { GetClipboardSequenceNumber() };
    send_copy(VK_INSERT, KEYEVENTF_EXTENDEDKEY);
    if !clipboard_changed(before) {
        // Flutter 等不认 Ctrl+Insert：再试 Ctrl+C。终端里 Ctrl+C 是中断正在跑的程序，绝不发
        if super::target::terminal_in_front() {
            tracing::info!(
                "selection copy: Ctrl+Insert copied nothing; terminal in front, no Ctrl+C"
            );
            return None;
        }
        send_copy(VK_C, KEYBD_EVENT_FLAGS(0));
        if !clipboard_changed(before) {
            // 没东西可复制（没选中、应用都不认）：剪贴板没动过，不必放回
            return None;
        }
        tracing::info!("selection copy fell back to Ctrl+C");
    }
    // 有的应用分几次写入多种格式，稍等它写完
    thread::sleep(Duration::from_millis(40));
    let text = read_text(owner.0, max_chars);
    saved.restore(owner.0);
    text
}

fn wait_for_modifiers() -> bool {
    let until = Instant::now() + MODIFIER_WAIT;
    loop {
        let held = [VK_SHIFT, VK_CONTROL, VK_MENU, VK_LWIN, VK_RWIN]
            .iter()
            .any(|key| unsafe { GetAsyncKeyState(key.0.into()) } < 0);
        if !held {
            return true;
        }
        if Instant::now() >= until {
            return false;
        }
        thread::sleep(Duration::from_millis(15));
    }
}

/// 等应用把复制内容放上剪贴板，最多 [`COPY_WAIT`]。
fn clipboard_changed(before: u32) -> bool {
    let until = Instant::now() + COPY_WAIT;
    while unsafe { GetClipboardSequenceNumber() } == before {
        if Instant::now() >= until {
            return false;
        }
        thread::sleep(Duration::from_millis(10));
    }
    true
}

/// 发一次 Ctrl + `key`（Ctrl+Insert 或 Ctrl+C）。
///
/// 每个键都带扫描码：Flutter 按扫描码认键，扫描码为 0 的模拟按键它不当 Ctrl+C（猛人快传实测，补上即可复制）。
fn send_copy(key_code: VIRTUAL_KEY, extended: KEYBD_EVENT_FLAGS) {
    let key = |vk: VIRTUAL_KEY, flags: KEYBD_EVENT_FLAGS| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: unsafe { MapVirtualKeyW(u32::from(vk.0), MAPVK_VK_TO_VSC) } as u16,
                dwFlags: flags,
                ..Default::default()
            },
        },
    };
    // Insert 带扩展键标志，是独立的 Insert 键，不是小键盘 0
    let inputs = [
        key(VK_CONTROL, KEYBD_EVENT_FLAGS(0)),
        key(key_code, extended),
        key(key_code, extended | KEYEVENTF_KEYUP),
        key(VK_CONTROL, KEYEVENTF_KEYUP),
    ];
    unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
}

/// 打开剪贴板，被别的程序占着时重试到 `patience`。复制后猛人剪贴板等监听程序会马上打开剪贴板读新内容，
/// 内容大时占着较久：放回原内容那一步要等得够久，否则原内容就丢了。
fn open(owner: HWND, patience: Duration) -> bool {
    let until = Instant::now() + patience;
    loop {
        if unsafe { OpenClipboard(Some(owner)) }.is_ok() {
            return true;
        }
        if Instant::now() >= until {
            return false;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn read_text(owner: HWND, max_chars: usize) -> Option<String> {
    if !open(owner, READ_PATIENCE) {
        return None;
    }
    let text = unsafe {
        GetClipboardData(CF_UNICODETEXT).ok().and_then(|handle| {
            let memory = HGLOBAL(handle.0);
            let pointer = GlobalLock(memory).cast::<u16>();
            if pointer.is_null() {
                return None;
            }
            let units = GlobalSize(memory) / 2;
            let slice = std::slice::from_raw_parts(pointer, units);
            let end = slice.iter().position(|&unit| unit == 0).unwrap_or(units);
            let text = String::from_utf16_lossy(&slice[..end]);
            let _ = GlobalUnlock(memory);
            Some(text)
        })
    };
    let _ = unsafe { CloseClipboard() };
    let text = text?;
    let text = text.trim();
    (!text.is_empty()).then(|| text.chars().take(max_chars).collect())
}

/// 复制前的剪贴板内容（按格式存字节）。
struct Saved(Vec<(u32, Vec<u8>)>);

impl Saved {
    fn capture(owner: HWND) -> Option<Self> {
        if !open(owner, CAPTURE_PATIENCE) {
            // 记下谁占着：远程桌面这类同步剪贴板的程序卡住时会一直占着，所有程序都没法复制
            let holder = unsafe { GetOpenClipboardWindow() }
                .ok()
                .and_then(super::target::process_name);
            tracing::info!(
                holder = holder.as_deref().unwrap_or("unknown"),
                "selection copy skipped: clipboard busy"
            );
            return None;
        }
        let sensitive = sensitive_formats();
        let mut formats = Vec::new();
        let mut total = 0usize;
        let mut format = 0;
        let mut too_big = false;
        loop {
            format = unsafe { EnumClipboardFormats(format) };
            if format == 0 {
                break;
            }
            if sensitive.contains(&format) {
                // 原内容是密码管理器复制的密码：放回去会被猛人剪贴板再私密发送一次，这次不模拟复制
                let _ = unsafe { CloseClipboard() };
                tracing::info!("selection copy skipped: clipboard holds a password");
                return None;
            }
            if is_handle_format(format) {
                continue;
            }
            let Ok(handle) = (unsafe { GetClipboardData(format) }) else {
                continue;
            };
            let memory = HGLOBAL(handle.0);
            let size = unsafe { GlobalSize(memory) };
            total += size;
            if total > MAX_SAVED_BYTES {
                too_big = true;
                break;
            }
            let pointer = unsafe { GlobalLock(memory) }.cast::<u8>();
            if pointer.is_null() {
                continue;
            }
            let bytes = unsafe { std::slice::from_raw_parts(pointer, size) }.to_vec();
            let _ = unsafe { GlobalUnlock(memory) };
            formats.push((format, bytes));
        }
        let _ = unsafe { CloseClipboard() };
        if too_big {
            tracing::info!("selection copy skipped: clipboard content too large to restore");
            return None;
        }
        Some(Self(formats))
    }

    /// 原样放回复制前的内容，不加任何标记（见文件头）。
    fn restore(&self, owner: HWND) {
        if !open(owner, RESTORE_PATIENCE) {
            tracing::warn!("cannot reopen the clipboard to restore it");
            return;
        }
        unsafe {
            let _ = EmptyClipboard();
            for (format, bytes) in &self.0 {
                put(*format, bytes);
            }
            let _ = CloseClipboard();
        }
        tracing::info!(
            formats = self.0.len(),
            "clipboard restored after selection copy"
        );
    }
}

/// 拷一块内存交给剪贴板；交出去后归剪贴板所有，失败才自己释放。
unsafe fn put(format: u32, bytes: &[u8]) {
    if format == 0 {
        return;
    }
    let Ok(memory) = (unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1)) }) else {
        return;
    };
    let pointer = unsafe { GlobalLock(memory) }.cast::<u8>();
    if pointer.is_null() {
        let _ = unsafe { GlobalFree(Some(memory)) };
        return;
    }
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), pointer, bytes.len()) };
    let _ = unsafe { GlobalUnlock(memory) };
    if let Err(error) = unsafe { SetClipboardData(format, Some(HANDLE(memory.0))) } {
        tracing::debug!(format, %error, "clipboard format not restored");
        let _ = unsafe { GlobalFree(Some(memory)) };
    }
}

/// 只收消息的隐藏窗口：剪贴板要有个所有者窗口，否则清空后 SetClipboardData 会失败。
struct MessageWindow(HWND);

impl MessageWindow {
    fn new() -> Option<Self> {
        unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                None,
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                None,
                None,
            )
        }
        .inspect_err(|error| tracing::warn!(%error, "cannot create clipboard owner window"))
        .ok()
        .map(Self)
    }
}

impl Drop for MessageWindow {
    fn drop(&mut self) {
        let _ = unsafe { DestroyWindow(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::is_handle_format;

    #[test]
    fn handle_formats_are_not_saved_as_bytes() {
        for format in [2, 3, 9, 14, 0x80, 0x300, 0x3FF] {
            assert!(is_handle_format(format), "{format}");
        }
        // 文本、DIB、文件列表、自定义格式都按字节存
        for format in [1, 8, 13, 15, 17, 0xC000] {
            assert!(!is_handle_format(format), "{format}");
        }
    }
}
