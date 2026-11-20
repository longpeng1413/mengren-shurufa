//! 取选中文字时看一眼前台：光标在哪（候选窗摆在它下面），前台是不是终端（不能发 Ctrl+C）。

use qingjian_platform::protocol::ScreenRect;
use windows::Win32::Foundation::{CloseHandle, HWND, POINT};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GUITHREADINFO, GetClassNameW, GetCursorPos, GetForegroundWindow, GetGUIThreadInfo,
    GetWindowThreadProcessId,
};
use windows::core::PWSTR;

/// 终端与带内置终端的编辑器：这些程序里 Ctrl+C 是中断正在跑的程序（或远端命令），不能拿来复制。
const TERMINAL_PROCESSES: [&str; 22] = [
    "windowsterminal.exe",
    "openconsole.exe",
    "conhost.exe",
    "cmd.exe",
    "powershell.exe",
    "pwsh.exe",
    "powershell_ise.exe",
    "mintty.exe",
    "bash.exe",
    "wsl.exe",
    "putty.exe",
    "kitty.exe",
    "finalshell.exe",
    "mobaxterm.exe",
    "xshell.exe",
    "securecrt.exe",
    "termius.exe",
    "tabby.exe",
    "wezterm-gui.exe",
    "alacritty.exe",
    "code.exe",
    "cursor.exe",
];

/// 终端窗口类名（进程名认不出时的补充）。
const TERMINAL_CLASSES: [&str; 3] = [
    "ConsoleWindowClass",
    "CASCADIA_HOSTING_WINDOW_CLASS",
    "PseudoConsoleWindow",
];

/// 前台程序的光标位置（屏幕坐标）；程序不用系统光标（浏览器、Flutter 等）时退到鼠标处。
pub fn caret_rect() -> Option<ScreenRect> {
    let mut info = GUITHREADINFO {
        cbSize: size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    if unsafe { GetGUIThreadInfo(0, &mut info) }.is_ok()
        && !info.hwndCaret.is_invalid()
        && (info.rcCaret.bottom > info.rcCaret.top)
    {
        let mut top_left = POINT {
            x: info.rcCaret.left,
            y: info.rcCaret.top,
        };
        let mut bottom_right = POINT {
            x: info.rcCaret.right,
            y: info.rcCaret.bottom,
        };
        let converted = unsafe {
            ClientToScreen(info.hwndCaret, &mut top_left).as_bool()
                && ClientToScreen(info.hwndCaret, &mut bottom_right).as_bool()
        };
        if converted {
            return Some(ScreenRect {
                left: top_left.x,
                top: top_left.y,
                right: bottom_right.x,
                bottom: bottom_right.y,
            });
        }
    }
    let mut cursor = POINT::default();
    unsafe { GetCursorPos(&mut cursor) }.ok()?;
    Some(ScreenRect {
        left: cursor.x,
        top: cursor.y,
        right: cursor.x,
        bottom: cursor.y + 20,
    })
}

/// 前台是不是终端（或带内置终端的编辑器）。
pub fn terminal_in_front() -> bool {
    let window = unsafe { GetForegroundWindow() };
    if window.is_invalid() {
        return false;
    }
    if TERMINAL_CLASSES.contains(&class_name(window).as_str()) {
        return true;
    }
    process_name(window).is_some_and(|name| TERMINAL_PROCESSES.contains(&name.as_str()))
}

fn class_name(window: HWND) -> String {
    let mut name = [0u16; 128];
    let len = unsafe { GetClassNameW(window, &mut name) };
    usize::try_from(len).map_or_else(
        |_| String::new(),
        |len| String::from_utf16_lossy(&name[..len]),
    )
}

/// 窗口所属进程的 exe 名（小写）。
fn process_name(window: HWND) -> Option<String> {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(window, Some(&mut pid)) };
    if pid == 0 {
        return None;
    }
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let mut buffer = [0u16; 512];
    let mut size = buffer.len() as u32;
    let queried = unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut size,
        )
    };
    let _ = unsafe { CloseHandle(process) };
    queried.ok()?;
    let path = String::from_utf16_lossy(&buffer[..size as usize]);
    path.rsplit(['\\', '/']).next().map(str::to_ascii_lowercase)
}

#[cfg(test)]
mod tests {
    use super::TERMINAL_PROCESSES;

    #[test]
    fn terminal_names_are_lowercase_exe_names() {
        for name in TERMINAL_PROCESSES {
            assert_eq!(name, name.to_ascii_lowercase());
            assert!(name.ends_with(".exe"), "{name}");
        }
    }
}
