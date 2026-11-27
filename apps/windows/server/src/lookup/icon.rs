//! 「译」图标：划词后出现在鼠标旁的小方块，点它读出选中的文字并请翻译窗口程序弹卡片。
//! 图标不抢焦点（原程序的选区与焦点都不动，读选区才读得到），几秒不点自己收起。

use std::cell::RefCell;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use qingjian_platform::windows_text::tr;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateFontW, CreateRoundRectRgn,
    CreateSolidBrush, DEFAULT_CHARSET, DT_CENTER, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER,
    DeleteObject, DrawTextW, EndPaint, FillRect, GetMonitorInfoW, HFONT, InvalidateRect,
    MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint, OUT_DEFAULT_PRECIS, PAINTSTRUCT,
    SelectObject, SetBkMode, SetTextColor, SetWindowRgn, TRANSPARENT, VARIABLE_PITCH,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime;
use windows::Win32::UI::WindowsAndMessaging::{
    CURSORINFO, CreateWindowExW, DefWindowProcW, DispatchMessageW, FindWindowW, GA_ROOT,
    GetAncestor, GetClassNameW, GetClientRect, GetCursorInfo, GetForegroundWindow, GetMessageW,
    GetSystemMetrics, GetWindowRect, GetWindowThreadProcessId, HWND_TOPMOST, IDC_HAND, IDC_IBEAM,
    KillTimer, LoadCursorW, MA_NOACTIVATE, MSG, RegisterClassW, SM_CXDOUBLECLK, SM_CXDRAG, SW_HIDE,
    SWP_NOACTIVATE, SWP_SHOWWINDOW, SetTimer, SetWindowPos, ShowWindow, TranslateMessage, WM_APP,
    WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_PAINT, WM_TIMER, WNDCLASSW, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, WindowFromPoint,
};
use windows::core::w;

use super::gesture::Gesture;
use super::hover::Dwell;
use super::watch::{self, HOVER_TIMER};
use super::{HOVER, ICON, VISIBLE, hover_mode, unpack};

/// 钩子转来的左键按下：`wParam` 是系统毫秒时间，`lParam` 是打包的屏幕坐标。
pub(super) const MSG_DOWN: u32 = WM_APP + 1;

/// 钩子转来的左键松开，参数同 [`MSG_DOWN`]。
pub(super) const MSG_UP: u32 = WM_APP + 2;

/// 收起图标。
pub(super) const MSG_HIDE: u32 = WM_APP + 3;

/// 悬停取词方式变了：`wParam` 是编码，见 `hover_code`。
pub(super) const MSG_HOVER: u32 = WM_APP + 4;

/// 图标边长、离鼠标的距离、圆角（96 DPI 下的像素）。
const SIZE: i32 = 26;
const OFFSET: i32 = 10;
const RADIUS: i32 = 8;

/// 多久不点就收起（毫秒）。
const LIFETIME: u32 = 4000;

const TIMER: usize = 1;

/// 底色（翻译窗口「翻译」按钮的薄荷绿）与字色，`0x00BBGGRR`。
const FILL: COLORREF = COLORREF(0x00a4_c971);
const INK: COLORREF = COLORREF(0x001f_1f1f);

/// 翻译窗口程序主窗口的类名：还没起就先在后台起好，点图标时卡片弹得快。
const TRANSLATOR_CLASS: windows::core::PCWSTR = w!("Mengren.Translator.Window");

/// 这些顶层窗口上的拖动不是选文字、悬停也不取词：桌面、任务栏，以及翻译窗口、卡片自己。
const SKIPPED: [&str; 6] = [
    "Progman",
    "WorkerW",
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
    "Mengren.Translator.Window",
    "Mengren.Translator.Card",
];

/// 后台预起翻译窗口程序后，这么久内不再起。
const PREWARM_GAP: Duration = Duration::from_secs(10);

struct State {
    gesture: Gesture,

    /// 按下时鼠标下的顶层窗口与它的位置，松开时比一比，认出拖窗口。
    root: HWND,

    rect: RECT,

    /// 图标文字的字体及其对应的 DPI。
    font: Option<(HFONT, u32)>,

    prewarmed: Option<Instant>,

    /// 这次按下时鼠标指针是不是 I 形文字光标：只有在文字上按下拖选、双击，才可能选中了文字。
    text_cursor: bool,

    /// 悬停取词的停留判断。
    dwell: Dwell,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

/// 起图标线程：建隐藏的图标窗口、登记句柄、跑消息循环。
pub(super) fn spawn() {
    let spawned = std::thread::Builder::new()
        .name("qingjian-lookup".to_owned())
        .spawn(|| unsafe {
            let Ok(module) = GetModuleHandleW(None) else {
                return;
            };
            let class = WNDCLASSW {
                lpfnWndProc: Some(proc),
                hInstance: module.into(),
                lpszClassName: w!("Mengren.Lookup.Icon"),
                hCursor: LoadCursorW(None, IDC_HAND).unwrap_or_default(),
                ..Default::default()
            };
            RegisterClassW(&class);
            let created = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                w!("Mengren.Lookup.Icon"),
                w!(""),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(module.into()),
                None,
            );
            let hwnd = match created {
                Ok(hwnd) => hwnd,
                Err(error) => {
                    tracing::warn!(%error, "lookup icon window unavailable");
                    return;
                }
            };
            let drag = GetSystemMetrics(SM_CXDRAG).max(4) * 2;
            let slop = GetSystemMetrics(SM_CXDOUBLECLK).max(4);
            STATE.with_borrow_mut(|state| {
                *state = Some(State {
                    gesture: Gesture::new(drag, GetDoubleClickTime(), slop),
                    root: HWND::default(),
                    rect: RECT::default(),
                    font: None,
                    prewarmed: None,
                    text_cursor: false,
                    dwell: Dwell::default(),
                });
            });
            ICON.store(hwnd.0 as isize, Ordering::Release);
            // 钩子配置可能先于窗口下发了悬停方式：按记下的再开一次
            with_state(|state| state.hover(hwnd, HOVER.load(Ordering::Acquire)));
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, "cannot spawn the lookup icon thread");
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match msg {
        MSG_DOWN => {
            let (x, y) = unpack(l.0);
            with_state(|state| state.down(hwnd, x, y, w.0 as u32));
            LRESULT(0)
        }
        MSG_UP => {
            let (x, y) = unpack(l.0);
            with_state(|state| state.up(hwnd, x, y));
            LRESULT(0)
        }
        MSG_HOVER => {
            with_state(|state| state.hover(hwnd, w.0));
            LRESULT(0)
        }
        WM_TIMER if w.0 == HOVER_TIMER => {
            with_state(|state| {
                if watch::tick(&mut state.dwell) {
                    state.prewarm();
                }
            });
            LRESULT(0)
        }
        MSG_HIDE | WM_TIMER => {
            hide(hwnd);
            LRESULT(0)
        }
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        // 鼠标停在图标上就重新计时
        WM_MOUSEMOVE => {
            unsafe { SetTimer(Some(hwnd), TIMER, LIFETIME, None) };
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            click(hwnd);
            LRESULT(0)
        }
        WM_PAINT => {
            with_state(|state| state.paint(hwnd));
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, w, l) },
    }
}

fn with_state(f: impl FnOnce(&mut State)) {
    STATE.with_borrow_mut(|state| {
        if let Some(state) = state.as_mut() {
            f(state);
        }
    });
}

impl State {
    /// 换悬停方式：开着就开采样定时器并预起翻译窗口程序。
    fn hover(&mut self, hwnd: HWND, code: usize) {
        let mode = hover_mode(code);
        if mode == self.dwell.mode() {
            return;
        }
        watch::apply(hwnd, &mut self.dwell, mode);
        if mode != qingjian_platform::HoverMode::Off {
            self.prewarm();
        }
    }

    fn down(&mut self, hwnd: HWND, x: i32, y: i32, at: u32) {
        // 点的就是图标：图标自己收 WM_LBUTTONUP 处理
        if VISIBLE.load(Ordering::Acquire) && contains(hwnd, x, y) {
            return;
        }
        hide(hwnd);
        self.root = root_at(x, y);
        self.rect = window_rect(self.root);
        self.text_cursor = text_cursor();
        self.gesture.down(x, y, at);
    }

    fn up(&mut self, hwnd: HWND, x: i32, y: i32) {
        let moved = window_rect(self.root) != self.rect;
        // 手势像选字还不够：拖图片、双击 K 线图或空白处也是这个手势。按下或松开时指针是文字光标
        // （拖选常从行首空白处起手、落在字上），或辅助接口读得到选中的文字，才出图标
        if self.gesture.up(x, y, moved)
            && eligible(self.root)
            && (self.text_cursor || text_cursor() || crate::selection::has_selected_text())
        {
            self.show(hwnd, x, y);
            self.prewarm();
        }
    }

    fn show(&mut self, hwnd: HWND, x: i32, y: i32) {
        unsafe {
            let point = POINT { x, y };
            let monitor = MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST);
            let (mut dpi, mut unused) = (96, 96);
            let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut unused);
            let px = |value: i32| value * dpi.max(96) as i32 / 96;
            let (size, offset) = (px(SIZE), px(OFFSET));
            let mut info = MONITORINFO {
                cbSize: size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            let _ = GetMonitorInfoW(monitor, &mut info);
            let work = info.rcWork;
            // 放在鼠标右上方；贴着屏幕上沿时改到下方
            let left = (x + offset).min(work.right - size).max(work.left);
            let mut top = y - offset - size;
            if top < work.top {
                top = y + offset;
            }
            let top = top.min(work.bottom - size);
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                left,
                top,
                size,
                size,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            let region = CreateRoundRectRgn(0, 0, size + 1, size + 1, px(RADIUS), px(RADIUS));
            // 区域交给系统后归窗口所有，不能再删
            let _ = SetWindowRgn(hwnd, Some(region), true);
            if self.font.is_none_or(|(_, at)| at != dpi) {
                if let Some((old, _)) = self.font.take() {
                    let _ = DeleteObject(old.into());
                }
                let font = CreateFontW(
                    -px(15),
                    0,
                    0,
                    0,
                    700,
                    0,
                    0,
                    0,
                    DEFAULT_CHARSET,
                    OUT_DEFAULT_PRECIS,
                    CLIP_DEFAULT_PRECIS,
                    CLEARTYPE_QUALITY,
                    VARIABLE_PITCH.0 as u32,
                    w!("Microsoft YaHei UI"),
                );
                self.font = Some((font, dpi));
            }
            let _ = InvalidateRect(Some(hwnd), None, true);
            VISIBLE.store(true, Ordering::Release);
            SetTimer(Some(hwnd), TIMER, LIFETIME, None);
        }
    }

    fn paint(&self, hwnd: HWND) {
        unsafe {
            let mut paint = PAINTSTRUCT::default();
            let dc = BeginPaint(hwnd, &mut paint);
            let mut rect = RECT::default();
            let _ = GetClientRect(hwnd, &mut rect);
            let brush = CreateSolidBrush(FILL);
            FillRect(dc, &rect, brush);
            let _ = DeleteObject(brush.into());
            SetBkMode(dc, TRANSPARENT);
            SetTextColor(dc, INK);
            let old = self.font.map(|(font, _)| SelectObject(dc, font.into()));
            let mut text: Vec<u16> = tr("translate").encode_utf16().collect();
            DrawTextW(
                dc,
                &mut text,
                &mut rect,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
            );
            if let Some(old) = old {
                SelectObject(dc, old);
            }
            let _ = EndPaint(hwnd, &paint);
        }
    }

    /// 翻译窗口程序还没起就在后台起好（不显示窗口），点图标时只差转交文字。
    fn prewarm(&mut self) {
        if self.prewarmed.is_some_and(|at| at.elapsed() < PREWARM_GAP) {
            return;
        }
        self.prewarmed = Some(Instant::now());
        if unsafe { FindWindowW(TRANSLATOR_CLASS, None) }.is_err() {
            crate::ui::launch_translator(&["--background".to_owned()], None);
        }
    }
}

/// 鼠标指针此刻是不是系统的 I 形文字光标。
fn text_cursor() -> bool {
    let mut info = CURSORINFO {
        cbSize: size_of::<CURSORINFO>() as u32,
        ..Default::default()
    };
    unsafe {
        GetCursorInfo(&mut info).is_ok()
            && LoadCursorW(None, IDC_IBEAM).is_ok_and(|ibeam| ibeam == info.hCursor)
    }
}

fn hide(hwnd: HWND) {
    if VISIBLE.swap(false, Ordering::AcqRel) {
        unsafe {
            let _ = KillTimer(Some(hwnd), TIMER);
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
    }
}

/// 点了图标：收起，后台读选中的文字（辅助接口，不行再模拟复制），请翻译窗口程序在图标处弹卡片；
/// 没读到也弹，卡片上说明原因，免得点了没反应。
fn click(hwnd: HWND) {
    let rect = window_rect(hwnd);
    hide(hwnd);
    let at = format!("{},{}", rect.left, rect.bottom);
    let spawned = std::thread::Builder::new()
        .name("qingjian-lookup-read".to_owned())
        .spawn(move || {
            let text = crate::selection::read();
            if text.is_none() {
                tracing::info!("lookup icon clicked but no selection was read");
            }
            crate::ui::launch_translator(&["--card".to_owned(), at], text);
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, "cannot spawn the lookup selection reader");
    }
}

fn contains(hwnd: HWND, x: i32, y: i32) -> bool {
    let rect = window_rect(hwnd);
    (rect.left..rect.right).contains(&x) && (rect.top..rect.bottom).contains(&y)
}

/// 屏幕点下的顶层窗口。
pub(super) fn root_at(x: i32, y: i32) -> HWND {
    unsafe { GetAncestor(WindowFromPoint(POINT { x, y }), GA_ROOT) }
}

fn window_rect(hwnd: HWND) -> RECT {
    let mut rect = RECT::default();
    if !hwnd.is_invalid() {
        let _ = unsafe { GetWindowRect(hwnd, &mut rect) };
    }
    rect
}

/// 选文字的地方才弹：是当前前台窗口，不是桌面、任务栏、猛人自己的窗口。
fn eligible(root: HWND) -> bool {
    eligible_window(root) && unsafe { GetAncestor(GetForegroundWindow(), GA_ROOT) } == root
}

/// 不是桌面、任务栏、猛人自己的窗口（悬停取词不要求是前台窗口）。
pub(super) fn eligible_window(root: HWND) -> bool {
    if root.is_invalid() {
        return false;
    }
    unsafe {
        let mut pid = 0;
        GetWindowThreadProcessId(root, Some(&mut pid));
        if pid == GetCurrentProcessId() {
            return false;
        }
        let mut name = [0u16; 64];
        let len = GetClassNameW(root, &mut name).max(0) as usize;
        let class = String::from_utf16_lossy(&name[..len]);
        !SKIPPED.contains(&class.as_str())
    }
}
