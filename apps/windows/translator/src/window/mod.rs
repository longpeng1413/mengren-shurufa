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
const CONTROL_COUNT: usize = BACKGROUND + 1;
thread_local! {static EVENTS:RefCell<VecDeque<(u32,usize)>>=const{RefCell::new(VecDeque::new())};}
unsafe extern "system" fn proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND if colors::erase(hwnd, HDC(w.0 as _)) => LRESULT(1),
        WM_CTLCOLOREDIT | WM_CTLCOLORSTATIC | WM_CTLCOLORLISTBOX => {
            if let Some(brush) = colors::brush(HDC(w.0 as _)) {
                LRESULT(brush.0 as isize)
            } else {
                unsafe { DefWindowProcW(hwnd, msg, w, l) }
            }
        }
        WM_GETMINMAXINFO => {
            let size = unsafe { &mut *(l.0 as *mut MINMAXINFO) };
            let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96) as i32;
            size.ptMinTrackSize.x = 640 * dpi / 96;
            size.ptMinTrackSize.y = 352 * dpi / 96;
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
                WS_OVERLAPPEDWINDOW,
                r.left + (r.right - r.left) / 4,
                r.top + (r.bottom - r.top) / 7,
                660,
                520,
                None,
                None,
                Some(inst.into()),
                None,
            )?;
            let mut controls = [HWND::default(); CONTROL_COUNT];
            for (id, class, label, flags) in [
                (
                    LANG,
                    w!("COMBOBOX"),
                    "",
                    CBS_DROPDOWNLIST as u32 | WS_VSCROLL.0,
                ),
                (SWAP, w!("BUTTON"), "swap", 0),
                (BACKGROUND, w!("BUTTON"), "background", 0),
                (
                    SOURCE,
                    w!("EDIT"),
                    "source",
                    ES_MULTILINE as u32
                        | ES_AUTOVSCROLL as u32
                        | ES_WANTRETURN as u32
                        | WS_VSCROLL.0
                        | WS_BORDER.0,
                ),
                (READ_SOURCE, w!("BUTTON"), "read_source", 0),
                (CLEAR, w!("BUTTON"), "clear", 0),
                (
                    RESULT,
                    w!("EDIT"),
                    "result",
                    ES_MULTILINE as u32 | ES_READONLY as u32 | WS_VSCROLL.0 | WS_BORDER.0,
                ),
                (READ_RESULT, w!("BUTTON"), "read_result", 0),
                (COPY, w!("BUTTON"), "copy", 0),
                (STATUS, w!("STATIC"), "", 0),
                (LABEL, w!("STATIC"), "language", 0),
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
                    WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(flags),
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
            for (_, name, _) in LANGS {
                SendMessageW(
                    controls[LANG],
                    CB_ADDSTRING,
                    None,
                    Some(LPARAM(HSTRING::from(name).as_ptr() as isize)),
                );
            }
            let model = Translation::new(config);
            SendMessageW(
                controls[LANG],
                CB_SETCURSEL,
                Some(WPARAM(model.language)),
                None,
            );
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
                    -px(16),
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
            let area = ((height - 168) / 2).max(24);
            for (i, x, y, w, h) in [
                (LABEL, 12, 16, 166, 26),
                (LANG, 180, 10, 136, 230),
                (SWAP, width - 130, 10, 118, 30),
                (BACKGROUND, 327, 10, 155, 30),
                (SOURCE, 12, 50, width - 24, area),
                (READ_SOURCE, 12, 56 + area, 185, 32),
                (CLEAR, 205, 56 + area, 110, 32),
                (RESULT, 12, 96 + area, width - 24, area),
                (READ_RESULT, 12, 102 + area * 2, 185, 32),
                (COPY, 205, 102 + area * 2, 190, 32),
                (STATUS, 12, 140 + area * 2, width - 24, 28),
            ] {
                let _ = MoveWindow(self.controls[i], px(x), px(y), px(w.max(1)), px(h), true);
            }
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
        let lang =
            unsafe { SendMessageW(self.controls[LANG], CB_GETCURSEL, None, None) }.0 as usize;
        self.model
            .change(self.text(SOURCE), lang.min(7), Instant::now());
        self.render();
    }
    fn render(&self) {
        if self.text(RESULT) != self.model.output {
            self.set(RESULT, &self.model.output);
        }
        self.set(STATUS, self.model.status);
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
            LANG if event == CBN_SELCHANGE => self.change(),
            SWAP if !self.model.output.is_empty() => {
                self.set(SOURCE, &self.model.output);
                self.change();
                self.model.send();
                self.render();
            }
            BACKGROUND => colors::choose(self.hwnd),
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
                win.change();
                win.model.send();
                win.render();
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
