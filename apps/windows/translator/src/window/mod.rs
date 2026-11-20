//! 原生多行编辑框保留 TSF 输入。窗口过程只排队，控件通知不会重入界面状态。
mod colors;
mod speech;
mod translation;
use qingjian_platform::{Config, dirs, windows_text::tr};
use speech::Speech;
use std::{
    cell::RefCell,
    collections::VecDeque,
    time::{Duration, Instant},
};
use translation::{LANGS, Translation};
use windows::Win32::{
    Foundation::{
        CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HWND, LPARAM, LRESULT, RECT, WPARAM,
    },
    Graphics::Gdi::{
        CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, COLOR_WINDOW, CreateFontW, DEFAULT_CHARSET,
        DeleteObject, HDC, HFONT, OUT_DEFAULT_PRECIS, VARIABLE_PITCH,
    },
    System::{
        Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize},
        LibraryLoader::GetModuleHandleW,
        Threading::CreateMutexW,
    },
    UI::{
        HiDpi::GetDpiForWindow,
        Input::KeyboardAndMouse::{GetKeyState, SetFocus, VK_CONTROL},
        WindowsAndMessaging::*,
    },
};
use windows::core::{HSTRING, PCWSTR, Result, w};
const CLASS: PCWSTR = w!("Mengren.Translator.Window");
const TOGGLE: u32 = WM_APP + 1;
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
const CONTROL_COUNT: usize = RESULT_LABEL + 1;
thread_local! {static EVENTS:RefCell<VecDeque<(u32,usize)>>=const{RefCell::new(VecDeque::new())};}
unsafe extern "system" fn proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match msg {
        WM_DRAWITEM if l.0 != 0 => {
            colors::draw_button(unsafe {
                &*(l.0 as *const windows::Win32::UI::Controls::DRAWITEMSTRUCT)
            });
            LRESULT(1)
        }
        WM_ERASEBKGND if colors::erase(hwnd, HDC(w.0 as _)) => LRESULT(1),
        WM_CTLCOLOREDIT | WM_CTLCOLORSTATIC | WM_CTLCOLORLISTBOX => {
            let id = unsafe { GetDlgCtrlID(HWND(l.0 as _)) } as usize;
            if let Some(brush) = colors::brush(HDC(w.0 as _), matches!(id, SOURCE | RESULT)) {
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
        WM_CLOSE | WM_SIZE | WM_TIMER | WM_COMMAND | TOGGLE => {
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
    dpi: u32,
    model: Translation,
    speech: Speech,
    hidden: Option<Instant>,
}
impl Window {
    fn new(config: Config) -> Result<Self> {
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
                660,
                615,
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
                    660 * dpi / 96,
                    615 * dpi / 96,
                    SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            colors::caption(hwnd);
            let mut controls = [HWND::default(); CONTROL_COUNT];
            for (id, class, label, flags) in [
                (LANG, w!("BUTTON"), "", BS_OWNERDRAW as u32),
                (SWAP, w!("BUTTON"), "swap", BS_OWNERDRAW as u32),
                (BACKGROUND, w!("BUTTON"), "background", BS_OWNERDRAW as u32),
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
                (COPY, w!("BUTTON"), "copy", BS_OWNERDRAW as u32),
                (STATUS, w!("STATIC"), "", 0),
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
                        | (if matches!(id, STATUS | LABEL | SOURCE_LABEL | RESULT_LABEL) {
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
            let model = Translation::new(config);
            let mut this = Self {
                hwnd,
                controls,
                font: HFONT::default(),
                dpi: 0,
                model,
                speech: Speech::default(),
                hidden: None,
            };
            this.layout();
            this.render();
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetFocus(Some(this.controls[SOURCE]));
            SetTimer(Some(hwnd), 1, 100, None);
            Ok(this)
        }
    }
    fn layout(&mut self) {
        unsafe {
            let dpi = GetDpiForWindow(self.hwnd).max(96);
            let px = |v: i32| v * dpi as i32 / 96;
            if self.dpi != dpi {
                let old = self.font;
                self.font = CreateFontW(
                    -px(14),
                    0,
                    0,
                    0,
                    400,
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
                self.dpi = dpi;
                for h in &self.controls[1..] {
                    SendMessageW(
                        *h,
                        WM_SETFONT,
                        Some(WPARAM(self.font.0 as usize)),
                        Some(LPARAM(1)),
                    );
                }
                if !old.is_invalid() {
                    let _ = DeleteObject(old.into());
                }
            }
            let mut r = RECT::default();
            let _ = GetClientRect(self.hwnd, &mut r);
            let width = r.right * 96 / dpi as i32;
            let height = r.bottom * 96 / dpi as i32;
            let area = ((height - 250) / 2).max(40);
            let left = 20;
            let end = width - left;
            let row = 96 + area;
            let footer = row + area + 76;
            for (i, x, y, w, h) in [
                (LABEL, left, 25, 136, 22),
                (LANG, 154, 17, 150, 34),
                (SWAP, 314, 17, 102, 34),
                (BACKGROUND, end - 150, 17, 150, 34),
                (SOURCE_LABEL, left, 63, end - left, 20),
                (SOURCE, left + 10, 96, end - left - 20, area - 20),
                (READ_SOURCE, left, row, 150, 34),
                (CLEAR, left + 158, row, 111, 34),
                (TRANSLATE, end - 132, row, 132, 34),
                (RESULT_LABEL, left, row + 47, end - left, 20),
                (RESULT, left + 10, row + 79, end - left - 20, area - 20),
                (READ_RESULT, left, footer, 150, 34),
                (COPY, end - 155, footer, 155, 34),
                (STATUS, left, footer + 40, end - left, 24),
            ] {
                let _ = MoveWindow(self.controls[i], px(x), px(y), px(w.max(1)), px(h), true);
            }
            let _ = windows::Win32::Graphics::Gdi::InvalidateRect(Some(self.hwnd), None, true);
        }
    }

    fn menu(&self, id: usize, labels: &[&str], checked: Option<usize>) -> Option<usize> {
        unsafe {
            let mut r = RECT::default();
            GetWindowRect(self.controls[id], &mut r).ok()?;
            let menu = CreatePopupMenu().ok()?;
            let selection = (|| {
                for (index, label) in labels.iter().enumerate() {
                    let flags = if Some(index) == checked {
                        MF_CHECKED
                    } else {
                        MF_STRING
                    };
                    AppendMenuW(menu, flags, index + 1, &HSTRING::from(*label)).ok()?;
                }
                let chosen = TrackPopupMenu(
                    menu,
                    TPM_RETURNCMD | TPM_NONOTIFY,
                    r.left,
                    r.bottom,
                    None,
                    self.hwnd,
                    None,
                )
                .0;
                (chosen > 0)
                    .then_some(chosen as usize)
                    .and_then(|n| n.checked_sub(1))
            })();
            let _ = DestroyMenu(menu);
            selection
        }
    }
    fn text(&self, id: usize) -> String {
        unsafe {
            let mut buf = vec![0; GetWindowTextLengthW(self.controls[id]) as usize + 1];
            let n = GetWindowTextW(self.controls[id], &mut buf);
            String::from_utf16_lossy(&buf[..n as usize])
        }
    }
    fn set(&self, id: usize, text: &str) {
        let _ = unsafe { SetWindowTextW(self.controls[id], &HSTRING::from(text)) };
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
        if self.text(RESULT) != self.model.output {
            self.set(RESULT, &self.model.output);
        }
        self.set(STATUS, self.model.status);
        self.set(LANG, &format!("{}  ▾", LANGS[self.model.language].1));
        for (id, enabled) in [
            (READ_SOURCE, !self.model.source.is_empty()),
            (CLEAR, !self.model.source.is_empty()),
            (TRANSLATE, !self.model.source.trim().is_empty()),
            (COPY, !self.model.output.is_empty()),
            (SWAP, !self.model.output.is_empty()),
            (READ_RESULT, !self.model.output.is_empty()),
        ] {
            unsafe {
                let _ = windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow(
                    self.controls[id],
                    enabled,
                );
            }
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
                if let Some(i) = self.menu(LANG, &LANGS.map(|l| l.1), Some(self.model.language)) {
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
            BACKGROUND => match self.menu(
                BACKGROUND,
                &[tr("background"), tr("reset_background")],
                None,
            ) {
                Some(0) => colors::choose(self.hwnd),
                Some(1) => colors::apply(self.hwnd, windows::Win32::Foundation::COLORREF(0)),
                _ => {}
            },
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
                let foreign = LANGS[self.model.language].0;
                let dest = translation::target(&self.model.source, foreign);
                let (text, language) = if id == READ_RESULT {
                    (&self.model.output, dest.as_str())
                } else {
                    (
                        &self.model.source,
                        if dest == "zh" { foreign } else { "zh" },
                    )
                };
                if self.speech.read(id, text, language).is_err() {
                    self.set(STATUS, tr("speech_failed"));
                }
            }
            _ => {}
        }
    }
    fn events(&mut self) {
        while let Some((msg, param)) = EVENTS.with_borrow_mut(VecDeque::pop_front) {
            match msg {
                WM_TIMER => {
                    if self
                        .hidden
                        .is_some_and(|at| at.elapsed() >= Duration::from_secs(600))
                    {
                        unsafe { PostQuitMessage(0) };
                    } else if self.model.tick(Instant::now()) {
                        self.render();
                    }
                }
                WM_SIZE => self.layout(),
                WM_COMMAND => self.command(param),
                WM_CLOSE => self.hide(),
                TOGGLE => unsafe {
                    if param == 1 {
                        self.hide();
                    } else {
                        self.hidden = None;
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
            if !self.font.is_invalid() {
                let _ = DeleteObject(self.font.into());
            }
        }
    }
}
pub fn run() -> Result<()> {
    unsafe {
        let mutex = CreateMutexW(None, false, w!("Local\\Mengren.Translator"))?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            for _ in 0..100 {
                if let Ok(hwnd) = FindWindowW(CLASS, None) {
                    let hide = GetForegroundWindow() == hwnd;
                    let mut pid = 0;
                    GetWindowThreadProcessId(hwnd, Some(&mut pid));
                    let _ = AllowSetForegroundWindow(pid);
                    PostMessageW(Some(hwnd), TOGGLE, WPARAM(hide as usize), LPARAM(0))?;
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            CloseHandle(mutex)?;
            return Ok(());
        }
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        if let Some(dir) = dirs::user_dir() {
            let _ = dotenvy::from_path(dir.join(".env"));
        }
        let _guard = dirs::log_dir().and_then(|dir| {
            let log = tracing_appender::rolling::RollingFileAppender::builder()
                .rotation(tracing_appender::rolling::Rotation::DAILY)
                .filename_prefix("translator")
                .max_log_files(7)
                .build(dir)
                .ok()?;
            let (writer, guard) = tracing_appender::non_blocking(
                qingjian_platform::logs::secrets::MaskingWriter::new(log),
            );
            let _ = tracing_subscriber::fmt()
                .with_writer(writer)
                .with_ansi(false)
                .try_init();
            Some(guard)
        });
        qingjian_platform::member::install();
        let cfg = dirs::config_path()
            .and_then(|p| Config::load(&p).ok())
            .unwrap_or_default();
        bamboo_slip_i18n::set_locale(cfg.general.ui_locale());
        let mut win = Window::new(cfg)?;
        let mut msg = MSG::default();
        loop {
            let rc = GetMessageW(&mut msg, None, 0, 0).0;
            if rc <= 0 {
                break;
            }
            if msg.message == WM_KEYDOWN && msg.wParam.0 == 27 {
                win.hide();
                continue;
            }
            if msg.message == WM_KEYDOWN
                && msg.wParam.0 == 13
                && GetKeyState(VK_CONTROL.0 as i32) < 0
            {
                win.translate_now();
                continue;
            }
            if !IsDialogMessageW(win.hwnd, &msg).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            win.events();
        }
        drop(win);
        CoUninitialize();
        CloseHandle(mutex)?;
        Ok(())
    }
}
