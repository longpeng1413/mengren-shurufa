//! 语音面板窗口：不抢焦点、置顶的分层窗口，贴在屏幕下方中央。显示「正在听 / 中间结果 / 出错」文字，
//! 上方一条随音量起伏的波形，底部一行提示。走与候选窗同款的分层合成（`layered::composite`），
//! 内容用 GDI 直接画（文字 + 矩形条），不依赖主题渲染器，样式简单稳定。

use std::cell::RefCell;

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BACKGROUND_MODE, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateFontW, CreateSolidBrush,
    DEFAULT_CHARSET, DT_CALCRECT, DT_EDITCONTROL, DT_NOPREFIX, DT_WORDBREAK, DeleteObject,
    DrawTextW, FF_DONTCARE, FillRect, HDC, OUT_TT_PRECIS, SelectObject, SetBkMode, SetTextColor,
    TRANSPARENT, TextOutW, VARIABLE_PITCH,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, SW_HIDE, SW_SHOWNA, ShowWindow, WM_MOUSEACTIVATE, WNDCLASSEXW,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{PCWSTR, Result, w};

use super::layered::{self, Layered};
use super::monitor;
use super::window_class::WindowClass;

const CLASS_NAME: PCWSTR = w!("QingjianVoicePanel");
static CLASS: WindowClass = WindowClass::new();

/// 内容尺寸（逻辑像素）；窗口会按 DPI 缩放。
const CONTENT_W: i32 = 420;
const CONTENT_H: i32 = 168;
/// 圆角半径。
const RADIUS: i32 = 18;
/// 常规字重；windows crate 未导出。
const FW_NORMAL: i32 = 400;
/// 背景色（浅灰蓝，接近主题面板）。COLORREF 是 0x00BBGGRR。
const BG: COLORREF = COLORREF(0x00F3F4F6);
/// 文字颜色（近黑）。
const FG: COLORREF = COLORREF(0x001F2431);
/// 重量颜色（主题绿）。
const ACCENT: COLORREF = COLORREF(0x0053A86A);

/// 面板要画的内容。
#[derive(Debug, Clone, Default)]
pub(super) struct VoiceView {
    /// `listening` / `recognizing` / `error`。
    pub(super) hint: String,
    pub(super) state: String,
    /// 中间结果或提示文字。
    pub(super) text: String,
    /// 音量 0..1。
    pub(super) level: f32,
    /// 历史音量柱（右进左出），画波形。
    pub(super) history: Vec<f32>,
}

/// 语音面板窗口。
pub(super) struct VoiceWindow {
    hwnd: HWND,
    view: RefCell<VoiceView>,
    visible: std::cell::Cell<bool>,
}

impl VoiceWindow {
    pub(super) fn new() -> Result<Self> {
        CLASS.ensure(|| WNDCLASSEXW {
            lpfnWndProc: Some(wndproc),
            hInstance: super::module_handle(),
            lpszClassName: CLASS_NAME,
            ..Default::default()
        })?;
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
                CLASS_NAME,
                w!("猛人输入法语音"),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(super::module_handle()),
                None,
            )?
        };
        Ok(Self {
            hwnd,
            view: RefCell::new(VoiceView::default()),
            visible: std::cell::Cell::new(false),
        })
    }

    /// 显示 / 更新。把这一拍的音量推进历史，画成滚动波形。
    pub(super) fn show(&self, mut view: VoiceView) {
        {
            let mut history = std::mem::take(&mut self.view.borrow_mut().history);
            history.push(view.level);
            if history.len() > 24 {
                history.remove(0);
            }
            view.history = history;
        }
        *self.view.borrow_mut() = view;
        self.redraw();
        if !self.visible.replace(true) {
            let _ = unsafe { ShowWindow(self.hwnd, SW_SHOWNA) };
        }
    }

    /// 收起。
    pub(super) fn hide(&self) {
        if self.visible.replace(false) {
            let _ = unsafe { ShowWindow(self.hwnd, SW_HIDE) };
        }
    }

    /// 换主题时重画（语音面板用固定色，不随主题，留空实现给 UI 线程的统一调用）。
    pub(super) fn theme_changed(&self) {
        if self.visible.get() {
            self.redraw();
        }
    }

    fn redraw(&self) {
        let dpi = unsafe { GetDpiForWindow(self.hwnd) };
        let scale = dpi as f32 / 96.0;
        let content_w = (CONTENT_W as f32 * scale) as i32;
        let content_h = (CONTENT_H as f32 * scale) as i32;
        let margin = layered::shadow_margin(dpi);
        // 摆在屏幕主显示器下方中央（置顶 + uiAccess，盖过高 z-band 宿主）。
        let area = monitor::primary_work_area();
        let win_w = content_w + 2 * margin;
        let win_h = content_h + 2 * margin;
        let left = area.left + (area.right - area.left - win_w) / 2;
        let top = area.bottom - win_h - (24.0 * scale) as i32;
        let view = self.view.borrow().clone();
        let paint = move |hdc: HDC, client: RECT| paint_content(hdc, client, &view, scale);
        let layered = Layered {
            content: (content_w, content_h),
            margin,
            win_pos: (left, top),
            win_size: (win_w, win_h),
            background: BG,
            corner_radius: (RADIUS as f32 * scale) as i32,
            paint: &paint,
        };
        if let Err(error) = layered::composite(self.hwnd, &layered) {
            tracing::warn!(%error, "语音面板重画失败");
        }
    }
}

/// 在内容坐标系里画：状态行、波形、中间文字、提示。
fn paint_content(hdc: HDC, _client: RECT, view: &VoiceView, scale: f32) {
    let pad = (22.0 * scale) as i32;
    let old_mode = unsafe { SetBkMode(hdc, TRANSPARENT) };
    let font = unsafe {
        CreateFontW(
            (17.0 * scale) as i32,
            0,
            0,
            0,
            FW_NORMAL,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_TT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            (VARIABLE_PITCH.0 | FF_DONTCARE.0) as u32,
            w!("Microsoft YaHei UI"),
        )
    };
    let old_font = unsafe { SelectObject(hdc, font.into()) };

    // 状态行
    unsafe {
        SetTextColor(hdc, ACCENT);
        let title = utf16(view.state.as_str());
        let _ = TextOutW(hdc, pad, pad, &title);
    }

    // 波形：一排竖条，高度按历史音量。
    let wave_top = pad + (34.0 * scale) as i32;
    let wave_h = (30.0 * scale) as i32;
    let bar_w = (10.0 * scale) as i32;
    let gap = (4.0 * scale) as i32;
    let bars = 24usize;
    for (i, &level) in view.history.iter().rev().take(bars).enumerate() {
        let h = (level.clamp(0.0, 1.0) * wave_h as f32) as i32;
        let x = pad + i as i32 * (bar_w + gap);
        let y = wave_top + wave_h - h;
        draw_bar(hdc, x, y, bar_w, h);
    }
    // 当前音量也画一根（最新的一根在右）。
    let h = (view.level.clamp(0.0, 1.0) * wave_h as f32) as i32;
    draw_bar(
        hdc,
        pad + bars as i32 * (bar_w + gap),
        wave_top + wave_h - h,
        bar_w,
        h,
    );

    // 中间结果 / 出错原因：在区域内换行。出错时没有波形和提示行，文字从波形位置排到底；
    // 中间结果放不下时留最后几行（最新说的话），前面用省略号。
    unsafe {
        SetTextColor(hdc, FG);
        let error = view.hint.is_empty();
        let area = RECT {
            left: pad,
            top: if error {
                wave_top
            } else {
                wave_top + wave_h + (12.0 * scale) as i32
            },
            right: (CONTENT_W as f32 * scale) as i32 - pad,
            bottom: if error {
                (CONTENT_H as f32 * scale) as i32 - pad / 2
            } else {
                (134.0 * scale) as i32
            },
        };
        let text = fit_tail(hdc, &view.text, area);
        let mut rect = area;
        draw_text(hdc, &text, &mut rect, WRAP);
    }

    unsafe {
        let _ = TextOutW(hdc, pad, (136.0 * scale) as i32, &utf16(&view.hint));
        SelectObject(hdc, old_font);
        let _ = DeleteObject(font.into());
        let _ = SetBkMode(hdc, BACKGROUND_MODE(old_mode as u32));
    }
}

/// 用实心矩形画一根音量柱。
fn draw_bar(hdc: HDC, x: i32, y: i32, w: i32, h: i32) {
    if h <= 0 {
        return;
    }
    unsafe {
        let brush = CreateSolidBrush(ACCENT);
        let rect = RECT {
            left: x,
            top: y,
            right: x + w,
            bottom: y + h,
        };
        let _ = FillRect(hdc, &rect, brush);
        let _ = DeleteObject(brush.into());
    }
}

const WRAP: windows::Win32::Graphics::Gdi::DRAW_TEXT_FORMAT =
    windows::Win32::Graphics::Gdi::DRAW_TEXT_FORMAT(
        DT_WORDBREAK.0 | DT_EDITCONTROL.0 | DT_NOPREFIX.0,
    );

/// 调 DrawTextW；空文字直接跳过（量高度时高度记 0）。空切片交给 DrawTextW 会在 USER32 里非法访问内存，
/// 开始听写时面板第一帧就是空文字，0.4.12 因此一按就崩。
fn draw_text(
    hdc: HDC,
    text: &str,
    rect: &mut RECT,
    format: windows::Win32::Graphics::Gdi::DRAW_TEXT_FORMAT,
) {
    if text.is_empty() {
        rect.bottom = rect.top;
        return;
    }
    let mut buffer = utf16(text);
    unsafe { DrawTextW(hdc, &mut buffer, rect, format) };
}

/// 文字换行后高过区域时，从前面去掉字（加省略号）直到放得下。
fn fit_tail(hdc: HDC, text: &str, area: RECT) -> String {
    let height = |candidate: &str| {
        let mut rect = area;
        draw_text(hdc, candidate, &mut rect, WRAP | DT_CALCRECT);
        rect.bottom - rect.top
    };
    let limit = area.bottom - area.top;
    if height(text) <= limit {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    let tail = |skip: usize| format!("…{}", chars[skip..].iter().collect::<String>());
    // 去掉的字越多越矮：二分找最少要去掉几个
    let skips: Vec<usize> = (1..chars.len()).collect();
    let fewest = skips.partition_point(|&skip| height(&tail(skip)) > limit);
    skips
        .get(fewest)
        .map(|&skip| tail(skip))
        .unwrap_or_default()
}

fn utf16(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // 不抢焦点：鼠标命中一律交给应用。
    if msg == WM_MOUSEACTIVATE {
        return LRESULT(windows::Win32::UI::WindowsAndMessaging::MA_NOACTIVATE as isize);
    }
    // SAFETY: 窗口过程参数由系统传入，原样转发给默认处理。
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::fit_tail;
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Gdi::{CreateCompatibleDC, DeleteDC};

    #[test]
    fn long_text_keeps_the_latest_words_and_short_text_is_untouched() {
        let hdc = unsafe { CreateCompatibleDC(None) };
        let area = RECT {
            left: 0,
            top: 0,
            right: 200,
            bottom: 40,
        };
        assert_eq!(fit_tail(hdc, "你好", area), "你好");
        // 开始听写时面板先画空文字：曾因把空切片交给 DrawTextW 在 USER32 里崩溃
        assert_eq!(fit_tail(hdc, "", area), "");
        let long = "一二三四五六七八九十".repeat(20) + "最后一句";
        let fitted = fit_tail(hdc, &long, area);
        assert!(fitted.starts_with('…'));
        assert!(fitted.ends_with("最后一句"));
        assert!(fitted.chars().count() < long.chars().count());
        let _ = unsafe { DeleteDC(hdc) };
    }

    /// 走真实的面板绘制：刚开始听写（空文字、空状态行）、长中间结果、出错提示（无提示行）都不能崩。
    #[test]
    fn panel_paints_empty_long_and_error_views() {
        use super::{CONTENT_H, CONTENT_W, VoiceView, paint_content};
        let hdc = unsafe { CreateCompatibleDC(None) };
        let client = RECT {
            left: 0,
            top: 0,
            right: CONTENT_W,
            bottom: CONTENT_H,
        };
        for (state, text, hint) in [
            ("", "", "松开结束 · Esc 取消"),
            ("正在听…", "", "松开结束 · Esc 取消"),
            (
                "识别中…",
                &"很长的一段识别结果".repeat(30),
                "再点「说」结束 · Esc 取消",
            ),
            (
                "语音识别出错",
                "麦克风被系统拒绝：请在「设置 → 隐私和安全性 → 麦克风」允许桌面应用访问，并检查静音键",
                "",
            ),
            ("语音识别出错", "", ""),
        ] {
            let view = VoiceView {
                hint: hint.to_owned(),
                state: state.to_owned(),
                text: text.to_owned(),
                level: 0.5,
                history: vec![0.1, 0.9],
            };
            for scale in [1.0, 1.5] {
                paint_content(hdc, client, &view, scale);
            }
        }
        let _ = unsafe { DeleteDC(hdc) };
    }
}
