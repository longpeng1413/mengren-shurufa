//! 划词卡片：点「译」图标后在图标处弹出的小窗。单个英文词显示美式音标、释义、考试标签（本机词表，不联网），
//! 其余文字显示译文（与翻译窗口同一个翻译服务）。点到别处或按 Esc 收起；拖四边调大小，拖标题一栏移动。
//!
//! 卡片的按钮事件与收起都排进翻译窗口的事件队列，由 [`super::Window`] 处理：朗读、打开翻译窗口都在那边。

mod arrange;
mod show;
mod words;

use std::cell::Cell;

use qingjian_platform::Config;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DWM_WINDOW_CORNER_PREFERENCE, DWMWA_BORDER_COLOR, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
    DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{HDC, HFONT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{DRAWITEMSTRUCT, MEASUREITEMSTRUCT, ODT_MENU};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    BS_OWNERDRAW, CS_DROPSHADOW, CreateWindowExW, DefWindowProcW, ES_AUTOVSCROLL, ES_MULTILINE,
    ES_READONLY, GetDlgCtrlID, HMENU, IDC_ARROW, LoadCursorW, RegisterClassW, WA_INACTIVE,
    WINDOW_STYLE, WM_ACTIVATE, WM_CLOSE, WM_COMMAND, WM_CTLCOLOREDIT, WM_CTLCOLORSTATIC,
    WM_DRAWITEM, WM_ERASEBKGND, WNDCLASSW, WS_CHILD, WS_CLIPCHILDREN, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP, WS_TABSTOP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowRect, HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT, HTCAPTION, HTCLIENT, HTLEFT, HTRIGHT,
    HTTOP, HTTOPLEFT, HTTOPRIGHT, MINMAXINFO, MSGF_MENU, WM_ENTERIDLE, WM_EXITSIZEMOVE,
    WM_GETMINMAXINFO, WM_MEASUREITEM, WM_NCHITTEST, WM_NCLBUTTONDOWN, WM_SIZE, WM_SIZING,
};
use windows::core::{PCWSTR, Result, w};

use super::colors;
use super::translation::Translation;
use qingjian_translate::WordCard;
use words::Words;

/// 卡片按钮的 `WM_COMMAND`（`wParam` 原样带上）。
pub(super) const CARD_COMMAND: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 20;

/// 卡片该收起了（点到别处、关闭）。
pub(super) const CARD_CLOSE: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 21;

/// 边框四周多宽算拖边框、标题一栏多高算拖动（96 DPI 下的像素）。
const EDGE: i32 = 6;
const HEADER: i32 = 46;

thread_local! {
    /// 正在拖边框（`WM_SIZING` 到 `WM_EXITSIZEMOVE` 之间）：这期间的 `WM_SIZE` 才是用户改的大小。
    static SIZING: Cell<bool> = const { Cell::new(false) };

    /// 卡片本体，见 [`Card::bind`]：拖边框时窗口过程直接重排，不排队（排队会在松手后一口气重排几十次、闪个不停）。
    static BOUND: Cell<*mut Card> = const { Cell::new(std::ptr::null_mut()) };
}

/// 正在拖卡片边框。
pub(super) fn sizing() -> bool {
    SIZING.get()
}

const CLASS: PCWSTR = w!("Mengren.Translator.Card");

/// 静态文字不把 `&` 当助记符（`SS_NOPREFIX`，在 SystemServices 里，为它不值得多开一个特性）。
const SS_NOPREFIX: u32 = 0x80;

/// 子控件编号。不能用翻译窗口「翻译」按钮的编号 12，按钮画法按它认主按钮。
pub(super) const TITLE: usize = 1;
pub(super) const PHONETIC: usize = 2;
/// 原形与考试标签（词），或翻译状态（句子）。
pub(super) const EXTRA: usize = 3;
/// 原文（句子）。
pub(super) const ORIGINAL: usize = 4;
/// 释义（词）或译文（句子）。
pub(super) const BODY: usize = 5;
pub(super) const SPEAK_SOURCE: usize = 6;
pub(super) const SPEAK_RESULT: usize = 7;
pub(super) const COPY: usize = 8;
pub(super) const OPEN: usize = 9;
pub(super) const CLOSE: usize = 10;
/// 词典区：短语或译文里各英文词的音标与释义。
pub(super) const DETAILS: usize = 11;
/// 循环播放按钮（跳过 12：翻译窗口「翻译」主按钮的编号）。
pub(super) const LOOP: usize = 13;
const COUNT: usize = LOOP + 1;

pub(super) struct Card {
    pub(super) hwnd: HWND,

    controls: [HWND; COUNT],

    /// 标题、正文、小字三种字体及其对应的 DPI。
    fonts: Option<(HFONT, HFONT, HFONT, u32)>,

    /// 句子卡片的翻译（与翻译窗口各用各的，互不打断）。
    pub(super) model: Translation,

    words: Words,

    /// 查到的词；`None` 是句子卡片。
    pub(super) word: Option<WordCard>,

    /// 卡片的原文；为空表示 Server 没读到选中的文字，卡片只显示提示。
    pub(super) text: String,

    /// 原文是中文且开着自动朗读：译文到了要读译文。
    pub(super) read_when_translated: bool,

    /// 用户拖过边框：这张卡片按用户定的大小排，不再自动改大小。
    pub(super) manual: bool,

    /// 悬停取词弹的卡片：不抢焦点，鼠标离开卡片一段距离就收起。
    hover: bool,

    /// 句子卡的词典区：英文原文或英文译文是短语时，各实词的音标与释义；否则为空。
    details: String,

    /// 「循环」按钮上的字，由翻译窗口按循环设置给。
    loop_label: String,

    /// 弹出位置（「译」图标的左下角，物理像素）。
    anchor: (i32, i32),
}

impl Card {
    /// 建好隐藏的卡片窗口；划词翻译开着时顺手在后台读词表。
    pub(super) fn new(config: Config, preload: bool) -> Result<Self> {
        unsafe {
            let instance = GetModuleHandleW(None)?;
            let class = WNDCLASSW {
                style: CS_DROPSHADOW,
                lpfnWndProc: Some(proc),
                hInstance: instance.into(),
                lpszClassName: CLASS,
                hCursor: LoadCursorW(None, IDC_ARROW)?,
                ..Default::default()
            };
            RegisterClassW(&class);
            let hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
                CLASS,
                w!(""),
                WS_POPUP | WS_CLIPCHILDREN,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance.into()),
                None,
            )?;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                (&DWMWCP_ROUND as *const DWM_WINDOW_CORNER_PREFERENCE).cast(),
                size_of_val(&DWMWCP_ROUND) as u32,
            );
            if let Some(color) = colors::menu_border() {
                let _ = DwmSetWindowAttribute(
                    hwnd,
                    DWMWA_BORDER_COLOR,
                    (&color as *const COLORREF).cast(),
                    size_of_val(&color) as u32,
                );
            }
            let mut controls = [HWND::default(); COUNT];
            let edit = (ES_MULTILINE | ES_READONLY | ES_AUTOVSCROLL) as u32;
            for (id, class, flags) in [
                (TITLE, w!("STATIC"), SS_NOPREFIX),
                (PHONETIC, w!("STATIC"), SS_NOPREFIX),
                (EXTRA, w!("STATIC"), SS_NOPREFIX),
                (ORIGINAL, w!("EDIT"), edit),
                (BODY, w!("EDIT"), edit),
                (DETAILS, w!("EDIT"), edit),
                (SPEAK_SOURCE, w!("BUTTON"), BS_OWNERDRAW as u32),
                (SPEAK_RESULT, w!("BUTTON"), BS_OWNERDRAW as u32),
                (COPY, w!("BUTTON"), BS_OWNERDRAW as u32),
                (OPEN, w!("BUTTON"), BS_OWNERDRAW as u32),
                (CLOSE, w!("BUTTON"), BS_OWNERDRAW as u32),
                (LOOP, w!("BUTTON"), BS_OWNERDRAW as u32),
            ] {
                let tab = if class == w!("STATIC") {
                    WINDOW_STYLE(0)
                } else {
                    WS_TABSTOP
                };
                controls[id] = CreateWindowExW(
                    Default::default(),
                    class,
                    w!(""),
                    WS_CHILD | tab | WINDOW_STYLE(flags),
                    0,
                    0,
                    0,
                    0,
                    Some(hwnd),
                    Some(HMENU(id as _)),
                    Some(instance.into()),
                    None,
                )?;
            }
            let mut words = Words::default();
            if preload {
                words.preload();
            }
            Ok(Self {
                hwnd,
                controls,
                fonts: None,
                model: Translation::new(config),
                words,
                word: None,
                text: String::new(),
                read_when_translated: false,
                manual: false,
                hover: false,
                details: String::new(),
                loop_label: String::new(),
                anchor: (0, 0),
            })
        }
    }

    /// 登记卡片本体的地址，拖边框时窗口过程直接调 [`Card::resized`]。
    /// 调用后卡片不能再挪动（翻译窗口建好后放在 `run` 的局部变量里，到进程退出都不动）。
    /// 拖边框是系统在 `DispatchMessageW` 里跑的模态循环，那时没有别处借用卡片。
    pub(super) fn bind(&mut self) {
        BOUND.set(self as *mut Card);
    }

    /// 用户动了卡片（拖边框、移动、点按钮）：按用户定的大小排，悬停卡片也不再因鼠标走开而收起。
    pub(super) fn engage(&mut self) {
        self.hover = false;
    }

    /// 「循环」按钮：菜单从它下面弹出。
    pub(super) fn loop_anchor(&self) -> HWND {
        self.controls[LOOP]
    }

    /// 循环设置变了：换按钮上的字（卡片开着时按新宽度重排）。
    pub(super) fn set_loop_label(&mut self, label: &str) {
        self.loop_label = label.to_owned();
        self.set(LOOP, label);
        if self.visible() {
            self.place();
        }
    }

    /// 划词翻译刚打开：后台读词表。
    pub(super) fn preload(&mut self) {
        self.words.preload();
    }
}

impl Drop for Card {
    fn drop(&mut self) {
        if let Some((title, font, small, _)) = self.fonts.take() {
            for font in [title, font, small] {
                let _ = unsafe { windows::Win32::Graphics::Gdi::DeleteObject(font.into()) };
            }
        }
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match msg {
        // 卡片的「循环」菜单以卡片为归属窗口，菜单项的自绘也到这里
        WM_DRAWITEM if l.0 != 0 => {
            let item = unsafe { &*(l.0 as *const DRAWITEMSTRUCT) };
            if item.CtlType == ODT_MENU {
                super::popup::draw(item);
            } else {
                colors::draw_button(item);
            }
            LRESULT(1)
        }
        WM_MEASUREITEM if l.0 != 0 => {
            super::popup::measure(unsafe { &mut *(l.0 as *mut MEASUREITEMSTRUCT) });
            LRESULT(1)
        }
        WM_ENTERIDLE if w.0 as u32 == MSGF_MENU => {
            super::popup::frame(HWND(l.0 as _));
            LRESULT(0)
        }
        WM_ERASEBKGND if colors::erase_panels(hwnd, HDC(w.0 as _), &[ORIGINAL, BODY, DETAILS]) => {
            LRESULT(1)
        }
        WM_CTLCOLOREDIT | WM_CTLCOLORSTATIC => {
            let id = unsafe { GetDlgCtrlID(HWND(l.0 as _)) } as usize;
            let dc = HDC(w.0 as _);
            match colors::brush(dc, matches!(id, ORIGINAL | BODY | DETAILS)) {
                Some(brush) => {
                    if matches!(id, PHONETIC | EXTRA | ORIGINAL) {
                        colors::mute(dc);
                    }
                    LRESULT(brush.0 as isize)
                }
                None => unsafe { DefWindowProcW(hwnd, msg, w, l) },
            }
        }
        WM_COMMAND => {
            super::EVENTS.with_borrow_mut(|queue| queue.push_back((CARD_COMMAND, w.0)));
            LRESULT(0)
        }
        WM_ACTIVATE if (w.0 & 0xffff) as u32 == WA_INACTIVE => {
            super::EVENTS.with_borrow_mut(|queue| queue.push_back((CARD_CLOSE, 0)));
            LRESULT(0)
        }
        WM_CLOSE => {
            super::EVENTS.with_borrow_mut(|queue| queue.push_back((CARD_CLOSE, 0)));
            LRESULT(0)
        }
        WM_NCHITTEST => hit_test(hwnd, l),
        WM_GETMINMAXINFO => {
            let info = unsafe { &mut *(l.0 as *mut MINMAXINFO) };
            let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96) as i32;
            info.ptMinTrackSize.x = arrange::MIN_WIDTH * dpi / 96;
            info.ptMinTrackSize.y = arrange::MIN_HEIGHT * dpi / 96;
            LRESULT(0)
        }
        WM_SIZING => {
            SIZING.set(true);
            unsafe { DefWindowProcW(hwnd, msg, w, l) }
        }
        WM_EXITSIZEMOVE => {
            SIZING.set(false);
            unsafe { DefWindowProcW(hwnd, msg, w, l) }
        }
        WM_SIZE if SIZING.get() => {
            let card = BOUND.get();
            if !card.is_null() {
                // SAFETY: 见 Card::bind，拖边框的模态循环里没有别处借用卡片，卡片地址不变
                let card = unsafe { &mut *card };
                card.engage();
                card.manual = true;
                card.resized();
            }
            LRESULT(0)
        }
        WM_NCLBUTTONDOWN => {
            let card = BOUND.get();
            if !card.is_null() {
                // SAFETY: 同上，按下标题栏或边框时还没进入别的借用
                unsafe { (*card).engage() };
            }
            unsafe { DefWindowProcW(hwnd, msg, w, l) }
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, w, l) },
    }
}

/// 卡片没有系统边框：四周一圈当边框拖大小，标题一栏当标题栏拖动；落在按钮等子控件上的不归这里管。
fn hit_test(hwnd: HWND, l: LPARAM) -> LRESULT {
    let hit = unsafe { DefWindowProcW(hwnd, WM_NCHITTEST, WPARAM(0), l) };
    if hit.0 != HTCLIENT as isize {
        return hit;
    }
    let (x, y) = (
        i32::from(l.0 as u16 as i16),
        i32::from((l.0 >> 16) as u16 as i16),
    );
    let mut rect = RECT::default();
    let _ = unsafe { GetWindowRect(hwnd, &mut rect) };
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96) as i32;
    let edge = EDGE * dpi / 96;
    let (left, right) = (x < rect.left + edge, x >= rect.right - edge);
    let (top, bottom) = (y < rect.top + edge, y >= rect.bottom - edge);
    let area = match (left, right, top, bottom) {
        (true, _, true, _) => HTTOPLEFT,
        (_, true, true, _) => HTTOPRIGHT,
        (true, _, _, true) => HTBOTTOMLEFT,
        (_, true, _, true) => HTBOTTOMRIGHT,
        (true, ..) => HTLEFT,
        (_, true, ..) => HTRIGHT,
        (_, _, true, _) => HTTOP,
        (_, _, _, true) => HTBOTTOM,
        _ if y < rect.top + HEADER * dpi / 96 => HTCAPTION,
        _ => HTCLIENT,
    };
    LRESULT(area as isize)
}
