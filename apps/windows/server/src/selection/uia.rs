//! 用系统辅助接口（UI Automation）读前台的选中文字：读屏软件用的那套，不碰剪贴板。
//! Chrome、Edge、Word、记事本等的文本控件与网页文档实现了 TextPattern；焦点在链接、段落这类子元素上时往上找几层。

use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationTextPattern, UIA_TextPatternId,
};

/// 往上找带 TextPattern 的祖先的层数上限。
const MAX_PARENTS: usize = 6;

/// 读当前焦点的选中文字；接口不支持或没选中回 `None`。在调用方开的独立线程上跑（可能被卡住的应用拖住）。
pub(super) fn selected_text(max_chars: usize) -> Option<String> {
    let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if initialized.is_err() {
        return None;
    }
    let text = read(max_chars)
        .inspect_err(|error| tracing::debug!(%error, "UI Automation selection unavailable"))
        .ok()
        .flatten();
    unsafe { CoUninitialize() };
    text
}

fn read(max_chars: usize) -> windows::core::Result<Option<String>> {
    unsafe {
        let automation: IUIAutomation =
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;
        let walker = automation.ControlViewWalker()?;
        let mut element: IUIAutomationElement = automation.GetFocusedElement()?;
        for _ in 0..=MAX_PARENTS {
            // 不支持时 windows 库把「成功但为空」也当错误交回来；Win32 原生 EDIT 就不给 TextPattern
            if let Ok(pattern) =
                element.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
            {
                return Ok(selection_of(&pattern, max_chars));
            }
            match walker.GetParentElement(&element) {
                Ok(parent) => element = parent,
                Err(_) => break,
            }
        }
        Ok(None)
    }
}

fn selection_of(pattern: &IUIAutomationTextPattern, max_chars: usize) -> Option<String> {
    let ranges = unsafe { pattern.GetSelection() }.ok()?;
    let count = unsafe { ranges.Length() }.unwrap_or(0);
    let mut text = String::new();
    for index in 0..count {
        let Ok(range) = (unsafe { ranges.GetElement(index) }) else {
            continue;
        };
        if let Ok(part) = unsafe { range.GetText(max_chars as i32) } {
            text.push_str(&part.to_string());
        }
    }
    let text = text.trim();
    (!text.is_empty()).then(|| text.chars().take(max_chars).collect())
}
