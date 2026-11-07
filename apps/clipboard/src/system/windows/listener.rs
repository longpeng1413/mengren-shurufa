//! 消息窗口线程：`AddClipboardFormatListener` 收复制，`RegisterHotKey` 收呼出快捷键，读好内容后送给面板；
//! 定时看配置文件变了没有，快捷键换了就重新注册。

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};
use std::sync::mpsc::Sender;
use std::time::{Duration, SystemTime};

use bamboo_slip::Waker;
use qingjian_clipboard::{ClipSettings, Hotkey, WorkArea};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromPoint,
};
use windows::Win32::System::DataExchange::AddClipboardFormatListener;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, MOD_NOREPEAT, RegisterHotKey, UnregisterHotKey,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GW_HWNDNEXT, GWL_EXSTYLE, GetCursorPos,
    GetForegroundWindow, GetMessageW, GetTopWindow, GetWindow, GetWindowLongPtrW,
    GetWindowThreadProcessId, HWND_MESSAGE, IsWindowVisible, MSG, RegisterClassW,
    RegisterWindowMessageW, SetTimer, TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE,
    WM_CLIPBOARDUPDATE, WM_HOTKEY, WM_LBUTTONUP, WM_RBUTTONUP, WM_SETTINGCHANGE, WM_TIMER,
    WNDCLASSW, WS_EX_TOOLWINDOW, WS_POPUP,
};
use windows::core::{PCWSTR, w};

use super::clipboard::{read_capture, set_owner};
use super::tray::{self, Choice};
use crate::system::Signal;

/// 窗口过程拿不到闭包，发送端放这里；进程里只有一个监听线程。
static OUTBOX: OnceLock<(Sender<Signal>, Waker)> = OnceLock::new();

const HOTKEY_ID: i32 = 1;

const PRIVATE_HOTKEY_ID: i32 = 2;

const SEND_HOTKEY_ID: i32 = 3;

const WATCH_TIMER: usize = 1;

/// 托盘图标挂的窗口。
static TRAY_WINDOW: AtomicIsize = AtomicIsize::new(0);

/// 资源管理器重启时广播的消息，托盘图标要重新加。
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

/// 多久看一次配置文件。
const WATCH_INTERVAL: Duration = Duration::from_secs(1);

/// 配置文件与它现在的样子。
pub struct Watch {
    /// `None` 时不看（命令行指定了数据、没有配置文件这类），一直用 `settings`。
    pub path: Option<PathBuf>,

    pub settings: ClipSettings,

    /// 由输入法 Server 拉起的：配置里关掉剪贴板就退出。
    pub managed: bool,
}

thread_local! {
    /// 窗口过程拿不到参数，监听线程自己的状态放这里。
    static WATCH: RefCell<Option<(Watch, Option<SystemTime>)>> = const { RefCell::new(None) };
}

pub fn spawn_listener(watch: Watch, sender: Sender<Signal>, waker: Waker) {
    if OUTBOX.set((sender, waker)).is_err() {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("clipboard-listener".to_owned())
        .spawn(move || {
            if let Err(error) = run(watch) {
                tracing::error!(%error, "clipboard listener failed");
                eprintln!("clipboard listener failed: {error}");
                std::process::exit(1);
            }
        });
    if let Err(error) = spawned {
        tracing::error!(%error, "cannot start clipboard listener");
    }
}

fn modified(path: Option<&PathBuf>) -> Option<SystemTime> {
    std::fs::metadata(path?)
        .and_then(|meta| meta.modified())
        .ok()
}

fn binding(hotkey: Hotkey) -> Option<(HOT_KEY_MODIFIERS, u32)> {
    qingjian_platform::global_hotkey::binding(
        hotkey.key,
        hotkey.control,
        hotkey.shift,
        hotkey.alt,
        hotkey.win,
    )
}

fn equivalent(first: Hotkey, second: Hotkey) -> bool {
    binding(first).is_some_and(|first| Some(first) == binding(second))
}

fn virtual_key(hotkey: Hotkey) -> u32 {
    binding(hotkey).map_or(0, |(_, key)| key)
}

fn register_id(window: HWND, hotkey: Hotkey, id: i32) {
    let Some((modifiers, key)) = binding(hotkey) else {
        notice("private.hotkey_taken");
        return;
    };
    if let Err(error) = unsafe { RegisterHotKey(Some(window), id, modifiers | MOD_NOREPEAT, key) } {
        tracing::warn!(%error, ?hotkey, "clipboard hotkey is taken");
        notice("private.hotkey_taken");
    }
}

fn register_all(window: HWND, settings: &ClipSettings) {
    for id in [HOTKEY_ID, PRIVATE_HOTKEY_ID, SEND_HOTKEY_ID] {
        let _ = unsafe { UnregisterHotKey(Some(window), id) };
    }
    register_id(window, settings.hotkey, HOTKEY_ID);
    if !equivalent(settings.private_hotkey, settings.hotkey) {
        register_id(window, settings.private_hotkey, PRIVATE_HOTKEY_ID);
    }
    if settings.sync_enabled
        && settings.private_send_mode != "off"
        && !equivalent(settings.private_send_hotkey, settings.hotkey)
        && !equivalent(settings.private_send_hotkey, settings.private_hotkey)
    {
        register_id(window, settings.private_send_hotkey, SEND_HOTKEY_ID);
    } else if settings.sync_enabled && settings.private_send_mode != "off" {
        notice("private.hotkey_taken");
    }
}

fn send_private(manual: bool) {
    let mode = WATCH.with_borrow(|state| {
        state.as_ref().map_or_else(
            || "off".to_owned(),
            |(watch, _)| {
                if watch.settings.sync_enabled && (manual || !watch.settings.paused) {
                    watch.settings.private_send_mode.clone()
                } else {
                    "off".into()
                }
            },
        )
    });
    if mode == "off" {
        if manual {
            notice("private.send_disabled");
        }
        return;
    }
    if manual || mode == "auto" {
        super::private::send_current(!manual);
    }
}

/// 配置文件变了就读：关了退出，快捷键换了重新注册，别的交给面板。
fn check_config(window: HWND) {
    let changed = WATCH.with_borrow_mut(|state| {
        let (watch, stamp) = state.as_mut()?;
        let now = modified(watch.path.as_ref());
        if now == *stamp {
            return None;
        }
        // 读成功才记下时刻：别的程序正写着时读不了、读到一半，下一秒要再读，不能当成看过了。
        // 写到一半的空文件也一样，按缺省算就成了「没开启」，进程会自己退出
        let text = std::fs::read_to_string(watch.path.as_ref()?).ok()?;
        if text.trim().is_empty() {
            return None;
        }
        let next = ClipSettings::parse(&text)?;
        *stamp = now;
        if watch.managed && !next.enabled {
            quit_now();
        }
        let previous = std::mem::replace(&mut watch.settings, next.clone());
        Some((previous, next.clone()))
    });
    let Some((previous, next)) = changed else {
        return;
    };
    if previous.hotkey != next.hotkey
        || previous.private_send_mode != next.private_send_mode
        || previous.sync_enabled != next.sync_enabled
        || previous.private_hotkey != next.private_hotkey
        || previous.private_send_hotkey != next.private_send_hotkey
    {
        register_all(window, &next);
    }
    send(Signal::Settings(next));
}

/// 收掉托盘图标再退出，不然图标要等鼠标划过才消失。
fn quit_now() -> ! {
    super::private::lock();
    let icon = HWND(TRAY_WINDOW.load(Ordering::Relaxed) as *mut _);
    if !icon.is_invalid() {
        tray::remove(icon);
    }
    std::process::exit(0);
}

/// 从托盘打开面板：点托盘时前台已是任务栏，要粘贴回去的是它下面第一个真正的窗口。
fn window_under_taskbar() -> isize {
    let own = std::process::id();
    let mut current = unsafe { GetTopWindow(None) }.ok();
    while let Some(window) = current {
        let mut process = 0;
        unsafe { GetWindowThreadProcessId(window, Some(&mut process)) };
        let style = unsafe { GetWindowLongPtrW(window, GWL_EXSTYLE) } as u32;
        if process != own
            && unsafe { IsWindowVisible(window) }.as_bool()
            && style & WS_EX_TOOLWINDOW.0 == 0
        {
            return window.0 as isize;
        }
        current = unsafe { GetWindow(window, GW_HWNDNEXT) }.ok();
    }
    0
}

fn open_from_tray() {
    let (area, scale) = cursor_monitor();
    send(Signal::Hotkey {
        target: window_under_taskbar(),
        area,
        scale,
        held_key: None,
    });
}

/// 托盘菜单改配置：写完马上读回来，菜单下次打开就是新状态。
fn write_flag(window: HWND, key: &str, value: bool) {
    let path = WATCH.with_borrow(|state| state.as_ref().and_then(|(watch, _)| watch.path.clone()));
    let Some(path) = path else {
        return;
    };
    match ClipSettings::write_flag(&path, key, value) {
        Ok(()) => check_config(window),
        Err(error) => tracing::warn!(%error, key, "cannot write clipboard setting"),
    }
}

fn settings_program() -> Option<PathBuf> {
    let program = std::env::current_exe()
        .ok()?
        .with_file_name("qingjian-settings.exe");
    program.is_file().then_some(program)
}

fn tray_menu(window: HWND) {
    let (paused, managed) = WATCH.with_borrow(|state| {
        state.as_ref().map_or((false, false), |(watch, _)| {
            (watch.settings.paused, watch.managed)
        })
    });
    let state = tray::MenuState {
        paused,
        has_settings: settings_program().is_some(),
        standalone: !managed,
    };
    match tray::show_menu(window, &state) {
        Some(Choice::Open) => open_from_tray(),
        Some(Choice::PrivateSend) => send_private(true),
        Some(Choice::PrivateInput) => {
            let target = window_under_taskbar();
            super::paste::refocus(target);
            super::private::input_target(target);
        }
        Some(Choice::TogglePause) => write_flag(window, "paused", !paused),
        Some(Choice::OpenSettings) => {
            if let Some(program) = settings_program()
                && let Err(error) = std::process::Command::new(program).spawn()
            {
                tracing::warn!(%error, "cannot open settings");
            }
        }
        Some(Choice::ToggleAutostart) => tray::set_autostart(!tray::autostart_enabled()),
        Some(Choice::Quit) => {
            // 由输入法拉起的：退出就是关掉，不然下次开机又起来，设置里的开关也对不上
            if managed {
                turn_off();
            }
            quit_now();
        }
        None => {}
    }
}

fn turn_off() {
    let path = WATCH.with_borrow(|state| state.as_ref().and_then(|(watch, _)| watch.path.clone()));
    if let Some(path) = path
        && let Err(error) = ClipSettings::write_flag(&path, "enabled", false)
    {
        tracing::warn!(%error, "cannot turn the clipboard off");
    }
}

fn run(watch: Watch) -> windows::core::Result<()> {
    unsafe {
        let instance = GetModuleHandleW(None)?;
        let class = w!("QingjianClipboardListener");
        let description = WNDCLASSW {
            lpfnWndProc: Some(procedure),
            hInstance: instance.into(),
            lpszClassName: class,
            ..WNDCLASSW::default()
        };
        RegisterClassW(&description);
        let window = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            PCWSTR::null(),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(instance.into()),
            None,
        )?;
        // 托盘要真窗口：纯消息窗口弹不了菜单。不显示
        let icon_window = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            PCWSTR::null(),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance.into()),
            None,
        )?;
        TRAY_WINDOW.store(icon_window.0 as isize, Ordering::Relaxed);
        use windows::Win32::System::RemoteDesktop::{
            NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification,
        };
        if WTSRegisterSessionNotification(icon_window, NOTIFY_FOR_THIS_SESSION).is_err() {
            super::private::lock();
            tracing::warn!("Private input disabled: session notifications unavailable");
        } else if super::private::session_unlocked() {
            super::private::unlock();
        } else {
            super::private::lock();
        }
        TASKBAR_CREATED.store(
            RegisterWindowMessageW(w!("TaskbarCreated")),
            Ordering::Relaxed,
        );
        tray::add(icon_window);
        set_owner(window);
        AddClipboardFormatListener(window)?;
        register_all(window, &watch.settings);
        if watch.path.is_some() {
            SetTimer(
                Some(window),
                WATCH_TIMER,
                WATCH_INTERVAL.as_millis() as u32,
                None,
            );
        }
        let stamp = modified(watch.path.as_ref());
        WATCH.set(Some((watch, stamp)));
        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    Ok(())
}

unsafe extern "system" fn procedure(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message != 0 && message == TASKBAR_CREATED.load(Ordering::Relaxed) {
        tray::add(window);
        return LRESULT(0);
    }
    match message {
        tray::CALLBACK => {
            match lparam.0 as u32 {
                WM_LBUTTONUP => open_from_tray(),
                WM_RBUTTONUP => tray_menu(window),
                _ => {}
            }
            LRESULT(0)
        }
        WM_CLIPBOARDUPDATE => {
            send_private(false);
            let capture = WATCH.with_borrow(|state| {
                read_capture(|source| {
                    state
                        .as_ref()
                        .is_none_or(|(watch, _)| watch.settings.records(source))
                })
            });
            if let Some(capture) = capture {
                send(Signal::Copied(capture));
            }
            LRESULT(0)
        }
        // 消息窗口收不到广播，这条只有托盘窗口会收到
        WM_SETTINGCHANGE if changed_color_mode(lparam) => {
            tray::refresh(window);
            LRESULT(0)
        }
        WM_TIMER => {
            check_config(window);
            LRESULT(0)
        }
        WM_HOTKEY => {
            let shared = WATCH.with_borrow(|state| {
                state.as_ref().is_some_and(|(watch, _)| {
                    equivalent(watch.settings.private_hotkey, watch.settings.hotkey)
                })
            });
            if wparam.0 == SEND_HOTKEY_ID as usize {
                send_private(true);
            } else if wparam.0 == PRIVATE_HOTKEY_ID as usize {
                super::private::input_current();
            } else if !shared || !super::private::input() {
                send(hotkey_signal());
            }
            LRESULT(0)
        }
        0x02B1 => {
            match wparam.0 {
                2 | 4 | 6 | 7 => super::private::lock(),
                8 => super::private::unlock(),
                _ => {}
            }
            LRESULT(0)
        }
        0x0218 => {
            match wparam.0 {
                4 => super::private::suspend(),
                7 | 18 => {
                    if !super::private::session_unlocked() {
                        super::private::lock();
                    }
                    super::private::resume();
                }
                _ => {}
            }
            LRESULT(1)
        }
        0x0011 | 0x0016 => {
            super::private::lock();
            LRESULT(1)
        }
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

/// 系统切换深浅色时广播的设置名。
fn changed_color_mode(lparam: LPARAM) -> bool {
    if lparam.0 == 0 {
        return false;
    }
    // SAFETY: 这条消息的 lParam 不为空时是系统给的、以 0 结尾的 UTF-16 字符串。
    let name = unsafe { PCWSTR(lparam.0 as *const u16).to_string() };
    name.is_ok_and(|name| name == "ImmersiveColorSet")
}

fn send(signal: Signal) {
    if let Some((sender, waker)) = OUTBOX.get()
        && sender.send(signal).is_ok()
    {
        waker.wake();
    }
}

pub(crate) fn notice(key: &'static str) {
    notice_text(crate::i18n::tr(key));
}

pub(crate) fn notice_text(message: &str) {
    let window = HWND(TRAY_WINDOW.load(Ordering::Relaxed) as *mut _);
    if !window.is_invalid() {
        tray::notice(window, message);
    }
}

fn hotkey_signal() -> Signal {
    let target = unsafe { GetForegroundWindow() }.0 as isize;
    let (area, scale) = cursor_monitor();
    let held_key = WATCH.with_borrow(|state| {
        state
            .as_ref()
            .map(|(watch, _)| virtual_key(watch.settings.hotkey))
    });
    Signal::Hotkey {
        target,
        area,
        scale,
        held_key,
    }
}

/// 光标所在显示器的工作区与倍数；取不到时按 1920×1080、100% 兜底。
fn cursor_monitor() -> (WorkArea, f32) {
    let mut area = WorkArea {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1080,
    };
    let mut scale = 1.0;
    unsafe {
        let mut cursor = POINT::default();
        let _ = GetCursorPos(&mut cursor);
        let monitor = MonitorFromPoint(cursor, MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..MONITORINFO::default()
        };
        if GetMonitorInfoW(monitor, &mut info).as_bool() {
            area = WorkArea {
                left: info.rcWork.left,
                top: info.rcWork.top,
                right: info.rcWork.right,
                bottom: info.rcWork.bottom,
            };
        }
        let (mut dpi_x, mut dpi_y) = (96, 96);
        if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y).is_ok() {
            scale = dpi_x as f32 / 96.0;
        }
    }
    (area, scale)
}
