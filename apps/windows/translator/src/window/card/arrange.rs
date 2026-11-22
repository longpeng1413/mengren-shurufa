//! 卡片排版：按文字量定各块高度，放不下先长高（最多占屏幕可用高度九成）再加宽；
//! 用户拖过边框后按用户定的大小排，正文占满剩下的高度，放不下就在文本框里滚动。

use windows::Win32::Foundation::{LPARAM, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, HFONT, InvalidateRect, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
};
use windows::Win32::UI::Controls::{SetWindowTheme, ShowScrollBar};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, GetDpiForWindow, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClientRect, HWND_TOPMOST, MoveWindow, SB_VERT, SW_HIDE, SW_SHOW, SWP_NOACTIVATE,
    SWP_SHOWWINDOW, SendMessageW, SetWindowPos, ShowWindow, WM_SETFONT,
};
use windows::core::w;

use super::super::colors;
use super::super::fit::measure;
use super::{
    BODY, CLOSE, COPY, Card, DETAILS, EXTRA, LOOP, OPEN, ORIGINAL, PHONETIC, SPEAK_RESULT,
    SPEAK_SOURCE, TITLE,
};

/// 卡片默认宽度、四周留白、行距、按钮高度（96 DPI 下的像素）。
const WIDTH: i32 = 380;
const PADDING: i32 = 16;
const GAP: i32 = 8;
const BUTTON: i32 = 28;

/// 原文区最高多少（96 DPI 下的像素），再长看前几行就够，全文在正文或翻译窗口里看。
const ORIGINAL_MAX: i32 = 84;

/// 词典区最高多少（96 DPI 下的像素），再多就在框里滚动。
const DETAILS_MAX: i32 = 220;

/// 文本区内边距（与圆角底板之间留的空）。
const INSET: i32 = 10;

/// 自动排版时卡片最多占所在屏工作区高度、宽度的几成。
const MAX_HEIGHT_SHARE: i32 = 90;
const MAX_WIDTH_SHARE: i32 = 60;

/// 正文放不下时每次加宽多少（96 DPI 下的像素）。
const WIDEN_STEP: i32 = 80;

/// 拖边框时卡片的最小宽高（96 DPI 下的像素）。
pub(super) const MIN_WIDTH: i32 = 300;
pub(super) const MIN_HEIGHT: i32 = 150;

/// 一个子控件的编号、位置、大小与字体。
type Row = (usize, i32, i32, i32, i32, HFONT);

/// 标题、正文、小字三种字体。
type Fonts = (HFONT, HFONT, HFONT);

impl Card {
    /// 自动排版并摆在「译」图标旁：新卡片与译文到达时调；用户拖过大小就只按当前大小重排。
    pub(super) fn place(&mut self) {
        if self.manual {
            self.resized();
            return;
        }
        let (dpi, work) = monitor_at(self.anchor);
        let fonts = self.fonts(dpi);
        let px = |value: i32| value * dpi as i32 / 96;
        let max_height = (work.bottom - work.top) * MAX_HEIGHT_SHARE / 100;
        let mut width = self.base_width(dpi, fonts.1).min(work.right - work.left);
        let max_width = ((work.right - work.left) * MAX_WIDTH_SHARE / 100).max(width);
        let (mut rows, mut height, mut overflow) = self.rows(dpi, fonts, width, None, max_height);
        // 长到顶还放不下：加宽，长段落少折几行
        while overflow && width < max_width {
            width = (width + px(WIDEN_STEP)).min(max_width);
            (rows, height, overflow) = self.rows(dpi, fonts, width, None, max_height);
        }
        self.apply(&rows, overflow);
        // 放在图标下方；下方放不下就翻到图标上方，再不行贴着工作区
        let (anchor_x, anchor_y) = self.anchor;
        let left = anchor_x.min(work.right - width).max(work.left);
        let mut top = anchor_y + px(6);
        if top + height > work.bottom {
            top = anchor_y - px(26 + 12) - height;
        }
        let top = top.min(work.bottom - height).max(work.top);
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                left,
                top,
                width,
                height,
                SWP_SHOWWINDOW | SWP_NOACTIVATE,
            );
            let _ = InvalidateRect(Some(self.hwnd), None, true);
        }
    }

    /// 用户拖了边框：按当前客户区大小重排，正文占满剩下的高度。
    pub(in crate::window) fn resized(&mut self) {
        let dpi = unsafe { GetDpiForWindow(self.hwnd) }.max(96);
        let fonts = self.fonts(dpi);
        let mut client = RECT::default();
        let _ = unsafe { GetClientRect(self.hwnd, &mut client) };
        let (rows, _, overflow) = self.rows(dpi, fonts, client.right, Some(client.bottom), 0);
        self.apply(&rows, overflow);
        let _ = unsafe { InvalidateRect(Some(self.hwnd), None, true) };
    }

    /// 这张卡片底部的左侧按钮：朗读、复制（只有提示时没有）。
    fn buttons(&self) -> Vec<usize> {
        if self.text.is_empty() {
            return Vec::new();
        }
        if self.word.is_some() {
            vec![SPEAK_SOURCE, COPY, LOOP]
        } else {
            vec![SPEAK_SOURCE, SPEAK_RESULT, COPY, LOOP]
        }
    }

    /// 放得下一排按钮的卡片宽度，不小于默认宽度。
    fn base_width(&self, dpi: u32, font: HFONT) -> i32 {
        let px = |value: i32| value * dpi as i32 / 96;
        let row = self
            .buttons()
            .iter()
            .map(|&id| self.button_width(id, font, px(64), px(24)) + px(GAP))
            .sum::<i32>()
            + px(GAP)
            + self.button_width(OPEN, font, px(88), px(24));
        (row + 2 * px(PADDING)).max(px(WIDTH))
    }

    /// 按宽度排各块。`height` 给定时正文填满剩余高度；否则按文字量，整张不超过 `max_height`。
    /// 返回各块位置、卡片总高、正文是不是放不下（要滚动）。
    fn rows(
        &self,
        dpi: u32,
        (title, font, small): Fonts,
        width: i32,
        height: Option<i32>,
        max_height: i32,
    ) -> (Vec<Row>, i32, bool) {
        let px = |value: i32| value * dpi as i32 / 96;
        let word = self.word.is_some();
        let inner = width - 2 * px(PADDING);
        let text_width = inner - 2 * px(INSET);
        let mut rows: Vec<Row> = Vec::new();
        let mut y = px(PADDING);
        rows.push((TITLE, px(PADDING), y, inner - px(34), px(28), title));
        rows.push((CLOSE, width - px(PADDING) - px(26), y, px(26), px(26), font));
        y += px(32);
        if word && !self.text_of(PHONETIC).is_empty() {
            rows.push((PHONETIC, px(PADDING), y, inner, px(22), font));
            y += px(24);
        }
        let extra = self.text_of(EXTRA);
        if !extra.is_empty() {
            let line = measure(self.hwnd, small, &extra, inner).max(px(16));
            rows.push((EXTRA, px(PADDING), y, inner, line, small));
            y += line + px(GAP);
        }
        if !word && !self.text.is_empty() {
            let original =
                measure(self.hwnd, font, &self.text, text_width).clamp(px(18), px(ORIGINAL_MAX));
            y += px(INSET);
            rows.push((ORIGINAL, px(PADDING + INSET), y, text_width, original, font));
            y += original + px(INSET) + px(GAP);
        }
        // 句子卡的词典区排在译文下面，高度有上限，正文占其余的
        let details = if !word && !self.details.is_empty() {
            measure(self.hwnd, font, &self.details, text_width).clamp(px(20), px(DETAILS_MAX))
        } else {
            0
        };
        let below = 2 * px(INSET)
            + px(GAP)
            + px(BUTTON)
            + px(PADDING)
            + if details > 0 {
                details + 2 * px(INSET) + px(GAP)
            } else {
                0
            };
        let measured = measure(self.hwnd, font, &self.text_of(BODY), text_width).max(px(20));
        let body = match height {
            Some(height) => (height - y - below).max(px(20)),
            None => measured.min((max_height - y - below).max(px(60))),
        };
        y += px(INSET);
        rows.push((BODY, px(PADDING + INSET), y, text_width, body, font));
        y += body + px(INSET) + px(GAP);
        if details > 0 {
            y += px(INSET);
            rows.push((DETAILS, px(PADDING + INSET), y, text_width, details, font));
            y += details + px(INSET) + px(GAP);
        }
        let mut left = px(PADDING);
        for id in self.buttons() {
            let w = self.button_width(id, font, px(64), px(24));
            rows.push((id, left, y, w, px(BUTTON), font));
            left += w + px(GAP);
        }
        let open = self.button_width(OPEN, font, px(88), px(24));
        rows.push((OPEN, width - px(PADDING) - open, y, open, px(BUTTON), font));
        (rows, y + px(BUTTON) + px(PADDING), measured > body)
    }

    /// 按排好的位置摆子控件，没排上的藏起来；正文放不下时给它滚动条。
    fn apply(&self, rows: &[Row], overflow: bool) {
        unsafe {
            // 只藏这次用不到的，用得到的原地挪，免得每次重排都整片闪一下
            for id in [
                TITLE,
                PHONETIC,
                EXTRA,
                ORIGINAL,
                BODY,
                SPEAK_SOURCE,
                SPEAK_RESULT,
                COPY,
                OPEN,
                CLOSE,
                DETAILS,
                LOOP,
            ] {
                if !rows.iter().any(|row| row.0 == id) {
                    let _ = ShowWindow(self.controls[id], SW_HIDE);
                }
            }
            for &(id, x, y, w, h, font) in rows {
                let control = self.controls[id];
                SendMessageW(
                    control,
                    WM_SETFONT,
                    Some(WPARAM(font.0 as usize)),
                    Some(LPARAM(0)),
                );
                let _ = MoveWindow(control, x, y, w, h, true);
                let _ = ShowWindow(control, SW_SHOW);
            }
            let body = self.controls[BODY];
            // 深色底上用深色滚动条，免得一条亮白的竖条
            let theme = if colors::dark() {
                w!("DarkMode_Explorer")
            } else {
                w!("Explorer")
            };
            let _ = SetWindowTheme(body, theme, None);
            let _ = ShowScrollBar(body, SB_VERT, overflow);
        }
    }
}

/// 点 `(x, y)` 所在屏的 DPI 与工作区。
fn monitor_at((x, y): (i32, i32)) -> (u32, RECT) {
    unsafe {
        let monitor = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
        let (mut dpi, mut unused) = (96, 96);
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut unused);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let _ = GetMonitorInfoW(monitor, &mut info);
        (dpi.max(96), info.rcWork)
    }
}
