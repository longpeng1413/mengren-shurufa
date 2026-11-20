//! 输入法读不到选区时（网页正文这类不可编辑内容、非猛人会话、旧 DLL），翻译窗口快捷键改用的两种取法：
//! 先用系统辅助接口（[`uia`]，不碰剪贴板），取不到再模拟 Ctrl+Insert（Flutter 等不认时 Ctrl+C，终端除外）从剪贴板取（[`copy`]）。
//! 都在独立线程上跑，辅助接口限时，免得被卡住的应用拖住。

mod copy;
mod target;
mod uia;

use std::sync::mpsc;
use std::time::Duration;

use windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetForegroundWindow};

pub use self::target::caret_rect;

/// 带进翻译窗口的字数上限，与 DLL 读选区一致。
const MAX_CHARS: usize = 4000;

/// 辅助接口的等待上限。
const UIA_TIMEOUT: Duration = Duration::from_millis(800);

/// 翻译窗口的窗口类名（`apps/windows/translator`）。
const TRANSLATOR_CLASS: &str = "Mengren.Translator.Window";

/// 前台是不是翻译窗口自己：是就只收起，不去读。
pub fn translator_in_front() -> bool {
    let window = unsafe { GetForegroundWindow() };
    if window.is_invalid() {
        return false;
    }
    let mut name = [0u16; 64];
    let len = unsafe { GetClassNameW(window, &mut name) };
    usize::try_from(len).is_ok_and(|len| String::from_utf16_lossy(&name[..len]) == TRANSLATOR_CLASS)
}

/// 取前台的选中文字：辅助接口 → 模拟复制。取不到回 `None`。会阻塞，在后台线程调。
pub fn read() -> Option<String> {
    let (sender, receiver) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("qingjian-selection-uia".to_owned())
        .spawn(move || {
            let _ = sender.send(uia::selected_text(MAX_CHARS));
        });
    let from_uia = spawned
        .ok()
        .and_then(|_| receiver.recv_timeout(UIA_TIMEOUT).ok())
        .flatten();
    if let Some(text) = from_uia {
        tracing::info!(
            chars = text.chars().count(),
            "selection read through UI Automation"
        );
        return Some(text);
    }
    let text = copy::copied_text(MAX_CHARS);
    tracing::info!(
        chars = text.as_ref().map_or(0, |text| text.chars().count()),
        "selection read through Ctrl+Insert"
    );
    text
}
