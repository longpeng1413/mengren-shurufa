//! 输入法吃掉带 Alt / Win 的快捷键（Ctrl+Alt+T 等保留键）后补发遮罩键。
//!
//! 主键被输入法吃掉，应用只看到 Alt（或 Win）单独按下又松开：经典菜单栏与 WPS / Office 进菜单访问键模式，
//! 新版记事本这类 WinUI 程序把焦点挪到访问键层，输入法随之收到失焦、把刚开始的翻译评审结束掉；Win 则会弹开始菜单。
//! - 补一个未分配的虚拟键 0xE8（按下 + 抬起）：管得住开始菜单与经典菜单栏；
//! - Alt 按着时再补按一下没按着的那侧 Ctrl：WinUI 不认 0xE8，认 Ctrl（2026-11-20 记事本实测）。
//!   Alt 按着时这些键以系统键消息到达，TSF 不交给按键接收器，评审中也不会被当成按键把评审结束掉；
//!   挑没按着的那侧，用户按着的 Ctrl 状态不受影响，最后也不会留下「Ctrl 一直按着」。
//!
//! 键都带扫描码（Flutter 等按扫描码认键）。Server 全局快捷键的遮罩（`hotkey::send_mask`）是同一做法。

use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, MAPVK_VK_TO_VSC, MapVirtualKeyW, SendInput,
    VIRTUAL_KEY, VK_LCONTROL, VK_LWIN, VK_MENU, VK_RCONTROL, VK_RWIN,
};

/// 未分配的虚拟键，用作遮罩。
const VK_MASK: VIRTUAL_KEY = VIRTUAL_KEY(0xE8);

/// Alt 或 Win 还按着时补发遮罩键；都没按着（Ctrl+Shift 组合等）就什么都不做。
pub(crate) fn mask_menu_activation() {
    let held = |key: VIRTUAL_KEY| unsafe { GetKeyState(i32::from(key.0)) } < 0;
    let alt = held(VK_MENU);
    if !alt && !held(VK_LWIN) && !held(VK_RWIN) {
        return;
    }
    let mut inputs = tap(VK_MASK).to_vec();
    if alt {
        let ctrl = if held(VK_LCONTROL) {
            VK_RCONTROL
        } else {
            VK_LCONTROL
        };
        inputs.extend(tap(ctrl));
    }
    unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
}

/// 按下 + 抬起一个键，带扫描码；右 Ctrl 是扩展键。
fn tap(key: VIRTUAL_KEY) -> [INPUT; 2] {
    let scan = unsafe { MapVirtualKeyW(u32::from(key.0), MAPVK_VK_TO_VSC) } as u16;
    let extended = if key == VK_RCONTROL {
        KEYEVENTF_EXTENDEDKEY
    } else {
        KEYBD_EVENT_FLAGS(0)
    };
    let input = |flags: KEYBD_EVENT_FLAGS| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                wScan: scan,
                dwFlags: flags,
                ..Default::default()
            },
        },
    };
    [input(extended), input(extended | KEYEVENTF_KEYUP)]
}
