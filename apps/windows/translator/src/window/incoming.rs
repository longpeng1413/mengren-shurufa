//! Server 带进来的选中文字：`--stdin` 启动时从管道读（不放命令行，别的进程看得见命令行）；
//! 窗口已开时新实例用 `WM_COPYDATA` 转给它后退出。划词卡片另用一个标记，内容是「`x,y`、换行、原文」。

use std::cell::RefCell;
use std::io::Read;

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::UI::WindowsAndMessaging::{SendMessageW, WM_COPYDATA};

/// `WM_COPYDATA` 的标记，认出是猛人 Server 转来的原文（"MRTX"）。
pub(super) const COPY_TEXT: usize = 0x4D52_5458;

/// 划词卡片的 `WM_COPYDATA` 标记（"MRCD"）。
pub(super) const COPY_CARD: usize = 0x4D52_4344;

/// 读进来的原文上限（字节）；DLL 那边最多 4000 字。
const MAX_BYTES: u64 = 64 * 1024;

thread_local! {
    /// 窗口过程收到、还没交给窗口的原文。
    static INCOMING: RefCell<Option<String>> = const { RefCell::new(None) };

    /// 收到、还没弹的划词卡片：屏幕位置、原文、是不是悬停卡片。
    static CARD: RefCell<Option<(i32, i32, String, bool)>> = const { RefCell::new(None) };
}

/// `--card x,y`：把原文弹成划词卡片，`x,y` 是屏幕上的位置（物理像素）；带 `--hover` 是悬停卡片。
pub(super) fn card_point() -> Option<(i32, i32, bool)> {
    let mut args = std::env::args();
    args.find(|arg| arg == "--card")?;
    let (x, y, _) = parse_point(&args.next()?)?;
    Some((x, y, std::env::args().any(|arg| arg == "--hover")))
}

/// `--background`：只在后台起好进程，不显示窗口。
pub(super) fn background() -> bool {
    std::env::args().any(|arg| arg == "--background")
}

/// `x,y` 或 `x,y,h`（`h` 表示悬停卡片）。
fn parse_point(text: &str) -> Option<(i32, i32, bool)> {
    let mut parts = text.split(',');
    let x = parts.next()?.trim().parse().ok()?;
    let y = parts.next()?.trim().parse().ok()?;
    Some((x, y, parts.next().is_some_and(|flag| flag.trim() == "h")))
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
    // 有的写入端会在开头带 UTF-8 的 BOM，它不算空白，留着会粘在第一个词上
    let text = text.trim_start_matches('\u{feff}').trim();
    (!text.is_empty()).then(|| text.to_owned())
}

/// 把原文转给已开着的窗口（同步发送，返回时对方已收下）。
pub(super) fn forward(hwnd: HWND, text: &str) {
    send(hwnd, COPY_TEXT, text);
}

/// 把划词卡片请求转给已开着的窗口。
pub(super) fn forward_card(hwnd: HWND, x: i32, y: i32, hover: bool, text: &str) {
    let flag = if hover { ",h" } else { "" };
    send(hwnd, COPY_CARD, &format!("{x},{y}{flag}\n{text}"));
}

fn send(hwnd: HWND, tag: usize, text: &str) {
    let mut units: Vec<u16> = text.encode_utf16().collect();
    let data = COPYDATASTRUCT {
        dwData: tag,
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

/// 窗口过程收到 `WM_COPYDATA`：是猛人的原文或划词卡片就收下，回收下的标记（[`COPY_TEXT`] / [`COPY_CARD`]）。
///
/// # Safety
/// `l` 必须是系统交来的 `WM_COPYDATA` 参数，数据只在本次消息处理期间有效。
pub(super) unsafe fn receive(l: LPARAM) -> Option<usize> {
    let data = unsafe { (l.0 as *const COPYDATASTRUCT).as_ref() }?;
    if !matches!(data.dwData, COPY_TEXT | COPY_CARD) || data.lpData.is_null() {
        return None;
    }
    let units =
        unsafe { std::slice::from_raw_parts(data.lpData.cast::<u16>(), data.cbData as usize / 2) };
    let text = String::from_utf16_lossy(units);
    if data.dwData == COPY_TEXT {
        INCOMING.with_borrow_mut(|slot| *slot = Some(text));
    } else {
        let (point, text) = text.split_once('\n')?;
        let (x, y, hover) = parse_point(point)?;
        CARD.with_borrow_mut(|slot| *slot = Some((x, y, text.to_owned(), hover)));
    }
    Some(data.dwData)
}

/// 取走收到的原文。
pub(super) fn take() -> Option<String> {
    INCOMING.with_borrow_mut(Option::take)
}

/// 取走收到的划词卡片请求。
pub(super) fn take_card() -> Option<(i32, i32, String, bool)> {
    CARD.with_borrow_mut(Option::take)
}

#[cfg(test)]
mod tests {
    use super::{COPY_CARD, COPY_TEXT, parse_point, receive, take, take_card};
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
        assert_eq!(unsafe { receive(param) }, None);
        assert_eq!(take(), None);
        data.dwData = COPY_TEXT;
        let param = LPARAM(&data as *const COPYDATASTRUCT as isize);
        assert_eq!(unsafe { receive(param) }, Some(COPY_TEXT));
        assert_eq!(take().as_deref(), Some("你好 world"));
        assert_eq!(take(), None);
    }

    #[test]
    fn card_requests_carry_a_screen_point() {
        let mut units: Vec<u16> = "-1280,640\nrun\nfast".encode_utf16().collect();
        let data = COPYDATASTRUCT {
            dwData: COPY_CARD,
            cbData: (units.len() * 2) as u32,
            lpData: units.as_mut_ptr().cast(),
        };
        let param = LPARAM(&data as *const COPYDATASTRUCT as isize);
        assert_eq!(unsafe { receive(param) }, Some(COPY_CARD));
        assert_eq!(
            take_card(),
            Some((-1280, 640, "run\nfast".to_owned(), false))
        );
        assert_eq!(take(), None);
        assert_eq!(parse_point(" 10 , 20"), Some((10, 20, false)));
        assert_eq!(parse_point("10,20,h"), Some((10, 20, true)));
        assert_eq!(parse_point("10"), None);
    }
}
