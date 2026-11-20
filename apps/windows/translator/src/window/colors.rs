//! 翻译窗口底色和与之配套的文字对比度。
use qingjian_platform::dirs;
use std::cell::RefCell;
use windows::Win32::{
    Foundation::{COLORREF, HWND, RECT},
    Graphics::Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute},
    Graphics::Gdi::{
        self as gdi, CreateSolidBrush, DeleteObject, FillRect, HBRUSH, HDC, RDW_ALLCHILDREN,
        RDW_ERASE, RDW_INVALIDATE, RedrawWindow, SetBkColor, SetTextColor,
    },
    UI::Controls::{DRAWITEMSTRUCT, ODS_DISABLED, ODS_FOCUS, ODS_SELECTED},
    UI::{
        Controls::Dialogs::{CC_FULLOPEN, CC_RGBINIT, CHOOSECOLORW, ChooseColorW},
        WindowsAndMessaging::GetClientRect,
    },
};

thread_local! {
    static PALETTE: RefCell<Option<Palette>> = const { RefCell::new(None) };
}

struct Palette {
    choice: COLORREF,
    background: COLORREF,
    card: COLORREF,
    brush: HBRUSH,
    card_brush: HBRUSH,
}

impl Drop for Palette {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(self.brush.into());
            let _ = DeleteObject(self.card_brush.into());
        }
    }
}

fn settings_file() -> Option<std::path::PathBuf> {
    Some(dirs::user_dir()?.join("translator-background"))
}

fn from_hex(text: &str) -> Option<COLORREF> {
    let text = text.trim();
    let hex = text.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    Some(COLORREF(
        u32::from_str_radix(hex, 16).ok()?.swap_bytes() >> 8,
    ))
}

fn to_hex(color: COLORREF) -> String {
    format!("#{:06X}", (color.0 << 8).swap_bytes())
}

fn foreground(background: COLORREF) -> COLORREF {
    let channel = |shift: u32| {
        let s = ((background.0 >> shift) & 255) as f64 / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    let luminance = channel(0) * 0.2126 + channel(8) * 0.7152 + channel(16) * 0.0722;
    if luminance > 0.179 {
        COLORREF(0)
    } else {
        COLORREF(0xffffff)
    }
}

fn set(choice: COLORREF) {
    // 亮色用作主题底色。大面积编辑区调淡，保留用户选定的色相。
    let light = foreground(choice).0 == 0;
    let background = if light {
        mix(choice, COLORREF(0xf8f8f8), 68)
    } else {
        choice
    };
    let card = if light {
        mix(background, COLORREF(0xffffff), 65)
    } else {
        mix(background, COLORREF(0xffffff), 9)
    };
    let palette = Palette {
        choice,
        background,
        card,
        brush: unsafe { CreateSolidBrush(background) },
        card_brush: unsafe { CreateSolidBrush(card) },
    };
    if !palette.brush.is_invalid() && !palette.card_brush.is_invalid() {
        PALETTE.with_borrow_mut(|slot| *slot = Some(palette));
    }
}

fn mix(base: COLORREF, tint: COLORREF, percent: u32) -> COLORREF {
    let channel = |shift: u32| {
        let a = (base.0 >> shift) & 255;
        let b = (tint.0 >> shift) & 255;
        ((a * (100 - percent) + b * percent) / 100) << shift
    };
    COLORREF(channel(0) | channel(8) | channel(16))
}

pub fn caption(hwnd: HWND) {
    PALETTE.with_borrow(|slot| {
        if let Some(p) = slot {
            let dark = i32::from(foreground(p.background).0 != 0);
            let _ = unsafe {
                DwmSetWindowAttribute(
                    hwnd,
                    DWMWA_USE_IMMERSIVE_DARK_MODE,
                    (&dark as *const i32).cast(),
                    4,
                )
            };
        }
    });
}

pub fn init() {
    let configured = settings_file()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| from_hex(&text));
    set(configured.unwrap_or(COLORREF(0)));
}

pub fn choose(hwnd: HWND) {
    let selected =
        PALETTE.with_borrow(|palette| palette.as_ref().map_or(COLORREF(0), |p| p.choice));
    let mut custom = [COLORREF(0); 16];
    let mut picker = CHOOSECOLORW {
        lStructSize: size_of::<CHOOSECOLORW>() as u32,
        hwndOwner: hwnd,
        rgbResult: selected,
        lpCustColors: custom.as_mut_ptr(),
        Flags: CC_RGBINIT | CC_FULLOPEN,
        ..Default::default()
    };
    if unsafe { ChooseColorW(&mut picker) }.as_bool() {
        apply(hwnd, picker.rgbResult);
    }
}

pub fn apply(hwnd: HWND, color: COLORREF) {
    set(color);
    caption(hwnd);
    if let Some(path) = settings_file()
        && let Err(error) = std::fs::write(path, to_hex(color))
    {
        tracing::warn!(%error,"translator background could not be saved");
    }
    let _ = unsafe {
        RedrawWindow(
            Some(hwnd),
            None,
            None,
            RDW_ALLCHILDREN | RDW_ERASE | RDW_INVALIDATE,
        )
    };
}

pub fn brush(dc: HDC, card: bool) -> Option<HBRUSH> {
    PALETTE.with_borrow(|slot| {
        let p = slot.as_ref()?;
        let bg = if card { p.card } else { p.background };
        unsafe {
            SetBkColor(dc, bg);
            SetTextColor(dc, foreground(bg));
        }
        Some(if card { p.card_brush } else { p.brush })
    })
}

/// 窗口底色画刷（由配色持有，别删）；下拉菜单拿它铺底。
pub fn background_brush() -> Option<HBRUSH> {
    PALETTE.with_borrow(|slot| slot.as_ref().map(|p| p.brush))
}

/// 下拉菜单窗口的边框色。
pub fn menu_border() -> Option<COLORREF> {
    PALETTE.with_borrow(|slot| {
        let p = slot.as_ref()?;
        Some(mix(p.background, foreground(p.background), 22))
    })
}

/// 底色深不深（深色时提示框用深色主题）。
pub fn dark() -> bool {
    PALETTE.with_borrow(|slot| {
        slot.as_ref()
            .is_some_and(|p| foreground(p.background).0 != 0)
    })
}

/// 画一个下拉菜单项：悬停时加一层高亮，选中的项左边打勾。
pub fn draw_menu_item(
    dc: HDC,
    rect: RECT,
    label: &str,
    checked: bool,
    selected: bool,
    font: gdi::HFONT,
    dpi: i32,
) {
    PALETTE.with_borrow(|slot| {
        let Some(p) = slot else { return };
        let ink = foreground(p.background);
        let fill = if selected {
            mix(p.background, ink, 18)
        } else {
            p.background
        };
        unsafe {
            gdi::SetDCBrushColor(dc, fill);
            FillRect(dc, &rect, HBRUSH(gdi::GetStockObject(gdi::DC_BRUSH).0));
            gdi::SetBkMode(dc, gdi::TRANSPARENT);
            SetTextColor(dc, ink);
            let old = gdi::SelectObject(dc, font.into());
            let gutter = 32 * dpi / 96;
            let flags = gdi::DT_VCENTER | gdi::DT_SINGLELINE | gdi::DT_NOPREFIX;
            if checked {
                let mut mark: Vec<u16> = "✓".encode_utf16().collect();
                let mut bounds = RECT {
                    right: rect.left + gutter,
                    ..rect
                };
                let _ = gdi::DrawTextW(dc, &mut mark, &mut bounds, flags | gdi::DT_CENTER);
            }
            let mut text: Vec<u16> = label.encode_utf16().collect();
            let mut bounds = RECT {
                left: rect.left + gutter,
                right: rect.right - 12 * dpi / 96,
                ..rect
            };
            let _ = gdi::DrawTextW(dc, &mut text, &mut bounds, flags | gdi::DT_LEFT);
            gdi::SelectObject(dc, old);
        }
    });
}

/// 说明小字：在 [`brush`] 设好的底色上把文字调淡。
pub fn mute(dc: HDC) {
    PALETTE.with_borrow(|slot| {
        if let Some(p) = slot {
            unsafe {
                SetTextColor(dc, mix(foreground(p.background), p.background, 45));
            }
        }
    });
}

fn rounded(dc: HDC, r: RECT, fill: COLORREF, edge: COLORREF, radius: i32) {
    unsafe {
        let pen = gdi::SelectObject(dc, gdi::GetStockObject(gdi::DC_PEN));
        let brush = gdi::SelectObject(dc, gdi::GetStockObject(gdi::DC_BRUSH));
        gdi::SetDCPenColor(dc, edge);
        gdi::SetDCBrushColor(dc, fill);
        let _ = gdi::RoundRect(dc, r.left, r.top, r.right, r.bottom, radius, radius);
        gdi::SelectObject(dc, brush);
        gdi::SelectObject(dc, pen);
    }
}

const MINT: COLORREF = COLORREF(0xa4c971);

pub fn draw_button(item: &DRAWITEMSTRUCT) {
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowTextLengthW, GetWindowTextW};
    PALETTE.with_borrow(|slot| {
        let Some(p) = slot else { return };
        let ink = foreground(p.background);
        let primary = item.CtlID as usize == super::TRANSLATE;
        let focused = item.itemState.0 & ODS_FOCUS.0 != 0;
        let down = item.itemState.0 & ODS_SELECTED.0 != 0;
        let disabled = item.itemState.0 & ODS_DISABLED.0 != 0;
        let fill = if primary && disabled {
            mix(MINT, p.background, 65)
        } else if primary {
            mix(MINT, p.background, if down { 28 } else { 5 })
        } else {
            mix(p.background, ink, if down { 27 } else { 15 })
        };
        let edge = if focused {
            mix(ink, fill, 20)
        } else {
            mix(fill, foreground(fill), 14)
        };
        unsafe {
            FillRect(item.hDC, &item.rcItem, p.brush);
            let dpi = windows::Win32::UI::HiDpi::GetDpiForWindow(item.hwndItem).max(96) as i32;
            rounded(item.hDC, item.rcItem, fill, edge, 12 * dpi / 96);
            let mut text = vec![0; GetWindowTextLengthW(item.hwndItem) as usize + 1];
            let len = GetWindowTextW(item.hwndItem, &mut text);
            let dc = item.hDC;
            gdi::SetBkMode(dc, gdi::TRANSPARENT);
            SetTextColor(
                dc,
                mix(foreground(fill), fill, if disabled { 55 } else { 5 }),
            );
            let mut bounds = item.rcItem;
            let _ = gdi::DrawTextW(
                dc,
                &mut text[..len as usize],
                &mut bounds,
                gdi::DT_CENTER
                    | gdi::DT_VCENTER
                    | gdi::DT_SINGLELINE
                    | gdi::DT_NOPREFIX
                    | gdi::DT_END_ELLIPSIS,
            );
        }
    });
}

pub fn erase(hwnd: HWND, dc: HDC) -> bool {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::{
        HiDpi::GetDpiForWindow, Input::KeyboardAndMouse::GetFocus, WindowsAndMessaging::GetDlgItem,
    };
    PALETTE.with_borrow(|slot| {
        let Some(p) = slot else { return false };
        unsafe {
            let mut rect = RECT::default();
            if GetClientRect(hwnd, &mut rect).is_err() {
                return false;
            }
            FillRect(dc, &rect, p.brush);
            let dpi = GetDpiForWindow(hwnd).max(96) as i32;
            for id in [super::SOURCE, super::RESULT] {
                let Ok(child) = GetDlgItem(Some(hwnd), id as i32) else {
                    continue;
                };
                let mut size = RECT::default();
                if GetClientRect(child, &mut size).is_err() {
                    continue;
                }
                let mut pt = [POINT::default()];
                gdi::MapWindowPoints(Some(child), Some(hwnd), &mut pt);
                let pad = 10 * dpi / 96;
                let r = RECT {
                    left: pt[0].x - pad,
                    top: pt[0].y - pad,
                    right: pt[0].x + size.right + pad,
                    bottom: pt[0].y + size.bottom + pad,
                };
                let edge = if GetFocus() == child {
                    mix(MINT, p.background, 20)
                } else {
                    mix(p.card, foreground(p.card), 16)
                };
                rounded(dc, r, p.card, edge, 16 * dpi / 96);
            }
        }
        true
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_button_paint_follows_dark_and_custom_palettes() {
        use windows::Win32::UI::WindowsAndMessaging::*;
        use windows::core::w;
        unsafe {
            let window = CreateWindowExW(
                Default::default(),
                w!("BUTTON"),
                w!("OK"),
                WS_POPUP,
                0,
                0,
                100,
                32,
                None,
                None,
                None,
                None,
            )
            .unwrap();
            let screen = gdi::GetDC(None);
            let dc = gdi::CreateCompatibleDC(Some(screen));
            let bitmap = gdi::CreateCompatibleBitmap(screen, 100, 32);
            let old = gdi::SelectObject(dc, bitmap.into());
            let item = DRAWITEMSTRUCT {
                hDC: dc,
                hwndItem: window,
                rcItem: RECT {
                    left: 0,
                    top: 0,
                    right: 100,
                    bottom: 32,
                },
                ..Default::default()
            };
            let mut paints = Vec::new();
            for bg in [COLORREF(0), from_hex("#FFFF80").unwrap()] {
                set(bg);
                draw_button(&item);
                paints.push(gdi::GetPixel(dc, 12, 12));
            }
            gdi::SelectObject(dc, old);
            let _ = gdi::DeleteObject(bitmap.into());
            let _ = gdi::DeleteDC(dc);
            gdi::ReleaseDC(None, screen);
            let _ = DestroyWindow(window);
            assert_eq!(paints[0], mix(COLORREF(0), COLORREF(0xffffff), 15));
            assert_eq!(
                paints[1],
                mix(
                    mix(from_hex("#FFFF80").unwrap(), COLORREF(0xf8f8f8), 68),
                    COLORREF(0),
                    15
                )
            );
        }
    }
    #[test]
    fn persisted_rgb_and_text_contrast() {
        let color = from_hex("#4080CC").unwrap();
        assert_eq!(color.0, 0xcc8040);
        assert_eq!(to_hex(color), "#4080CC");
        assert!(from_hex("broken").is_none());
        assert_eq!(foreground(COLORREF(0)), COLORREF(0xffffff));
        assert_eq!(foreground(COLORREF(0xffffff)), COLORREF(0));
    }
}
