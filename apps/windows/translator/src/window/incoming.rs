//! Server 带进来的选中文字：`--stdin` 启动时从管道读（不放命令行，别的进程看得见命令行）；
//! 窗口已开时新实例用 `WM_COPYDATA` 转给它后退出。

use std::cell::RefCell;
use std::io::Read;

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::UI::WindowsAndMessaging::{SendMessageW, WM_COPYDATA};

/// `WM_COPYDATA` 的标记，认出是猛人 Server 转来的原文（"MRTX"）。
pub(super) const COPY_TEXT: usize = 0x4D52_5458;

/// 读进来的原文上限（字节）；DLL 那边最多 4000 字。
const MAX_BYTES: u64 = 64 * 1024;

thread_local! {
    /// 窗口过程收到、还没交给窗口的原文。
    static INCOMING: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// 以 `--stdin` 启动时读管道里的原文；没有或为空回 `None`。
pub(super) fn stdin_text() -> Option<String> {
    if !std::env::args().any(|arg| arg == "--stdin") {
        return None;
    }
    let mut text = String::new();
    if let Err(error) = std::io::stdin().take(MAX_BYTES).read_to_string(&mut text) {
        tracing::warn!(%error, "cannot read the selection from stdin");
        return None;
    }
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

/// 把原文转给已开着的窗口（同步发送，返回时对方已收下）。
pub(super) fn forward(hwnd: HWND, text: &str) {
    let mut units: Vec<u16> = text.encode_utf16().collect();
    let data = COPYDATASTRUCT {
        dwData: COPY_TEXT,
        cbData: (units.len() * 2) as u32,
        lpData: units.as_mut_ptr().cast(),
    };
    unsafe {
        SendMessageW(
            hwnd,
            WM_COPYDATA,
            Some(WPARAM(0)),
            Some(LPARAM(&data as *const COPYDATASTRUCT as isize)),
        );
    }
}

/// 窗口过程收到 `WM_COPYDATA`：是猛人的原文就收下，回 `true`。
///
/// # Safety
/// `l` 必须是系统交来的 `WM_COPYDATA` 参数，数据只在本次消息处理期间有效。
pub(super) unsafe fn receive(l: LPARAM) -> bool {
    let Some(data) = (unsafe { (l.0 as *const COPYDATASTRUCT).as_ref() }) else {
        return false;
    };
    if data.dwData != COPY_TEXT || data.lpData.is_null() {
        return false;
    }
    let units =
        unsafe { std::slice::from_raw_parts(data.lpData.cast::<u16>(), data.cbData as usize / 2) };
    let text = String::from_utf16_lossy(units);
    INCOMING.with_borrow_mut(|slot| *slot = Some(text));
    true
}

/// 取走收到的原文。
pub(super) fn take() -> Option<String> {
    INCOMING.with_borrow_mut(Option::take)
}

#[cfg(test)]
mod tests {
    use super::{COPY_TEXT, receive, take};
    use windows::Win32::Foundation::LPARAM;
    use windows::Win32::System::DataExchange::COPYDATASTRUCT;

    #[test]
    fn only_tagged_copydata_is_taken() {
        let mut units: Vec<u16> = "你好 world".encode_utf16().collect();
        let mut data = COPYDATASTRUCT {
            dwData: 7,
            cbData: (units.len() * 2) as u32,
            lpData: units.as_mut_ptr().cast(),
        };
        let param = LPARAM(&data as *const COPYDATASTRUCT as isize);
        assert!(!unsafe { receive(param) });
        assert_eq!(take(), None);
        data.dwData = COPY_TEXT;
        let param = LPARAM(&data as *const COPYDATASTRUCT as isize);
        assert!(unsafe { receive(param) });
        assert_eq!(take().as_deref(), Some("你好 world"));
        assert_eq!(take(), None);
    }
}
