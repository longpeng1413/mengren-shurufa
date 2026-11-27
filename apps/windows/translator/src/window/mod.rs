//! 原生多行编辑框保留 TSF 输入。窗口过程只排队，控件通知不会重入界面状态。
mod card;
mod colors;
mod fit;
mod incoming;
mod layout;
mod lookup;
mod popup;
mod repeat_menu;
mod run;
mod speech;
mod translation;
use card::{CARD_CLOSE, CARD_COMMAND, Card};
use qingjian_platform::{Config, LookupConfig, windows_text::tr};
pub use run::run;
use speech::{MM_MCINOTIFY, Outcome, SPEEDS, Side, Source, Speech, VOICE_IN};
use std::{
    cell::RefCell,
    collections::VecDeque,
    time::{Duration, Instant},
};
use translation::{LANGS, Translation};
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, RECT, SIZE, WPARAM},
    Graphics::Gdi::{
        COLOR_WINDOW, DeleteObject, GetDC, GetTextExtentPoint32W, HDC, HFONT, ReleaseDC,
        SelectObject,
    },
    System::LibraryLoader::GetModuleHandleW,
    UI::{HiDpi::GetDpiForWindow, Input::KeyboardAndMouse::SetFocus, WindowsAndMessaging::*},
};
use windows::core::{HSTRING, PCWSTR, Result, w};
const CLASS: PCWSTR = w!("Mengren.Translator.Window");
const TOGGLE: u32 = WM_APP + 1;
/// 收到 Server 转来的原文；投递一条让消息循环醒来处理（跨进程 SendMessage 不会让 GetMessage 返回）。
const TEXT_IN: u32 = WM_APP + 2;
/// 收到 Server 转来的划词卡片请求（+3 已给联网朗读）。
const CARD_IN: u32 = WM_APP + 22;
const LANG: usize = 1;
const SWAP: usize = 2;
const SOURCE: usize = 3;
const READ_SOURCE: usize = 4;
const CLEAR: usize = 5;
const RESULT: usize = 6;
const READ_RESULT: usize = 7;
const COPY: usize = 8;
const STATUS: usize = 9;
const LABEL: usize = 10;
const BACKGROUND: usize = 11;
const TRANSLATE: usize = 12;
const SOURCE_LABEL: usize = 13;
const RESULT_LABEL: usize = 14;
const SPEED: usize = 15;
/// 底部说明小字：本机能朗读哪些语言。
const NOTE: usize = 16;
/// 原文的倍速按钮（`SPEED` 是译文的）。
const SOURCE_SPEED: usize = 17;
/// 「取词…」菜单按钮。
const LOOKUP: usize = 18;
/// 循环播放按钮（译文一行）。
const LOOP: usize = 19;
/// 原文一行的循环播放按钮，与 `LOOP` 改同一份设置。
const SOURCE_LOOP: usize = 20;
const CONTROL_COUNT: usize = SOURCE_LOOP + 1;
/// 按钮高度（96 DPI 下的像素）。
const BUTTON_HEIGHT: i32 = 28;
/// 按钮文字两侧留白之和。
const BUTTON_PADDING: i32 = 28;
/// 窗口默认外框尺寸（96 DPI 下的像素）。
const DEFAULT_WIDTH: i32 = 660;
const DEFAULT_HEIGHT: i32 = 615;
/// 客户区里两块文本区以外的固定高度（96 DPI 下的像素），两块文本区平分剩下的。
const FIXED_HEIGHT: i32 = 268;
/// 每块文本区比其中的文本框高出的部分（96 DPI 下的像素）。
const BOX_MARGIN: i32 = 20;
thread_local! {static EVENTS:RefCell<VecDeque<(u32,usize)>>=const{RefCell::new(VecDeque::new())};}
unsafe extern "system" fn proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match msg {
        WM_DRAWITEM if l.0 != 0 => {
            let item = unsafe { &*(l.0 as *const windows::Win32::UI::Controls::DRAWITEMSTRUCT) };
            if item.CtlType == windows::Win32::UI::Controls::ODT_MENU {
                popup::draw(item);
            } else {
                colors::draw_button(item);
            }
            LRESULT(1)
        }
        WM_MEASUREITEM if l.0 != 0 => {
            popup::measure(unsafe {
                &mut *(l.0 as *mut windows::Win32::UI::Controls::MEASUREITEMSTRUCT)
            });
            LRESULT(1)
        }
        // 菜单弹着时 wParam 是 MSGF_MENU、lParam 是菜单窗口
        WM_ENTERIDLE if w.0 as u32 == MSGF_MENU => {
            popup::frame(HWND(l.0 as _));
            LRESULT(0)
        }
        WM_ERASEBKGND if colors::erase(hwnd, HDC(w.0 as _)) => LRESULT(1),
        WM_CTLCOLOREDIT | WM_CTLCOLORSTATIC | WM_CTLCOLORLISTBOX => {
            let id = unsafe { GetDlgCtrlID(HWND(l.0 as _)) } as usize;
            if let Some(brush) = colors::brush(HDC(w.0 as _), matches!(id, SOURCE | RESULT)) {
                if id == NOTE {
                    colors::mute(HDC(w.0 as _));
                }
                LRESULT(brush.0 as isize)
            } else {
                unsafe { DefWindowProcW(hwnd, msg, w, l) }
            }
        }
        WM_GETMINMAXINFO => {
            let size = unsafe { &mut *(l.0 as *mut MINMAXINFO) };
            let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96) as i32;
            size.ptMinTrackSize.x = 640 * dpi / 96;
            size.ptMinTrackSize.y = 455 * dpi / 96;
            LRESULT(0)
        }
        WM_COPYDATA => match unsafe { incoming::receive(l) } {
            Some(tag) => {
                let event = if tag == incoming::COPY_CARD {
                    CARD_IN
                } else {
                    TEXT_IN
                };
                let _ = unsafe { PostMessageW(Some(hwnd), event, WPARAM(0), LPARAM(0)) };
                LRESULT(1)
            }
            None => LRESULT(0),
        },
        // 切回窗口时重查朗读声音：用户可能刚在系统设置里装了语音
        WM_ACTIVATE if w.0 & 0xFFFF != WA_INACTIVE as usize => {
            EVENTS.with_borrow_mut(|q| q.push_back((msg, w.0)));
            unsafe { DefWindowProcW(hwnd, msg, w, l) }
        }
        WM_CLOSE | WM_SIZE | WM_TIMER | WM_COMMAND | TOGGLE | TEXT_IN | CARD_IN | VOICE_IN
        | MM_MCINOTIFY => {
            EVENTS.with_borrow_mut(|q| q.push_back((msg, w.0)));
            LRESULT(0)
        }
        WM_DPICHANGED => {
            let r = unsafe { &*(l.0 as *const RECT) };
            let _ = unsafe {
                SetWindowPos(
                    hwnd,
                    None,
                    r.left,
                    r.top,
                    r.right - r.left,
                    r.bottom - r.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                )
            };
            EVENTS.with_borrow_mut(|q| q.push_back((WM_SIZE, 0)));
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, w, l) },
    }
}
struct Window {
    hwnd: HWND,
    controls: [HWND; CONTROL_COUNT],
    font: HFONT,

    /// 底部说明小字用的小号字体。
    small: HFONT,
    dpi: u32,
    model: Translation,
    speech: Speech,

    /// 当前的朗读是卡片发起的：卡片收起时要一起停掉。
    card_speech: bool,
    hidden: Option<Instant>,

    /// 上次自动调出的窗口外框尺寸；当前尺寸与它不同说明用户拖过，就不再缩小。
    fitted: Option<(i32, i32)>,

    /// 划词卡片。
    card: Card,

    /// `[lookup]`：划词翻译开着时进程常驻，随时弹卡片。
    lookup: LookupConfig,
}
impl Window {
    /// `visible` 为假时只建好窗口不显示（后台预起、只弹卡片）。
    fn new(config: Config, visible: bool) -> Result<Self> {
        unsafe {
            colors::init();
            let inst = GetModuleHandleW(None)?;
            let wc = WNDCLASSW {
                lpszClassName: CLASS,
                hInstance: inst.into(),
                lpfnWndProc: Some(proc),
                hCursor: LoadCursorW(None, IDC_ARROW)?,
                hbrBackground: windows::Win32::Graphics::Gdi::HBRUSH(
                    (COLOR_WINDOW.0 + 1) as usize as _,
                ),
                ..Default::default()
            };
            RegisterClassW(&wc);
            let mut r = RECT::default();
            SystemParametersInfoW(
                SPI_GETWORKAREA,
                0,
                Some(&mut r as *mut _ as _),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            )?;
            let hwnd = CreateWindowExW(
                Default::default(),
                CLASS,
                &HSTRING::from(tr("title")),
                WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
                r.left + (r.right - r.left) / 4,
                r.top + (r.bottom - r.top) / 7,
                DEFAULT_WIDTH,
                DEFAULT_HEIGHT,
                None,
                None,
                Some(inst.into()),
                None,
            )?;
            // 进程按 PerMonitorV2 运行，创建时给的是物理像素：按窗口所在屏的缩放放大到设计尺寸，
            // 否则 125% / 150% 缩放的屏上按钮按缩放摆放会挤出窗口
            let dpi = GetDpiForWindow(hwnd).max(96) as i32;
            if dpi != 96 {
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    0,
                    0,
                    DEFAULT_WIDTH * dpi / 96,
                    DEFAULT_HEIGHT * dpi / 96,
                    SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            colors::caption(hwnd);
            let mut controls = [HWND::default(); CONTROL_COUNT];
            for (id, class, label, flags) in [
                (LANG, w!("BUTTON"), "", BS_OWNERDRAW as u32),
                (SWAP, w!("BUTTON"), "swap", BS_OWNERDRAW as u32),
                (BACKGROUND, w!("BUTTON"), "background", BS_OWNERDRAW as u32),
                (LOOKUP, w!("BUTTON"), "lookup", BS_OWNERDRAW as u32),
                (
                    SOURCE,
                    w!("EDIT"),
                    "",
                    ES_MULTILINE as u32 | ES_AUTOVSCROLL as u32 | ES_WANTRETURN as u32,
                ),
                (
                    READ_SOURCE,
                    w!("BUTTON"),
                    "read_source",
                    BS_OWNERDRAW as u32,
                ),
                (CLEAR, w!("BUTTON"), "clear", BS_OWNERDRAW as u32),
                (
                    TRANSLATE,
                    w!("BUTTON"),
                    "translate_now",
                    BS_OWNERDRAW as u32,
                ),
                (
                    RESULT,
                    w!("EDIT"),
                    "",
                    ES_MULTILINE as u32 | ES_READONLY as u32 | ES_AUTOVSCROLL as u32,
                ),
                (
                    READ_RESULT,
                    w!("BUTTON"),
                    "read_result",
                    BS_OWNERDRAW as u32,
                ),
                (SPEED, w!("BUTTON"), "", BS_OWNERDRAW as u32),
                (LOOP, w!("BUTTON"), "", BS_OWNERDRAW as u32),
                (SOURCE_LOOP, w!("BUTTON"), "", BS_OWNERDRAW as u32),
                (SOURCE_SPEED, w!("BUTTON"), "", BS_OWNERDRAW as u32),
                (COPY, w!("BUTTON"), "copy", BS_OWNERDRAW as u32),
                (STATUS, w!("STATIC"), "", 0),
                (NOTE, w!("STATIC"), "", 0),
                (LABEL, w!("STATIC"), "language", 0),
                (SOURCE_LABEL, w!("STATIC"), "source", 0),
                (RESULT_LABEL, w!("STATIC"), "result", 0),
            ] {
                let title = if (id == SOURCE || id == RESULT) || label.is_empty() {
                    ""
                } else {
                    tr(label)
                };
                controls[id] = CreateWindowExW(
                    Default::default(),
                    class,
                    &HSTRING::from(title),
                    WS_CHILD
                        | WS_VISIBLE
                        | (if matches!(id, STATUS | NOTE | LABEL | SOURCE_LABEL | RESULT_LABEL) {
                            WINDOW_STYLE(0)
                        } else {
                            WS_TABSTOP
                        })
                        | WINDOW_STYLE(flags),
                    0,
                    0,
                    0,
                    0,
                    Some(hwnd),
                    Some(HMENU(id as _)),
                    Some(inst.into()),
                    None,
                )?;
            }
            let lookup = config.lookup;
            let card = Card::new(
                config.clone(),
                lookup.selection || lookup.hover != qingjian_platform::HoverMode::Off,
            )?;
            let model = Translation::new(config);
            let mut this = Self {
                hwnd,
                controls,
                font: HFONT::default(),
                small: HFONT::default(),
                dpi: 0,
                model,
                speech: Speech::new(hwnd),
                card_speech: false,
                hidden: (!visible).then(Instant::now),
                fitted: None,
                card,
                lookup,
            };
            this.layout();
            this.render();
            this.add_tooltip(SPEED, tr("speed_tip"));
            this.add_tooltip(SOURCE_SPEED, tr("speed_tip"));
            this.add_tooltip(LOOP, tr("loop_tip"));
            this.add_tooltip(SOURCE_LOOP, tr("loop_tip"));
            if visible {
                let _ = ShowWindow(hwnd, SW_SHOW);
                let _ = SetFocus(Some(this.controls[SOURCE]));
            }
            SetTimer(Some(hwnd), 1, 100, None);
            Ok(this)
        }
    }

    /// 文字在窗口字体下的宽度（96 DPI 下的像素）。
    fn text_width(&self, text: &str) -> i32 {
        let wide: Vec<u16> = text.encode_utf16().collect();
        let mut size = SIZE::default();
        unsafe {
            let dc = GetDC(Some(self.hwnd));
            let old = SelectObject(dc, self.font.into());
            let _ = GetTextExtentPoint32W(dc, &wide, &mut size);
            SelectObject(dc, old);
            ReleaseDC(Some(self.hwnd), dc);
        }
        size.cx * 96 / self.dpi.max(96) as i32
    }

    /// 放得下这段文字的按钮宽度，不小于 `min`。
    fn button_width(&self, text: &str, min: i32) -> i32 {
        (self.text_width(text) + BUTTON_PADDING).max(min)
    }

    /// 在按钮下方弹出跟随配色的下拉菜单，返回选中项的下标。
    fn menu(&self, id: usize, labels: &[&str], checked: &[usize]) -> Option<usize> {
        let mut r = RECT::default();
        unsafe { GetWindowRect(self.controls[id], &mut r) }.ok()?;
        popup::show(self.hwnd, r, self.font, labels, checked)
    }
    /// 语速按钮的悬停说明；底色深时用深色提示框。
    fn add_tooltip(&self, id: usize, text: &str) {
        use windows::Win32::UI::Controls::{
            ICC_WIN95_CLASSES, INITCOMMONCONTROLSEX, InitCommonControlsEx, SetWindowTheme,
            TOOLTIPS_CLASSW, TTF_IDISHWND, TTF_SUBCLASS, TTM_ADDTOOLW, TTM_SETMAXTIPWIDTH,
            TTS_ALWAYSTIP, TTS_NOPREFIX, TTTOOLINFOW,
        };
        unsafe {
            let _ = InitCommonControlsEx(&INITCOMMONCONTROLSEX {
                dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
                dwICC: ICC_WIN95_CLASSES,
            });
            let Ok(tip) = CreateWindowExW(
                WS_EX_TOPMOST,
                TOOLTIPS_CLASSW,
                None,
                WS_POPUP | WINDOW_STYLE(TTS_ALWAYSTIP | TTS_NOPREFIX),
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                Some(self.hwnd),
                None,
                None,
                None,
            ) else {
                return;
            };
            if colors::dark() {
                let _ = SetWindowTheme(tip, w!("DarkMode_Explorer"), PCWSTR::null());
            }
            let mut text: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
            let info = TTTOOLINFOW {
                cbSize: size_of::<TTTOOLINFOW>() as u32,
                uFlags: TTF_IDISHWND | TTF_SUBCLASS,
                hwnd: self.hwnd,
                uId: self.controls[id].0 as usize,
                lpszText: windows::core::PWSTR(text.as_mut_ptr()),
                ..Default::default()
            };
            SendMessageW(
                tip,
                TTM_ADDTOOLW,
                None,
                Some(LPARAM(&info as *const _ as isize)),
            );
            let dpi = GetDpiForWindow(self.hwnd).max(96) as isize;
            SendMessageW(tip, TTM_SETMAXTIPWIDTH, None, Some(LPARAM(320 * dpi / 96)));
        }
    }
    /// 控件里的文字，换行统一成 `\n`（系统文本框里存的是 `\r\n`）。
    fn text(&self, id: usize) -> String {
        let text = unsafe {
            let mut buf = vec![0; GetWindowTextLengthW(self.controls[id]) as usize + 1];
            let n = GetWindowTextW(self.controls[id], &mut buf);
            String::from_utf16_lossy(&buf[..n as usize])
        };
        text.replace("\r\n", "\n")
    }
    /// 文字没变就不重设，免得按钮与状态行每次刷新都闪。
    /// 系统多行文本框只认 `\r\n`，网页取来的选区与译文多是 `\n`，不换掉就挤成一段。
    fn set(&self, id: usize, text: &str) {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        if self.text(id) != text {
            let shown = text.replace('\n', "\r\n");
            let _ = unsafe { SetWindowTextW(self.controls[id], &HSTRING::from(shown)) };
        }
    }
    /// 朗读原文、译文各用哪种语言的声音。
    fn speech_languages(&self) -> (&'static str, String) {
        self.model.voices()
    }
    /// 底部小字：本机能朗读哪些语言；当前外语读不了时直接点明。
    fn speech_note(&self) -> String {
        let names = |langs: Vec<&'static str>| {
            langs
                .iter()
                .map(|lang| translation::local_name(lang))
                .collect::<Vec<_>>()
                .join(" · ")
        };
        let code = LANGS[self.model.language].0;
        if !self.speech.readable(code) {
            return tr("speech_missing").replace("{lang}", translation::local_name(code));
        }
        let local = self.speech.local_languages();
        let online = self.speech.online_languages();
        let mut note = if local.is_empty() {
            String::new()
        } else {
            tr("speech_local").replace("{list}", &names(local))
        };
        if !online.is_empty() {
            note.push_str(&tr("speech_online").replace("{list}", &names(online)));
        }
        note
    }
    fn change(&mut self) {
        self.model
            .change(self.text(SOURCE), self.model.language, Instant::now());
        self.render();
    }
    /// 「翻译」按钮与 Ctrl+Enter：内容没变但上次失败（没有译文）时也重发。
    fn translate_now(&mut self) {
        self.change();
        self.model.retry(Instant::now());
        self.model.send();
        self.render();
    }
    fn render(&self) {
        self.set(RESULT, &self.model.output);
        self.set(STATUS, self.model.status);
        self.set(
            LANG,
            &format!("{}  ▾", translation::label(self.model.language)),
        );
        self.set(SPEED, SPEEDS[self.speech.speed(Side::Result)].0);
        self.set(SOURCE_SPEED, SPEEDS[self.speech.speed(Side::Source)].0);
        self.set(LOOP, &self.loop_label());
        self.set(SOURCE_LOOP, &self.loop_label());
        self.set(NOTE, &self.speech_note());
        let (source_voice, result_voice) = self.speech_languages();
        for (id, enabled) in [
            (
                READ_SOURCE,
                !self.model.source.is_empty() && self.speech.readable(source_voice),
            ),
            (CLEAR, !self.model.source.is_empty()),
            (TRANSLATE, !self.model.source.trim().is_empty()),
            (COPY, !self.model.output.is_empty()),
            (SWAP, !self.model.output.is_empty()),
            (
                READ_RESULT,
                !self.model.output.is_empty() && self.speech.readable(&result_voice),
            ),
        ] {
            unsafe {
                let _ = windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow(
                    self.controls[id],
                    enabled,
                );
            }
        }
    }
    /// Server 带进来的选中文字：放进原文框立即翻译，窗口带到前台。
    fn receive(&mut self, text: String) {
        self.hidden = None;
        self.speech.refresh();
        self.set(SOURCE, &text);
        self.translate_now();
        unsafe {
            // 最小化、最大化时先还原，才能按内容定大小
            if IsIconic(self.hwnd).as_bool() || IsZoomed(self.hwnd).as_bool() {
                let _ = ShowWindow(self.hwnd, SW_RESTORE);
            }
        }
        self.fit(true);
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_RESTORE);
            let _ = SetForegroundWindow(self.hwnd);
            let _ = SetFocus(Some(self.controls[SOURCE]));
            // 光标放到原文末尾（EM_SETSEL 起止都设成长度），接着改不会覆盖
            let end = GetWindowTextLengthW(self.controls[SOURCE]).max(0) as usize;
            SendMessageW(
                self.controls[SOURCE],
                0xB1,
                Some(WPARAM(end)),
                Some(LPARAM(end as isize)),
            );
        }
    }
    fn hide(&mut self) {
        self.hidden = Some(Instant::now());
        self.speech.stop();
        let _ = unsafe { ShowWindow(self.hwnd, SW_HIDE) };
    }
    fn command(&mut self, param: usize) {
        let id = param & 0xffff;
        let event = (param >> 16) as u32;
        match id {
            SOURCE if event == EN_CHANGE => self.change(),
            SOURCE | RESULT if matches!(event, EN_SETFOCUS | EN_KILLFOCUS) => unsafe {
                let _ = windows::Win32::Graphics::Gdi::InvalidateRect(Some(self.hwnd), None, true);
            },
            LANG => {
                let labels: Vec<String> = (0..LANGS.len()).map(translation::label).collect();
                let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
                if let Some(i) = self.menu(LANG, &labels, &[self.model.language]) {
                    self.model.change(self.text(SOURCE), i, Instant::now());
                    self.render();
                }
            }
            TRANSLATE => self.translate_now(),
            SWAP if !self.model.output.is_empty() => {
                self.set(SOURCE, &self.model.output);
                self.change();
                self.model.send();
                self.render();
            }
            LOOKUP => self.lookup_menu(),
            LOOP | SOURCE_LOOP => self.loop_menu(self.hwnd, self.controls[id]),
            BACKGROUND => {
                match self.menu(BACKGROUND, &[tr("background"), tr("reset_background")], &[]) {
                    Some(0) => colors::choose(self.hwnd),
                    Some(1) => colors::apply(self.hwnd, windows::Win32::Foundation::COLORREF(0)),
                    _ => {}
                }
            }
            CLEAR => {
                self.set(SOURCE, "");
                self.change();
            }
            COPY => unsafe {
                SendMessageW(
                    self.controls[RESULT],
                    0xB1,
                    Some(WPARAM(0)),
                    Some(LPARAM(-1)),
                );
                SendMessageW(self.controls[RESULT], WM_COPY, None, None);
            },
            READ_SOURCE | READ_RESULT => {
                let (source_voice, result_voice) = self.speech_languages();
                let (side, text, language) = if id == READ_RESULT {
                    (
                        Side::Result,
                        self.model.output.clone(),
                        result_voice.as_str(),
                    )
                } else {
                    (Side::Source, self.model.source.clone(), source_voice)
                };
                self.card_speech = false;
                if self.speech.read(side, &text, language).is_err() {
                    self.set(STATUS, tr("speech_failed"));
                }
            }
            SPEED | SOURCE_SPEED => {
                let side = if id == SPEED {
                    Side::Result
                } else {
                    Side::Source
                };
                // 倍速各档，分隔线，再是朗读来源两项（来源两边共用）
                let mut labels: Vec<&str> = SPEEDS.iter().map(|s| s.0).collect();
                labels.extend(["", tr("source_auto"), tr("source_local")]);
                let source = SPEEDS.len() + 1;
                let checked = [
                    self.speech.speed(side),
                    source + usize::from(self.speech.source == Source::Local),
                ];
                match self.menu(id, &labels, &checked) {
                    Some(index) if index < SPEEDS.len() => self.speech.set_speed(side, index),
                    Some(index) if index == source => self.speech.set_source(Source::Auto),
                    Some(index) if index == source + 1 => {
                        self.speech.stop();
                        self.speech.set_source(Source::Local);
                    }
                    _ => {}
                }
                self.render();
            }
            _ => {}
        }
    }
    fn events(&mut self) {
        while let Some((msg, param)) = EVENTS.with_borrow_mut(VecDeque::pop_front) {
            match msg {
                WM_TIMER => {
                    // 划词翻译开着时常驻，随时弹卡片；卡片开着时也不退
                    if self
                        .hidden
                        .is_some_and(|at| at.elapsed() >= Duration::from_secs(600))
                        && !self.lookup.selection
                        && self.lookup.hover == qingjian_platform::HoverMode::Off
                        && !self.card.visible()
                    {
                        unsafe { PostQuitMessage(0) };
                    } else {
                        if self.card.model.tick(Instant::now()) {
                            self.card.render();
                            self.card_translated();
                        }
                        self.card.watch_outside_click();
                        self.speech.tick();
                        // 原文一变译文就清空，所以译文由空变有就是新结果到了
                        let waiting = self.model.output.is_empty();
                        if self.model.tick(Instant::now()) {
                            self.render();
                            if waiting && !self.model.output.is_empty() {
                                self.fit(false);
                            }
                        }
                    }
                }
                WM_SIZE => self.layout(),
                WM_COMMAND => self.command(param),
                WM_CLOSE => self.hide(),
                VOICE_IN => {
                    if let Some(Outcome::Failed) = self.speech.online_ready() {
                        self.set(STATUS, tr("speech_online_failed"));
                    }
                }
                MM_MCINOTIFY => self.speech.online_notify(param),
                WM_ACTIVATE => {
                    self.speech.refresh();
                    self.render();
                }
                TEXT_IN => {
                    if let Some(text) = incoming::take() {
                        self.receive(text);
                    }
                }
                CARD_IN => {
                    if let Some((x, y, text, hover)) = incoming::take_card() {
                        self.show_card(x, y, text, hover);
                    }
                }
                CARD_COMMAND => self.card_command(param),
                CARD_CLOSE => self.close_card(),
                TOGGLE => unsafe {
                    if param == 1 {
                        self.hide();
                    } else {
                        self.hidden = None;
                        self.speech.refresh();
                        self.render();
                        let _ = ShowWindow(self.hwnd, SW_RESTORE);
                        let _ = SetForegroundWindow(self.hwnd);
                        let _ = SetFocus(Some(self.controls[SOURCE]));
                    }
                },
                _ => {}
            }
        }
    }
}
impl Drop for Window {
    fn drop(&mut self) {
        self.speech.stop();
        unsafe {
            let _ = DestroyWindow(self.hwnd);
            for font in [self.font, self.small] {
                if !font.is_invalid() {
                    let _ = DeleteObject(font.into());
                }
            }
        }
    }
}
