//! 翻译窗口底色和与之配套的文字对比度。
use qingjian_platform::dirs;
use std::cell::RefCell;
use windows::Win32::{
    Foundation::{COLORREF, HWND, RECT},
    Graphics::Gdi::{
        CreateSolidBrush, DeleteObject, FillRect, HBRUSH, HDC, RDW_ALLCHILDREN, RDW_ERASE,
        RDW_INVALIDATE, RedrawWindow, SetBkColor, SetTextColor,
    },
    UI::{
        Controls::Dialogs::{CC_FULLOPEN, CC_RGBINIT, CHOOSECOLORW, ChooseColorW},
        WindowsAndMessaging::GetClientRect,
    },
};

thread_local! {
    static PALETTE: RefCell<Option<Palette>> = const { RefCell::new(None) };
}

struct Palette {
    background: COLORREF,
    foreground: COLORREF,
    brush: HBRUSH,
}

impl Drop for Palette {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(self.brush.into());
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

fn set(background: COLORREF) {
    let brush = unsafe { CreateSolidBrush(background) };
    if !brush.is_invalid() {
        PALETTE.with_borrow_mut(|palette| {
            *palette = Some(Palette {
                background,
                foreground: foreground(background),
                brush,
            })
        });
    }
}

pub fn init() {
    let configured = settings_file()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| from_hex(&text));
    set(configured.unwrap_or(COLORREF(0)));
}

pub fn choose(hwnd: HWND) {
    let selected =
        PALETTE.with_borrow(|palette| palette.as_ref().map_or(COLORREF(0), |p| p.background));
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
        set(picker.rgbResult);
        if let Some(path) = settings_file()
            && let Err(error) = std::fs::write(path, to_hex(picker.rgbResult))
        {
            tracing::warn!(%error, "translator background could not be saved");
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
}

pub fn brush(dc: HDC) -> Option<HBRUSH> {
    PALETTE.with_borrow(|palette| {
        let palette = palette.as_ref()?;
        unsafe {
            SetBkColor(dc, palette.background);
            SetTextColor(dc, palette.foreground);
        }
        Some(palette.brush)
    })
}

pub fn erase(hwnd: HWND, dc: HDC) -> bool {
    let Some(brush) = brush(dc) else {
        return false;
    };
    let mut rect = RECT::default();
    unsafe { GetClientRect(hwnd, &mut rect).is_ok() && FillRect(dc, &rect, brush) != 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

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
