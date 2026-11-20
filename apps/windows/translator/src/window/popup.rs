//! 下拉菜单（互译语言、背景色、语速）：系统菜单固定白底，改成自绘，底色、文字与高亮跟随窗口配色。
//!
//! 菜单项设成 `MF_OWNERDRAW`，系统在菜单弹着时向所属窗口发 `WM_MEASUREITEM` / `WM_DRAWITEM`；
//! 窗口过程只转到这里，菜单内容在弹出期间放在线程局部变量里。
use std::cell::RefCell;
use windows::Win32::{
    Foundation::{COLORREF, HWND, RECT, SIZE},
    Graphics::Dwm::{DWMWA_BORDER_COLOR, DwmSetWindowAttribute},
    Graphics::Gdi::{GetDC, GetTextExtentPoint32W, HFONT, ReleaseDC, SelectObject},
    UI::Controls::{DRAWITEMSTRUCT, MEASUREITEMSTRUCT, ODS_SELECTED},
    UI::HiDpi::GetDpiForWindow,
    UI::WindowsAndMessaging::{
        AppendMenuW, CreatePopupMenu, DestroyMenu, MENUINFO, MF_OWNERDRAW, MIM_BACKGROUND,
        SetMenuInfo, TPM_RETURNCMD, TrackPopupMenu,
    },
};
use windows::core::PCWSTR;

/// 正弹着的菜单。
struct Open {
    labels: Vec<String>,

    /// 打勾的那一项。
    checked: Option<usize>,

    font: HFONT,

    owner: HWND,

    /// 已改过边框色的菜单窗口，免得空闲时反复设。
    framed: Option<HWND>,
}

thread_local! {
    static OPEN: RefCell<Option<Open>> = const { RefCell::new(None) };
}

/// 在 `anchor` 下方弹出菜单，返回选中项的下标；点空白处收起回 `None`。
pub fn show(
    owner: HWND,
    anchor: RECT,
    font: HFONT,
    labels: &[&str],
    checked: Option<usize>,
) -> Option<usize> {
    unsafe {
        let menu = CreatePopupMenu().ok()?;
        if let Some(brush) = super::colors::background_brush() {
            let info = MENUINFO {
                cbSize: size_of::<MENUINFO>() as u32,
                fMask: MIM_BACKGROUND,
                hbrBack: brush,
                ..Default::default()
            };
            let _ = SetMenuInfo(menu, &info);
        }
        for index in 0..labels.len() {
            if AppendMenuW(menu, MF_OWNERDRAW, index + 1, PCWSTR::null()).is_err() {
                let _ = DestroyMenu(menu);
                return None;
            }
        }
        OPEN.set(Some(Open {
            labels: labels.iter().map(|label| (*label).to_owned()).collect(),
            checked,
            font,
            owner,
            framed: None,
        }));
        let chosen = TrackPopupMenu(
            menu,
            TPM_RETURNCMD,
            anchor.left,
            anchor.bottom,
            None,
            owner,
            None,
        )
        .0;
        OPEN.set(None);
        let _ = DestroyMenu(menu);
        usize::try_from(chosen).ok().and_then(|n| n.checked_sub(1))
    }
}

/// `WM_MEASUREITEM`：每项高 28、宽按文字（左边留出打勾的位置）。
pub fn measure(item: &mut MEASUREITEMSTRUCT) {
    OPEN.with_borrow(|open| {
        let Some(open) = open else { return };
        let Some(label) = (item.itemID as usize)
            .checked_sub(1)
            .and_then(|i| open.labels.get(i))
        else {
            return;
        };
        let dpi = unsafe { GetDpiForWindow(open.owner) }.max(96) as i32;
        let wide: Vec<u16> = label.encode_utf16().collect();
        let mut size = SIZE::default();
        unsafe {
            let dc = GetDC(Some(open.owner));
            let old = SelectObject(dc, open.font.into());
            let _ = GetTextExtentPoint32W(dc, &wide, &mut size);
            SelectObject(dc, old);
            ReleaseDC(Some(open.owner), dc);
        }
        item.itemWidth = (size.cx + 52 * dpi / 96) as u32;
        item.itemHeight = (28 * dpi / 96) as u32;
    });
}

/// `WM_DRAWITEM`（菜单项）。
pub fn draw(item: &DRAWITEMSTRUCT) {
    OPEN.with_borrow(|open| {
        let Some(open) = open else { return };
        let Some(index) = (item.itemID as usize).checked_sub(1) else {
            return;
        };
        let Some(label) = open.labels.get(index) else {
            return;
        };
        let dpi = unsafe { GetDpiForWindow(open.owner) }.max(96) as i32;
        super::colors::draw_menu_item(
            item.hDC,
            item.rcItem,
            label,
            open.checked == Some(index),
            item.itemState.0 & ODS_SELECTED.0 != 0,
            open.font,
            dpi,
        );
    });
}

/// `WM_ENTERIDLE`（菜单弹着）：Windows 11 给菜单窗口描一圈浅色边框，改成与底色相配的颜色。
pub fn frame(menu_window: HWND) {
    OPEN.with_borrow_mut(|open| {
        let Some(open) = open else { return };
        if open.framed == Some(menu_window) {
            return;
        }
        open.framed = Some(menu_window);
        if let Some(color) = super::colors::menu_border() {
            let _ = unsafe {
                DwmSetWindowAttribute(
                    menu_window,
                    DWMWA_BORDER_COLOR,
                    (&color as *const COLORREF).cast(),
                    size_of_val(&color) as u32,
                )
            };
        }
    });
}
