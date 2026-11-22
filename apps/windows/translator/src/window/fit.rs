//! 窗口随文字多少调大小：带进新选区时按原文定高（短了回到默认高），译文回来放不下再长高；
//! 屏幕不够高时适当加宽。用户自己打字时不动窗口，免得边打边跳；最大化、最小化时也不动。

use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    DT_CALCRECT, DT_EDITCONTROL, DT_EXPANDTABS, DT_NOPREFIX, DT_WORDBREAK, DrawTextW, GetDC,
    GetMonitorInfoW, HFONT, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow, ReleaseDC,
    SelectObject,
};
use windows::Win32::UI::Controls::EM_GETRECT;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    GetClientRect, GetWindowRect, IsIconic, IsZoomed, SWP_NOACTIVATE, SWP_NOZORDER, SendMessageW,
    SetWindowPos,
};

use super::{BOX_MARGIN, DEFAULT_HEIGHT, DEFAULT_WIDTH, FIXED_HEIGHT, SOURCE, Window};

/// 自动加宽的上限（96 DPI 下的像素）。
const MAX_WIDTH: i32 = 1200;

/// 屏幕不够高时每次加宽多少（96 DPI 下的像素）。
const WIDEN_STEP: i32 = 160;

/// 每个文本框在文字高度外多留的余量（96 DPI 下的像素），末行不被裁掉。
const BOX_SLACK: i32 = 12;

impl Window {
    /// `reset` 为真（新选区）时按原文定大小，窗口没被用户拖过就可以缩回默认；否则只在原文或译文放不下时长大。
    pub(super) fn fit(&mut self, reset: bool) {
        unsafe {
            if IsZoomed(self.hwnd).as_bool() || IsIconic(self.hwnd).as_bool() {
                return;
            }
            let dpi = GetDpiForWindow(self.hwnd).max(96) as i32;
            let px = |v: i32| v * dpi / 96;
            let (mut window, mut client, mut format) =
                (RECT::default(), RECT::default(), RECT::default());
            if GetWindowRect(self.hwnd, &mut window).is_err()
                || GetClientRect(self.hwnd, &mut client).is_err()
            {
                return;
            }
            SendMessageW(
                self.controls[SOURCE],
                EM_GETRECT,
                None,
                Some(LPARAM(&mut format as *mut RECT as isize)),
            );
            let mut info = MONITORINFO {
                cbSize: size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            let monitor = MonitorFromWindow(self.hwnd, MONITOR_DEFAULTTONEAREST);
            if !GetMonitorInfoW(monitor, &mut info).as_bool() {
                return;
            }
            let work = info.rcWork;
            let (work_width, work_height) = (work.right - work.left, work.bottom - work.top);
            let (old_width, old_height) = (window.right - window.left, window.bottom - window.top);
            let frame = old_height - client.bottom;
            let texts: &[&str] = if reset {
                &[&self.model.source]
            } else {
                &[&self.model.source, &self.model.output]
            };
            // 两个文本框平分中间的高度，所以按较高的那段算
            let needed = |width: i32| {
                let text_width = format.right - format.left + width - old_width;
                let text = texts
                    .iter()
                    .map(|text| self.text_height(text, text_width))
                    .max()
                    .unwrap_or(0);
                frame + px(FIXED_HEIGHT + 2 * (BOX_MARGIN + BOX_SLACK)) + 2 * text
            };
            let shrink = reset && self.fitted == Some((old_width, old_height));
            let base = if shrink { px(DEFAULT_WIDTH) } else { old_width };
            let max_width = work_width.min(px(MAX_WIDTH)).max(base);
            let mut width = base;
            let mut height = needed(width);
            // 加宽只在能少折几行时才做；多是短行（段落、列表）时加宽没用
            while height > work_height && width < max_width {
                let wider = (width + px(WIDEN_STEP)).min(max_width);
                let lower = needed(wider);
                if lower >= height {
                    break;
                }
                (width, height) = (wider, lower);
            }
            let floor = if shrink || (reset && self.fitted.is_none()) {
                px(DEFAULT_HEIGHT)
            } else {
                old_height
            };
            let height = height.max(floor).min(work_height);
            if (width, height) == (old_width, old_height) {
                return;
            }
            let left = window.left.min(work.right - width).max(work.left);
            let top = window.top.min(work.bottom - height).max(work.top);
            let _ = SetWindowPos(
                self.hwnd,
                None,
                left,
                top,
                width,
                height,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            // 记实际尺寸：系统可能按最小尺寸改过
            if GetWindowRect(self.hwnd, &mut window).is_ok() {
                self.fitted = Some((window.right - window.left, window.bottom - window.top));
            }
        }
    }

    /// 文字在文本框字体下按 `width`（物理像素）折行后的高度。
    fn text_height(&self, text: &str, width: i32) -> i32 {
        measure(self.hwnd, self.font, text, width)
    }
}

/// 文字用 `font` 按 `width`（物理像素）折行后的高度，与多行文本框的折行一致；翻译窗口与划词卡片共用。
pub(super) fn measure(hwnd: HWND, font: HFONT, text: &str, width: i32) -> i32 {
    if text.is_empty() {
        return 0;
    }
    let mut wide: Vec<u16> = text.replace('\n', "\r\n").encode_utf16().collect();
    let mut rect = RECT {
        right: width.max(1),
        ..Default::default()
    };
    unsafe {
        let dc = GetDC(Some(hwnd));
        let old = SelectObject(dc, font.into());
        DrawTextW(
            dc,
            &mut wide,
            &mut rect,
            DT_CALCRECT | DT_WORDBREAK | DT_EDITCONTROL | DT_NOPREFIX | DT_EXPANDTABS,
        );
        SelectObject(dc, old);
        ReleaseDC(Some(hwnd), dc);
    }
    rect.bottom - rect.top
}
