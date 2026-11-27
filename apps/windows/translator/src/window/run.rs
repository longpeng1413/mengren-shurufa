//! 进程入口：读管道里的原文、单实例转交、建窗口、跑消息循环。
//!
//! 启动参数：`--stdin` 从管道读原文；`--card x,y` 把原文弹成划词卡片（不开主窗口）；
//! `--background` 只在后台起好进程（Server 显示「译」图标时预起，点图标时卡片弹得快）。

use std::time::Duration;

use qingjian_platform::{Config, dirs};
use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, LPARAM, WPARAM};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL};
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, DispatchMessageW, FindWindowW, GetForegroundWindow, GetMessageW,
    GetWindowThreadProcessId, IsDialogMessageW, MSG, PostMessageW, TranslateMessage, WM_KEYDOWN,
};
use windows::core::{Result, w};

use super::{CLASS, TOGGLE, Window, incoming};

pub fn run() -> Result<()> {
    // 先读管道再判单实例：Server 写完就关管道，不读它会一直等
    let text = incoming::stdin_text();
    let card = incoming::card_point();
    let background = incoming::background();
    unsafe {
        let mutex = CreateMutexW(None, false, w!("Local\\Mengren.Translator"))?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            for _ in 0..100 {
                if let Ok(hwnd) = FindWindowW(CLASS, None) {
                    let hide = GetForegroundWindow() == hwnd;
                    let mut pid = 0;
                    GetWindowThreadProcessId(hwnd, Some(&mut pid));
                    let _ = AllowSetForegroundWindow(pid);
                    // 带原文时总是显示并翻译；只按快捷键时在前台就收起；后台预起时已经在跑就什么都不做
                    match (&text, card) {
                        (Some(text), Some((x, y, hover))) => {
                            incoming::forward_card(hwnd, x, y, hover, text)
                        }
                        (Some(text), None) => incoming::forward(hwnd, text),
                        // Server 点了图标却没读到文字：卡片上说明
                        (None, Some((x, y, hover))) => {
                            incoming::forward_card(hwnd, x, y, hover, "")
                        }
                        (None, _) if background => {}
                        (None, _) => {
                            PostMessageW(Some(hwnd), TOGGLE, WPARAM(hide as usize), LPARAM(0))?
                        }
                    }
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
        // 只弹卡片或后台预起时不开主窗口
        let visible = card.is_none() && !background;
        let mut win = Window::new(cfg, visible)?;
        // 卡片从这里起地址不变，拖边框时窗口过程直接找它重排
        win.card.bind();
        match (text, card) {
            (Some(text), Some((x, y, hover))) => win.show_card(x, y, text, hover),
            (Some(text), None) => win.receive(text),
            (None, Some((x, y, hover))) => win.show_card(x, y, String::new(), hover),
            (None, None) => {}
        }
        let mut msg = MSG::default();
        loop {
            let rc = GetMessageW(&mut msg, None, 0, 0).0;
            if rc <= 0 {
                break;
            }
            if msg.message == WM_KEYDOWN && msg.wParam.0 == 27 {
                if win.card.owns(msg.hwnd) {
                    win.close_card();
                } else {
                    win.hide();
                }
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
