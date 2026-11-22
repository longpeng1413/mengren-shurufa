//! 悬停取词的运行部分：图标线程上定时采样鼠标与按键，停住了就在工作线程里取鼠标下的词，
//! 请翻译窗口程序在词下方弹不抢焦点的卡片（`--card x,y --hover`）。

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use qingjian_platform::HoverMode;
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VIRTUAL_KEY, VK_CONTROL, VK_LBUTTON, VK_MBUTTON, VK_RBUTTON,
};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetCursorPos, IsWindowVisible, KillTimer, SetTimer,
};
use windows::core::w;

use super::OTHER_KEY;
use super::hover::{Dwell, Sample};
use super::icon::{eligible_window, root_at};

/// 悬停采样定时器的编号（图标自己收起用 1）。
pub(super) const HOVER_TIMER: usize = 2;

/// 多久采样一次（毫秒）。
const PERIOD: u32 = 80;

/// 正在取词：取词可能被卡住的程序拖住，同时只取一个。
static BUSY: AtomicBool = AtomicBool::new(false);

/// 上一张悬停卡片的词与位置：卡片还开着时同一个词不再弹。
static LAST: Mutex<Option<(String, RECT)>> = Mutex::new(None);

/// 按新方式开关采样定时器。
pub(super) fn apply(hwnd: HWND, dwell: &mut Dwell, mode: HoverMode) {
    dwell.set_mode(mode);
    unsafe {
        if mode == HoverMode::Off {
            let _ = KillTimer(Some(hwnd), HOVER_TIMER);
        } else {
            SetTimer(Some(hwnd), HOVER_TIMER, PERIOD, None);
        }
    }
}

/// 采样一次；该取词时开工作线程去取，回 `true`。
pub(super) fn tick(dwell: &mut Dwell) -> bool {
    let mut cursor = POINT::default();
    if unsafe { GetCursorPos(&mut cursor) }.is_err() {
        return false;
    }
    let down =
        |key: VIRTUAL_KEY| unsafe { GetAsyncKeyState(i32::from(key.0)) } as u16 & 0x8000 != 0;
    let ctrl = down(VK_CONTROL);
    if !ctrl {
        OTHER_KEY.store(false, Ordering::Release);
    }
    let sample = Sample {
        x: cursor.x,
        y: cursor.y,
        ctrl: ctrl && !OTHER_KEY.load(Ordering::Acquire),
        button: down(VK_LBUTTON) || down(VK_RBUTTON) || down(VK_MBUTTON),
    };
    match dwell.tick(sample, Instant::now()) {
        Some((x, y)) => start(x, y),
        None => false,
    }
}

fn start(x: i32, y: i32) -> bool {
    if !eligible_window(root_at(x, y)) || BUSY.swap(true, Ordering::AcqRel) {
        return false;
    }
    let spawned = std::thread::Builder::new()
        .name("qingjian-hover-word".to_owned())
        .spawn(move || {
            let found = super::point::word_at(x, y);
            BUSY.store(false, Ordering::Release);
            if let Some((word, rect)) = found {
                show(word, rect);
            }
        });
    if spawned.is_err() {
        BUSY.store(false, Ordering::Release);
        return false;
    }
    true
}

fn show(word: String, rect: RECT) {
    let mut last = LAST.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let key = (word, rect);
    if card_visible() && last.as_ref() == Some(&key) {
        return;
    }
    let (word, rect) = key.clone();
    *last = Some(key);
    drop(last);
    tracing::debug!(chars = word.chars().count(), "hover word found");
    crate::ui::launch_translator(
        &[
            "--card".to_owned(),
            format!("{},{}", rect.left, rect.bottom),
            "--hover".to_owned(),
        ],
        Some(word),
    );
}

fn card_visible() -> bool {
    unsafe { FindWindowW(w!("Mengren.Translator.Card"), None) }
        .is_ok_and(|card| unsafe { IsWindowVisible(card) }.as_bool())
}
