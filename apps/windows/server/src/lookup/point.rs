//! 鼠标下的词：先问系统辅助接口（网页、Word、记事本这类给出文字范围的程序），
//! 程序不给文字范围时截图识字（[`super::ocr`]）。在独立的工作线程上调，可能被卡住的程序拖住。

use windows::Win32::Foundation::{POINT, RECT};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::Win32::System::Ole::{
    SafeArrayAccessData, SafeArrayDestroy, SafeArrayGetLBound, SafeArrayGetUBound,
    SafeArrayUnaccessData,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationTextPattern, IUIAutomationTextRange, TextUnit_Word,
    UIA_TextPatternId,
};

/// 往上找带 TextPattern 的祖先的层数上限。
const MAX_PARENTS: usize = 6;

/// 一个词最多取多少字。
const MAX_CHARS: i32 = 64;

/// 词框四周放宽多少再判断鼠标在不在词上（物理像素）：有的程序给的框比字形略小。
const SLACK: i32 = 2;

/// 鼠标 `(x, y)` 下的词及其屏幕位置；取不到回 `None`。
pub(super) fn word_at(x: i32, y: i32) -> Option<(String, RECT)> {
    if unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_err() {
        return None;
    }
    let found = match automation_word(x, y) {
        Ok(found) => found,
        // 程序没给文字范围：截图识字
        Err(error) => {
            tracing::debug!(%error, "hover word unavailable through UI Automation");
            super::ocr::word_at(x, y)
        }
    };
    unsafe { CoUninitialize() };
    found
}

/// 用辅助接口取词：程序给了文字范围就回 `Ok`（鼠标不在字上时是 `Ok(None)`，不再截图），没给回 `Err`。
fn automation_word(x: i32, y: i32) -> windows::core::Result<Option<(String, RECT)>> {
    unsafe {
        let automation: IUIAutomation =
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;
        let point = POINT { x, y };
        let walker = automation.ControlViewWalker()?;
        let mut element = automation.ElementFromPoint(point)?;
        for _ in 0..=MAX_PARENTS {
            // 不支持时 windows 库把「成功但为空」也当错误交回来
            if let Ok(pattern) =
                element.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
            {
                let range = pattern.RangeFromPoint(point)?;
                range.ExpandToEnclosingUnit(TextUnit_Word)?;
                let text = range.GetText(MAX_CHARS)?.to_string();
                let word = text.trim_matches(|c: char| !c.is_alphanumeric());
                // RangeFromPoint 给的是离鼠标最近的词，鼠标在空白处时也会给：看词框盖不盖住鼠标
                let rect = boxes(&range)?.into_iter().find(|rect| {
                    (rect.left - SLACK..rect.right + SLACK).contains(&x)
                        && (rect.top - SLACK..rect.bottom + SLACK).contains(&y)
                });
                return Ok(rect
                    .filter(|_| !word.is_empty())
                    .map(|rect| (word.to_owned(), rect)));
            }
            element = walker.GetParentElement(&element)?;
        }
        Err(windows::core::Error::empty())
    }
}

/// 文字范围的各段屏幕框（一个词折行时有多段）：`GetBoundingRectangles` 给的是按 左、上、宽、高 排的双精度数组。
fn boxes(range: &IUIAutomationTextRange) -> windows::core::Result<Vec<RECT>> {
    unsafe {
        let array = range.GetBoundingRectangles()?;
        if array.is_null() {
            return Ok(Vec::new());
        }
        let lower = SafeArrayGetLBound(array, 1)?;
        let upper = SafeArrayGetUBound(array, 1)?;
        let count = usize::try_from(upper - lower + 1).unwrap_or(0);
        let mut data = std::ptr::null_mut();
        SafeArrayAccessData(array, &mut data)?;
        let values = std::slice::from_raw_parts(data.cast::<f64>(), count);
        let rects = values
            .chunks_exact(4)
            .map(|v| RECT {
                left: v[0] as i32,
                top: v[1] as i32,
                right: (v[0] + v[2]) as i32,
                bottom: (v[1] + v[3]) as i32,
            })
            .collect();
        let _ = SafeArrayUnaccessData(array);
        let _ = SafeArrayDestroy(array);
        Ok(rects)
    }
}
