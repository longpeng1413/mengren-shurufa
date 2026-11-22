//! 划词翻译（`[lookup] selection`）：拖选或双击选中文字后在鼠标旁显示「译」图标，点它读出选中的文字，
//! 交给翻译窗口程序弹词典卡片（单个英文词）或译文。
//! 悬停取词（`[lookup] hover`）：图标线程定时看鼠标停没停住（[`hover`]），停住就取鼠标下的词（[`point`]），
//! 在词下方弹不抢焦点的卡片。
//!
//! 低级鼠标 / 键盘钩子（[`crate::hotkey`]）只把左键按下、松开转发给图标线程，别的键与滚轮让图标收起；
//! 认手势、看窗口、画图标都在图标线程上做，钩子回调里不碰别的窗口，免得拖慢全系统的输入。

mod gesture;
mod hover;
mod icon;
mod ocr;
mod point;
mod watch;

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicUsize, Ordering};

use qingjian_platform::HoverMode;

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    PostMessageW, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MOUSEHWHEEL, WM_MOUSEWHEEL,
    WM_RBUTTONDOWN, WM_XBUTTONDOWN,
};

/// 划词翻译开着没有。
static ENABLED: AtomicBool = AtomicBool::new(false);

/// 图标正显示着。
static VISIBLE: AtomicBool = AtomicBool::new(false);

/// 图标窗口的句柄；图标线程没起来时为 0。
static ICON: AtomicIsize = AtomicIsize::new(0);

/// 悬停取词方式（[`hover_code`] 的编码）：图标线程起来时按它开定时器，免得启动时发的消息落空。
static HOVER: AtomicUsize = AtomicUsize::new(0);

/// 按住 Ctrl 期间按过别的键（Ctrl+C 这类快捷键）：这次按 Ctrl 不算悬停取词。松开 Ctrl 时清掉。
static OTHER_KEY: AtomicBool = AtomicBool::new(false);

/// 起图标线程（进程启动时一次）；开关关着时线程闲着不占什么。
pub fn start() {
    icon::spawn();
}

/// 配置热加载时由钩子配置调；关掉时顺手收起图标。
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Release);
    if !on {
        hide();
    }
}

/// 配置热加载时由钩子配置调：告诉图标线程悬停取词开没开、用哪种方式。
pub fn set_hover(mode: HoverMode) {
    let code = hover_code(mode);
    HOVER.store(code, Ordering::Release);
    post(icon::MSG_HOVER, code, 0);
}

/// 悬停方式与消息参数互转。
fn hover_code(mode: HoverMode) -> usize {
    match mode {
        HoverMode::Off => 0,
        HoverMode::Ctrl => 1,
        HoverMode::Always => 2,
    }
}

fn hover_mode(code: usize) -> HoverMode {
    match code {
        1 => HoverMode::Ctrl,
        2 => HoverMode::Always,
        _ => HoverMode::Off,
    }
}

/// 键盘钩子：按了键就收起图标；按的不是 Ctrl 就记下，这次按 Ctrl 不算悬停取词。
pub fn key(vk: u32) {
    if !matches!(vk, 0x11 | 0xA2 | 0xA3) {
        OTHER_KEY.store(true, Ordering::Release);
    }
    hide();
}

/// 鼠标钩子：左键按下 / 松开交给图标线程认手势，别的键与滚轮收起图标。`time` 是系统给的毫秒时间。
pub fn mouse(message: u32, x: i32, y: i32, time: u32) {
    if !ENABLED.load(Ordering::Acquire) {
        return;
    }
    match message {
        WM_LBUTTONDOWN => post(icon::MSG_DOWN, time as usize, pack(x, y)),
        WM_LBUTTONUP => post(icon::MSG_UP, time as usize, pack(x, y)),
        WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN | WM_MOUSEWHEEL | WM_MOUSEHWHEEL => hide(),
        _ => {}
    }
}

fn hide() {
    if VISIBLE.load(Ordering::Acquire) {
        post(icon::MSG_HIDE, 0, 0);
    }
}

fn post(message: u32, w: usize, l: isize) {
    let hwnd = ICON.load(Ordering::Acquire);
    if hwnd != 0 {
        let _ = unsafe { PostMessageW(Some(HWND(hwnd as _)), message, WPARAM(w), LPARAM(l)) };
    }
}

/// 屏幕坐标装进一个 `LPARAM`（各 16 位，多屏时可以是负数），与 `MAKELPARAM` 一样。
fn pack(x: i32, y: i32) -> isize {
    ((y as i16 as u16 as isize) << 16) | (x as i16 as u16 as isize)
}

/// [`pack`] 的反操作。
fn unpack(l: isize) -> (i32, i32) {
    (
        i32::from(l as u16 as i16),
        i32::from((l >> 16) as u16 as i16),
    )
}

#[cfg(test)]
mod tests;
