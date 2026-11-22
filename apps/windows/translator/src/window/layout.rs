//! 窗口布局：按 DPI 建字体，按客户区大小摆放各控件；两块文本区平分固定部分以外的高度。

use qingjian_platform::windows_text::tr;
use windows::Win32::Foundation::{LPARAM, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateFontW, DEFAULT_CHARSET, DeleteObject,
    InvalidateRect, OUT_DEFAULT_PRECIS, VARIABLE_PITCH,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    GetClientRect, MoveWindow, SendMessageW, WM_SETFONT,
};
use windows::core::w;

use super::translation::{self, LANGS};
use super::{
    BACKGROUND, BOX_MARGIN, BUTTON_HEIGHT, CLEAR, COPY, FIXED_HEIGHT, LABEL, LANG, LOOKUP, LOOP,
    NOTE, READ_RESULT, READ_SOURCE, RESULT, RESULT_LABEL, SOURCE, SOURCE_LABEL, SOURCE_LOOP,
    SOURCE_SPEED, SPEED, SPEEDS, STATUS, SWAP, TRANSLATE, Window,
};

impl Window {
    pub(super) fn layout(&mut self) {
        unsafe {
            let dpi = GetDpiForWindow(self.hwnd).max(96);
            let px = |v: i32| v * dpi as i32 / 96;
            if self.dpi != dpi {
                let old = self.font;
                self.font = CreateFontW(
                    -px(14),
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
                let old_small = self.small;
                self.small = CreateFontW(
                    -px(12),
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
                for (id, h) in self.controls.iter().enumerate().skip(1) {
                    let font = if id == NOTE { self.small } else { self.font };
                    SendMessageW(
                        *h,
                        WM_SETFONT,
                        Some(WPARAM(font.0 as usize)),
                        Some(LPARAM(1)),
                    );
                }
                for font in [old, old_small] {
                    if !font.is_invalid() {
                        let _ = DeleteObject(font.into());
                    }
                }
            }
            let mut r = RECT::default();
            let _ = GetClientRect(self.hwnd, &mut r);
            let width = r.right * 96 / dpi as i32;
            let height = r.bottom * 96 / dpi as i32;
            let area = ((height - FIXED_HEIGHT) / 2).max(40);
            let left = 20;
            let end = width - left;
            let row = 96 + area;
            let footer = row + area + 76;
            let h = BUTTON_HEIGHT;
            let gap = 8;
            // 按钮宽度跟着文字走：英文、德文等文案长，写死宽度会截断
            let label = self.text_width(tr("language")) + 4;
            let lang = LANGS
                .iter()
                .enumerate()
                .map(|(i, _)| self.button_width(&format!("{}  ▾", translation::label(i)), 110))
                .max()
                .unwrap_or(110);
            let swap = self.button_width(tr("swap"), 64);
            let background = self.button_width(tr("background"), 80);
            let lookup = self.button_width(tr("lookup"), 64);
            let read_source = self.button_width(tr("read_source"), 88);
            let clear = self.button_width(tr("clear"), 64);
            let translate = self.button_width(tr("translate_now"), 88);
            let read_result = self.button_width(tr("read_result"), 88);
            let speed = SPEEDS
                .iter()
                .map(|s| self.button_width(s.0, 64))
                .max()
                .unwrap_or(64);
            let copy = self.button_width(tr("copy"), 88);
            let repeat = Self::loop_labels()
                .iter()
                .map(|label| self.button_width(label, 72))
                .max()
                .unwrap_or(72);
            let lang_x = left + label + 12;
            for (i, x, y, w, h) in [
                (LABEL, left, 25, label, 22),
                (LANG, lang_x, 20, lang, h),
                (SWAP, lang_x + lang + gap, 20, swap, h),
                (BACKGROUND, end - background, 20, background, h),
                (LOOKUP, end - background - gap - lookup, 20, lookup, h),
                (SOURCE_LABEL, left, 63, end - left, 20),
                (SOURCE, left + 10, 96, end - left - 20, area - BOX_MARGIN),
                (READ_SOURCE, left, row + 3, read_source, h),
                (SOURCE_SPEED, left + read_source + gap, row + 3, speed, h),
                (
                    SOURCE_LOOP,
                    left + read_source + speed + 2 * gap,
                    row + 3,
                    repeat,
                    h,
                ),
                (
                    CLEAR,
                    left + read_source + speed + repeat + 3 * gap,
                    row + 3,
                    clear,
                    h,
                ),
                (TRANSLATE, end - translate, row + 3, translate, h),
                (RESULT_LABEL, left, row + 47, end - left, 20),
                (
                    RESULT,
                    left + 10,
                    row + 79,
                    end - left - 20,
                    area - BOX_MARGIN,
                ),
                (READ_RESULT, left, footer + 3, read_result, h),
                (SPEED, left + read_result + gap, footer + 3, speed, h),
                (
                    LOOP,
                    left + read_result + speed + 2 * gap,
                    footer + 3,
                    repeat,
                    h,
                ),
                (COPY, end - copy, footer + 3, copy, h),
                (STATUS, left, footer + 40, end - left, 22),
                (NOTE, left, footer + 64, end - left, 18),
            ] {
                let _ = MoveWindow(self.controls[i], px(x), px(y), px(w.max(1)), px(h), true);
            }
            let _ = InvalidateRect(Some(self.hwnd), None, true);
        }
    }
}
