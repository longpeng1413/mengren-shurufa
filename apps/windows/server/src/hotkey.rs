//! 全局语音热键：低级键盘钩子（WH_KEYBOARD_LL）检测两种触发，通过 [`Work::VoiceKey`] 交给 Router。
//!
//! - 缺省长按 Win 键：`hold_ms` 内松开是短按，照常打开开始菜单；按住超过阈值才触发语音，
//!   并吞掉这次的 Win 抬起（否则开始菜单会弹出来盖住语音面板）。再按 Esc 或松开结束。
//! - 自定义组合键（如 Ctrl+Alt+V）：按下开始、松开结束；组合键用配置的修饰键 + 一个主键。
//!
//! 钩子装在 Server 的 UI 线程（它有消息循环，低级钩子的回调就在这条线程上跑），回调用原子状态，
//! 只做「置标志 + 发一条 Work」，不做识别等重活。开始菜单 / 系统快捷键（Win+R 等）在短按与组合未命中时
//! 一律放行，只在「长按 Win 已触发语音」或「自定义键命中」时吞按键。

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU8, Ordering};
use std::sync::mpsc::Sender;
use std::time::{SystemTime, UNIX_EPOCH};

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    VK_CONTROL, VK_ESCAPE, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU, VK_RCONTROL,
    VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, HC_ACTION, KBDLLHOOKSTRUCT, SetWindowsHookExW, WH_KEYBOARD_LL, WM_KEYDOWN,
    WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use crate::ipc::Work;

/// 触发方式。由 `install` 注入，之后只读；热更新时重新 install。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Trigger {
    /// 长按左右 Win 键触发。
    Win { hold_ms: u32 },
    /// 自定义：修饰键掩码（见 `MOD_*`）+ 主键虚拟键。
    Combo { mods: u8, key: u32 },
}

const MOD_CONTROL: u8 = 1;
const MOD_SHIFT: u8 = 2;
const MOD_ALT: u8 = 4;

/// 钩子全局状态。回调与 install 线程共享，全部原子或一次性写入。
struct State {
    trigger: Trigger,
    /// 当前修饰键掩码（钩子收到 keydown/keyup 时更新）。
    mods: AtomicU8,
    /// Win 键按下的时刻（毫秒时间戳）；0 表示没按下。
    win_down_ms: AtomicI64,
    /// 长按是否已经触发（防止重复 start，且用它决定 keyup 要不要吞）。
    win_fired: AtomicBool,
    /// 自定义组合键是否正在按住（松开时 stop）。
    combo_down: AtomicBool,
    /// 发给工人线程的通道。
    work: Sender<Work>,
}

// 全局单例：钩子回调是裸函数指针，只能经静态状态取上下文。
static STATE: Mutex<Option<Box<State>>> = Mutex::new(None);
// 钩子句柄进程生命周期内不卸载（Server 常驻），装好后直接丢弃句柄；HHOOK 原始指针不跨线程存。

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// 安装全局热键钩子。`hold_ms` 为长按 Win 的阈值；`combo` 为自定义组合键（`None` 用长按 Win）。
/// 返回值只是「是否装上」，失败时语音热键不可用，但输入法其余功能照常。
pub fn install(work: Sender<Work>, hold_ms: u32, combo: Option<(u8, u32)>) -> bool {
    let trigger = match combo {
        Some((mods, key)) if mods != 0 => Trigger::Combo { mods, key },
        _ => Trigger::Win { hold_ms },
    };
    *STATE.lock().unwrap() = Some(Box::new(State {
        trigger,
        mods: AtomicU8::new(0),
        win_down_ms: AtomicI64::new(0),
        win_fired: AtomicBool::new(false),
        combo_down: AtomicBool::new(false),
        work,
    }));
    // 低级钩子回调在装钩子的线程消息循环上跑；Server 的 UI 线程有循环（见 ui::run）。
    let hook = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), None, 0) };
    match hook {
        Ok(_handle) => {
            // 句柄不卸载，随进程存活；这里不保存，免得裸指针跨线程。
            tracing::info!(?trigger, "语音热键钩子已安装");
            true
        }
        Err(error) => {
            tracing::warn!(%error, "安装语音热键钩子失败");
            false
        }
    }
}

/// 从配置构造钩子的自定义组合键参数：`(修饰键掩码, 主键虚拟键)`。`None` 表示用长按 Win。
pub fn combo_from_config(combo: Option<qingjian_platform::KeyCombo>) -> Option<(u8, u32)> {
    let combo = combo?;
    let mut mods = 0u8;
    if combo.modifiers.control {
        mods |= MOD_CONTROL;
    }
    if combo.modifiers.shift {
        mods |= MOD_SHIFT;
    }
    if combo.modifiers.option {
        mods |= MOD_ALT;
    }
    if mods == 0 {
        return None; // 至少要一个修饰键，否则和正常打字冲突
    }
    let key = if combo.key == '.' {
        0xBE // VK_OEM_PERIOD
    } else {
        combo.key.to_ascii_uppercase() as u32
    };
    Some((mods, key))
}

/// 钩子回调：只更新原子状态、按需发 Work，耗时识别都在工人线程。
unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let action = code == HC_ACTION as i32;
    let mut swallow = false;
    if action {
        // SAFETY: HC_ACTION 时 lparam 指向系统保证有效的 KBDLLHOOKSTRUCT。
        let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        let vk = info.vkCode;
        let down = wparam.0 == WM_KEYDOWN as usize || wparam.0 == WM_SYSKEYDOWN as usize;
        let up = wparam.0 == WM_KEYUP as usize || wparam.0 == WM_SYSKEYUP as usize;
        if down || up {
            swallow = handle_key(vk, down, up);
        }
    }
    if swallow {
        LRESULT(1)
    } else {
        // SAFETY: 钩子链参数由系统调用原样传入。
        unsafe { CallNextHookEx(None, code, wparam, lparam) }
    }
}

/// 处理一次按键，返回是否吞掉（阻止继续传给系统）。
fn handle_key(vk: u32, down: bool, up: bool) -> bool {
    let guard = STATE.lock().unwrap();
    let state = match guard.as_ref() {
        Some(state) => state,
        None => return false,
    };

    // 维护修饰键掩码。左右键都归一到同一位。
    if let Some(bit) = modifier_bit(vk) {
        let current = state.mods.load(Ordering::Relaxed);
        state.mods.store(
            if down { current | bit } else { current & !bit },
            Ordering::Relaxed,
        );
    }

    match state.trigger {
        Trigger::Win { .. } => handle_win(state, vk, down, up),
        Trigger::Combo { mods, key } => handle_combo(state, vk, down, up, mods, key),
    }
}

/// 长按 Win：短按放行（开始菜单照常），按住到阈值才触发并吞掉这次抬起。
fn handle_win(state: &State, vk: u32, down: bool, up: bool) -> bool {
    let is_win = vk == u32::from(VK_LWIN.0) || vk == u32::from(VK_RWIN.0);
    if is_win {
        if down {
            if state.win_down_ms.load(Ordering::Relaxed) == 0 {
                state.win_down_ms.store(now_ms(), Ordering::Relaxed);
                state.win_fired.store(false, Ordering::Relaxed);
            }
            return false; // 放行 keydown；开始菜单是 keyup 时弹的
        }
        if up {
            if state.win_fired.swap(false, Ordering::Relaxed) {
                state.win_down_ms.store(0, Ordering::Relaxed);
                let _ = state.work.send(Work::VoiceKey { down: false });
                return true; // 吞掉抬键，别让开始菜单弹出来
            }
            state.win_down_ms.store(0, Ordering::Relaxed); // 短按：放行，开始菜单照常
            return false;
        }
    }
    // Win 长按期间按 Esc：取消这段语音（抬起时不再吞，开始菜单也不会弹，因为 fired 清了）。
    if down && vk == u32::from(VK_ESCAPE.0) && state.win_fired.swap(false, Ordering::Relaxed) {
        state.win_down_ms.store(0, Ordering::Relaxed);
        let _ = state.work.send(Work::VoiceKey { down: false });
    }
    false
}

/// 自定义组合键：修饰键全按下 + 主键按下 → 开始；主键松开 → 结束。
fn handle_combo(state: &State, vk: u32, down: bool, up: bool, mods: u8, key: u32) -> bool {
    if vk == key
        && down
        && (state.mods.load(Ordering::Relaxed) & mods) == mods
        && !state.combo_down.swap(true, Ordering::Relaxed)
    {
        let _ = state.work.send(Work::VoiceKey { down: true });
        return true;
    }
    if vk == key && up && state.combo_down.swap(false, Ordering::Relaxed) {
        let _ = state.work.send(Work::VoiceKey { down: false });
        return true;
    }
    false
}

/// 虚拟键对应的修饰键位；非修饰键为 `None`。
fn modifier_bit(vk: u32) -> Option<u8> {
    match vk {
        x if x == u32::from(VK_CONTROL.0)
            || x == u32::from(VK_LCONTROL.0)
            || x == u32::from(VK_RCONTROL.0) =>
        {
            Some(MOD_CONTROL)
        }
        x if x == u32::from(VK_SHIFT.0)
            || x == u32::from(VK_LSHIFT.0)
            || x == u32::from(VK_RSHIFT.0) =>
        {
            Some(MOD_SHIFT)
        }
        x if x == u32::from(VK_MENU.0)
            || x == u32::from(VK_LMENU.0)
            || x == u32::from(VK_RMENU.0) =>
        {
            Some(MOD_ALT)
        }
        _ => None,
    }
}

/// 供计时线程调用：周期性检查长按是否到点（用户按住 Win 不动时钩子不会被触发，得靠这里推进）。
pub fn tick_hold() {
    let guard = STATE.lock().unwrap();
    let state = match guard.as_ref() {
        Some(state) => state,
        None => return,
    };
    if let Trigger::Win { hold_ms } = state.trigger {
        let down_at = state.win_down_ms.load(Ordering::Relaxed);
        if down_at != 0
            && !state.win_fired.load(Ordering::Relaxed)
            && now_ms() - down_at >= i64::from(hold_ms)
        {
            state.win_fired.store(true, Ordering::Relaxed);
            let _ = state.work.send(Work::VoiceKey { down: true });
        }
    }
}

/// 测试用：清掉全局状态（每个测试开头调，避免互相污染）。
#[cfg(test)]
pub(crate) fn reset_for_test() {
    *STATE.lock().unwrap() = None;
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU8};
    use std::sync::mpsc::{Receiver, Sender, channel};

    use windows::Win32::UI::Input::KeyboardAndMouse::{VK_CONTROL, VK_LWIN};

    use super::{
        MOD_CONTROL, MOD_SHIFT, STATE, State, Trigger, handle_key, modifier_bit, reset_for_test,
        tick_hold,
    };
    use crate::ipc::Work;

    fn setup(trigger: Trigger, work: Sender<Work>) {
        *STATE.lock().unwrap() = Some(Box::new(State {
            trigger,
            mods: AtomicU8::new(0),
            win_down_ms: AtomicI64::new(0),
            win_fired: AtomicBool::new(false),
            combo_down: AtomicBool::new(false),
            work,
        }));
    }

    /// 取下一条语音 Work，只关心 down 值；没有消息返回 None。
    fn voice(rx: &Receiver<Work>) -> Option<bool> {
        match rx.try_recv() {
            Ok(Work::VoiceKey { down }) => Some(down),
            _ => None,
        }
    }

    #[test]
    fn modifier_vks_normalize() {
        assert_eq!(modifier_bit(0xA2u32), Some(MOD_CONTROL)); // LCONTROL
        assert_eq!(modifier_bit(0xA0u32), Some(MOD_SHIFT)); // LSHIFT
        assert_eq!(modifier_bit(b'A' as u32), None);
    }

    #[test]
    fn combo_needs_a_modifier() {
        // 单个字母没有修饰键：combo_from_config 会拒绝（解析本身也会失败，退回默认组合）。
        assert!(super::combo_from_config(None).is_none());
        let with_mod: qingjian_platform::KeyCombo = "ctrl+alt+v".parse().unwrap();
        assert!(super::combo_from_config(Some(with_mod)).is_some());
    }

    /// 触发状态机（共用全局 STATE，必须串行放一个测试里，避免并行测试互相 reset 污染）。
    #[test]
    fn voice_trigger_flow() {
        // 长按 Win：到点触发一次 start，松手吞掉抬键并发 stop。
        {
            reset_for_test();
            let (tx, rx) = channel();
            setup(Trigger::Win { hold_ms: 0 }, tx);
            let win = u32::from(VK_LWIN.0);
            assert!(!handle_key(win, true, false)); // keydown 放行
            tick_hold();
            assert_eq!(voice(&rx), Some(true)); // 到点触发
            tick_hold();
            assert_eq!(voice(&rx), None); // 不重复触发
            assert!(handle_key(win, false, true)); // 松手吞掉
            assert_eq!(voice(&rx), Some(false));
        }
        // 短按 Win：没到阈值就松手，不触发、不吞键（开始菜单照常）。
        {
            reset_for_test();
            let (tx, rx) = channel();
            setup(Trigger::Win { hold_ms: 60_000 }, tx);
            let win = u32::from(VK_LWIN.0);
            handle_key(win, true, false);
            tick_hold();
            assert!(!handle_key(win, false, true));
            assert_eq!(voice(&rx), None);
        }
        // 自定义组合键：修饰键 + 主键触发，主键松开结束。
        {
            reset_for_test();
            let (tx, rx) = channel();
            setup(
                Trigger::Combo {
                    mods: MOD_CONTROL,
                    key: b'V' as u32,
                },
                tx,
            );
            let ctrl = u32::from(VK_CONTROL.0);
            assert!(!handle_key(ctrl, true, false)); // 先按修饰键，放行
            assert!(handle_key(b'V' as u32, true, false)); // 主键触发并吞掉
            assert_eq!(voice(&rx), Some(true));
            assert!(handle_key(b'V' as u32, false, true)); // 主键松开结束
            assert_eq!(voice(&rx), Some(false));
        }
        reset_for_test();
    }
}
