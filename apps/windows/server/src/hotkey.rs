//! Windows 全局语音及翻译快捷键。物理修饰键始终成对放行。
//!
//! 低级键盘 / 鼠标钩子装在专用线程上：钩子回调必须在 LowLevelHooksTimeout（约 1 秒）内返回，
//! 超时多次后系统会静默卸掉钩子，放在要画候选窗的 UI 线程上，机器一卡快捷键就永久失灵。
use crate::dispatch::{TranslatorKeys, VoiceInsertResult};
use crate::ipc::Work;
use qingjian_platform::foreground::InputFocus;
use qingjian_platform::{KeyCombo, VoiceConfig, VoiceHotkey, global_hotkey};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc::Sender,
};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Threading::{
    GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_HIGHEST,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, MAPVK_VK_TO_VSC, MapVirtualKeyW,
    SendInput, VIRTUAL_KEY, VK_CONTROL, VK_LCONTROL, VK_LWIN, VK_MENU, VK_RCONTROL as RIGHT_CTRL,
    VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, HC_ACTION, KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG,
    SetWindowsHookExW, TranslateMessage, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN, WM_LBUTTONDOWN,
    WM_MBUTTONDOWN, WM_MOUSEHWHEEL, WM_MOUSEWHEEL, WM_RBUTTONDOWN, WM_SYSKEYDOWN, WM_XBUTTONDOWN,
};

pub static LISTENING: AtomicBool = AtomicBool::new(false);
static STATE: Mutex<Option<State>> = Mutex::new(None);
type Combo = (u32, u32);
struct State {
    work: Sender<Work>,
    voice: VoiceHotkey,
    hold: Duration,
    speak: Option<Combo>,
    translate: Option<Combo>,
    modifiers: u8,
    pending: Option<(u32, Instant)>,
    held_voice: Option<u32>,
    swallowed: Vec<u32>,
    /// 吞掉了带 Win / Alt 的组合主键：钩子返回后补发遮罩键，免得松开 Win 弹开始菜单、松开 Alt 激活菜单栏。
    mask: bool,
    /// 双击右 Ctrl 打开翻译窗口。
    double_ctrl: bool,
    /// 双击右 Shift 原地翻译。
    double_shift: bool,
    /// 这次单独按下的右 Ctrl / 右 Shift 及时刻；中途按了别的键就作废。
    tap_press: Option<(u32, Instant)>,
    /// 上一次「单独、快速」松开的右 Ctrl / 右 Shift 及时刻。
    last_tap: Option<(u32, Instant)>,
}

/// 右 Ctrl 的虚拟键码。
const VK_RCONTROL: u32 = 0xA3;
/// 右 Shift 的虚拟键码。
const VK_RSHIFT: u32 = 0xA1;
/// 按下到松开不超过它才算轻点。
const TAP_MAX: Duration = Duration::from_millis(300);
/// 上一次轻点松开到这一次按下不超过它才算双击。
const DOUBLE_GAP: Duration = Duration::from_millis(400);
fn combo(value: Option<KeyCombo>) -> Option<Combo> {
    let value = value?;
    let m = value.modifiers;
    let (mods, key) = global_hotkey::binding(value.key, m.control, m.shift, m.option, m.command)?;
    Some((mods.0, key))
}
impl State {
    fn configure(&mut self, voice: &VoiceConfig, translator: TranslatorKeys) {
        self.pending = None;
        if self.held_voice.take().is_some() {
            let _ = self.work.send(Work::VoiceCancel);
        }
        self.voice = voice.trigger();
        self.hold = Duration::from_millis(voice.hold_ms);
        self.speak = combo(voice.shortcut);
        self.translate = combo(translator.combo);
        self.double_ctrl = translator.double_ctrl;
        self.double_shift = translator.double_shift;
        self.tap_press = None;
        self.last_tap = None;
    }
    fn key(&mut self, vk: u32, down: bool, injected: bool, now: Instant) -> bool {
        if injected {
            return false;
        }
        if let Some(index) = self.swallowed.iter().position(|k| *k == vk) {
            if !down {
                self.swallowed.swap_remove(index);
                self.release(vk);
            }
            return true;
        }
        let physical = match vk {
            0xA4 | 0xA5 => 1 << (vk - 0xA4),
            0xA2 | 0xA3 => 4 << (vk - 0xA2),
            0xA0 | 0xA1 => 16 << (vk - 0xA0),
            0x5B | 0x5C => 64 << (vk - 0x5B),
            _ => 0,
        };
        if physical != 0 {
            // 按住说话时系统对触发键的自动连发不交给程序：连发的「Alt 按下」排在遮罩之后，Claude 这类
            // Electron 程序当成又单独按了一次 Alt，松开就进菜单模式，删除键、空格被菜单吃掉（2026-11-20 实测）
            if down && self.modifiers & physical != 0 && self.held_voice == Some(vk) {
                return true;
            }
            let tap_key = matches!(vk, VK_RCONTROL | VK_RSHIFT);
            if down && self.modifiers & physical == 0 {
                // 新按下的修饰键：只有「什么都没按着时按右 Ctrl / 右 Shift」才可能是轻点
                self.tap_press = (tap_key && self.modifiers == 0).then_some((vk, now));
                if !tap_key {
                    self.last_tap = None;
                }
            }
            if !down && tap_key {
                self.tap_released(vk, now);
            }
            if down {
                // 按住触发键时又按了别的修饰键（如 Win 后接 Shift 准备截图）：这是组合键，不是说话
                if self.modifiers & physical == 0 && self.pending.is_some_and(|(key, _)| key != vk)
                {
                    self.pending = None;
                }
                if self.modifiers & physical == 0
                    && matches!(
                        (self.voice, vk),
                        (VoiceHotkey::RightAlt, 0xA5)
                            | (VoiceHotkey::RightCtrl, 0xA3)
                            | (VoiceHotkey::Win, 0x5B | 0x5C)
                    )
                {
                    self.pending = Some((vk, now));
                }
                self.modifiers |= physical;
            } else {
                self.modifiers &= !physical;
                if self.pending.is_some_and(|(key, _)| key == vk) {
                    self.pending = None;
                }
                self.release(vk);
            }
            return false;
        }
        if !down {
            return false;
        }
        self.pending = None;
        self.tap_press = None;
        self.last_tap = None;
        let mods = (0..4).fold(0, |mask, i| {
            mask | (u32::from(self.modifiers & (3 << (2 * i)) != 0) << i)
        });
        let pressed = Some((mods, vk));
        let work = if vk == 0x1B && (LISTENING.load(Ordering::Acquire) || self.held_voice.is_some())
        {
            self.held_voice = None;
            Work::VoiceCancel
        } else if self.translate == pressed {
            self.mask = mods & (MOD_ALT | MOD_WIN) != 0;
            Work::OpenTranslator
        } else if self.voice == VoiceHotkey::Shortcut && self.speak == pressed {
            self.mask = mods & (MOD_ALT | MOD_WIN) != 0;
            self.held_voice = Some(vk);
            Work::VoiceKey { down: true }
        } else {
            return false;
        };
        self.swallowed.push(vk);
        let _ = self.work.send(work);
        true
    }
    /// 按住触发键期间点了鼠标或滚了滚轮（右 Ctrl + 点击多选、Ctrl + 滚轮缩放）：不是说话。
    fn mouse(&mut self) {
        self.pending = None;
        self.tap_press = None;
        self.last_tap = None;
    }
    /// 右 Ctrl / 右 Shift 松开：单独、快速、没触发语音的算一次轻点，同一个键两次轻点挨得够近就触发
    /// （右 Ctrl 开翻译窗口，右 Shift 原地翻译）。这两个键本身照常放行。
    fn tap_released(&mut self, vk: u32, now: Instant) {
        let voiced = self.held_voice == Some(vk);
        let tapped = self
            .tap_press
            .take()
            .filter(|(key, start)| *key == vk && !voiced && now.duration_since(*start) <= TAP_MAX);
        let Some((_, start)) = tapped else {
            self.last_tap = None;
            return;
        };
        let (enabled, work) = match vk {
            VK_RCONTROL => (self.double_ctrl, Work::OpenTranslator),
            _ => (self.double_shift, Work::TranslateInPlace),
        };
        if enabled
            && self.last_tap.is_some_and(|(key, previous)| {
                key == vk && start.duration_since(previous) <= DOUBLE_GAP
            })
        {
            self.last_tap = None;
            let _ = self.work.send(work);
        } else {
            self.last_tap = Some((vk, now));
        }
    }
    fn release(&mut self, vk: u32) {
        if self.held_voice == Some(vk) {
            self.held_voice = None;
            let _ = self.work.send(Work::VoiceKey { down: false });
        }
    }
    fn tick(&mut self, now: Instant) -> bool {
        if let Some((vk, start)) = self.pending
            && now.duration_since(start) >= self.hold
        {
            self.pending = None;
            self.held_voice = Some(vk);
            let _ = self.work.send(Work::VoiceKey { down: true });
            return true;
        }
        false
    }
}
/// 组合键修饰位，与 RegisterHotKey 的 MOD_* 一致（见 `global_hotkey::binding`）。
const MOD_ALT: u32 = 1;
const MOD_WIN: u32 = 8;

/// 装钩子：配置放进全局状态，钩子在专用线程上装并跑消息循环（低级钩子靠装它的线程取消息才会被调用）。
pub fn install(work: Sender<Work>, voice: &VoiceConfig, translator: TranslatorKeys) {
    let mut state = State {
        work,
        voice: VoiceHotkey::None,
        hold: Duration::ZERO,
        speak: None,
        translate: None,
        modifiers: 0,
        pending: None,
        held_voice: None,
        swallowed: Vec::new(),
        mask: false,
        double_ctrl: false,
        double_shift: false,
        tap_press: None,
        last_tap: None,
    };
    state.configure(voice, translator);
    tracing::info!(
        trigger = ?state.voice,
        hold_ms = state.hold.as_millis() as u64,
        translator = state.translate.is_some(),
        double_ctrl = state.double_ctrl,
        double_shift = state.double_shift,
        "global shortcut configured"
    );
    *STATE.lock().unwrap() = Some(state);
    let spawned = std::thread::Builder::new()
        .name("qingjian-hotkey".to_owned())
        .spawn(|| unsafe {
            // 钩子回调慢了所有程序的键盘都会卡，这条线程只做这件事，优先级调高
            let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_HIGHEST);
            if let Err(error) = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook), None, 0) {
                tracing::warn!(%error, "global shortcut hook unavailable");
                return;
            }
            if let Err(error) = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), None, 0) {
                tracing::warn!(%error, "mouse hook unavailable; clicks will not cancel a pending hold");
            }
            tracing::info!("global shortcut hook installed");
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, "cannot spawn global shortcut thread");
    }
}
pub fn reconfigure(voice: &VoiceConfig, translator: TranslatorKeys) {
    if let Some(state) = STATE.lock().unwrap().as_mut() {
        state.configure(voice, translator);
    }
}
unsafe extern "system" fn hook(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let k = unsafe { &*(l.0 as *const KBDLLHOOKSTRUCT) };
        let (swallow, mask) = STATE.lock().unwrap().as_mut().map_or((false, false), |s| {
            let swallow = s.key(
                k.vkCode,
                w.0 == WM_KEYDOWN as usize || w.0 == WM_SYSKEYDOWN as usize,
                k.flags.contains(LLKHF_INJECTED),
                Instant::now(),
            );
            (swallow, std::mem::take(&mut s.mask))
        });
        // 锁放开后再注入：注入的键会再进一次本钩子
        if mask {
            send_mask();
        }
        if swallow {
            return LRESULT(1);
        }
    }
    unsafe { CallNextHookEx(None, code, w, l) }
}
unsafe extern "system" fn mouse_hook(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32
        && matches!(
            w.0 as u32,
            WM_LBUTTONDOWN
                | WM_RBUTTONDOWN
                | WM_MBUTTONDOWN
                | WM_XBUTTONDOWN
                | WM_MOUSEWHEEL
                | WM_MOUSEHWHEEL
        )
        && let Some(state) = STATE.lock().unwrap().as_mut()
    {
        state.mouse();
    }
    unsafe { CallNextHookEx(None, code, w, l) }
}
pub fn tick_hold() {
    let fired = STATE
        .lock()
        .unwrap()
        .as_mut()
        .is_some_and(|s| s.tick(Instant::now()));
    if fired {
        send_mask();
    }
}

/// 补发遮罩键：系统看到「修饰键之间夹了别的键」，松开 Win 不弹开始菜单、松开 Alt 不激活菜单栏。
///
/// 未分配的虚拟键 0xE8 管得住开始菜单与经典菜单栏，管不住新版记事本这类 WinUI 程序的访问键模式（实测），
/// Alt 按着时再补按一下没按着的那侧 Ctrl。与 DLL 的 `com::key::mask` 同一做法；注入的键本钩子不理。
fn send_mask() {
    let held = |key: VIRTUAL_KEY| unsafe { GetAsyncKeyState(key.0.into()) } < 0;
    let mut inputs = tap(VIRTUAL_KEY(0xe8)).to_vec();
    if held(VK_MENU) {
        let ctrl = if held(VK_LCONTROL) {
            RIGHT_CTRL
        } else {
            VK_LCONTROL
        };
        inputs.extend(tap(ctrl));
    }
    unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
}

/// 按下 + 抬起一个键，带扫描码（Flutter 等按扫描码认键）；右 Ctrl 是扩展键。
fn tap(key: VIRTUAL_KEY) -> [INPUT; 2] {
    let scan = unsafe { MapVirtualKeyW(u32::from(key.0), MAPVK_VK_TO_VSC) } as u16;
    let extended = if key == RIGHT_CTRL {
        KEYEVENTF_EXTENDEDKEY
    } else {
        KEYBD_EVENT_FLAGS(0)
    };
    let input = |flags: KEYBD_EVENT_FLAGS| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                wScan: scan,
                dwFlags: flags,
                ..Default::default()
            },
        },
    };
    [input(extended), input(extended | KEYEVENTF_KEYUP)]
}

/// 旧 DLL 未取走待上屏文本时，向仍持有焦点的窗口投递 Unicode。全段作为一批键消息，避免分段后焦点转移。
pub fn insert_voice(target: InputFocus, text: &str) -> VoiceInsertResult {
    if [VK_CONTROL, VK_SHIFT, VK_MENU, VK_LWIN, VK_RWIN]
        .iter()
        .any(|key| unsafe { GetAsyncKeyState(key.0.into()) } < 0)
    {
        return VoiceInsertResult::Busy;
    }
    let keys: Vec<_> = text
        .encode_utf16()
        .flat_map(|unit| {
            [KEYEVENTF_UNICODE, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP].map(|flags| INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wScan: unit,
                        dwFlags: flags,
                        ..Default::default()
                    },
                },
            })
        })
        .collect();
    if InputFocus::current() != Some(target) {
        return VoiceInsertResult::Failed;
    }
    let written = unsafe { SendInput(&keys, size_of::<INPUT>() as i32) };
    if written as usize == keys.len() {
        VoiceInsertResult::Sent
    } else {
        tracing::warn!(
            written,
            expected = keys.len(),
            "Unicode input delivery failed"
        );
        VoiceInsertResult::Failed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;
    /// 需要交互桌面。只在本测试拥有前台焦点的 EDIT 内发送，退出即销毁。
    #[test]
    #[ignore = "requires an interactive Windows desktop"]
    fn unicode_delivery_reaches_the_native_caret() {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
        use windows::Win32::UI::WindowsAndMessaging::*;
        use windows::core::w;
        struct TestWindow(HWND);
        impl Drop for TestWindow {
            fn drop(&mut self) {
                let _ = unsafe { DestroyWindow(self.0) };
            }
        }
        unsafe {
            let edit = TestWindow(
                CreateWindowExW(
                    Default::default(),
                    w!("EDIT"),
                    w!("AB"),
                    WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                    50,
                    50,
                    230,
                    90,
                    None,
                    None,
                    None,
                    None,
                )
                .unwrap(),
            );
            let _ = SetForegroundWindow(edit.0);
            let _ = SetFocus(Some(edit.0));
            let target = InputFocus::current().expect("interactive keyboard focus");
            assert_eq!(
                target.window, edit.0.0 as isize,
                "test window must own focus before sending"
            );
            SendMessageW(edit.0, 0xb1, Some(WPARAM(1)), Some(LPARAM(1)));
            assert_eq!(insert_voice(target, "语音🙂"), VoiceInsertResult::Sent);
            let until = Instant::now() + Duration::from_secs(2);
            let mut msg = MSG::default();
            let mut result = String::new();
            while Instant::now() < until {
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                let mut buf = [0; 32];
                let len = GetWindowTextW(edit.0, &mut buf);
                result = String::from_utf16_lossy(&buf[..len as usize]);
                if result == "A语音🙂B" {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            assert_eq!(result, "A语音🙂B");
        }
    }
    #[test]
    fn voice_trigger_flow() {
        let (work, rx) = channel();
        let mut s = State {
            work,
            voice: VoiceHotkey::RightAlt,
            hold: Duration::from_millis(400),
            speak: None,
            translate: Some((3, 70)),
            modifiers: 0,
            pending: None,
            held_voice: None,
            swallowed: vec![],
            mask: false,
            double_ctrl: true,
            double_shift: true,
            tap_press: None,
            last_tap: None,
        };
        let now = Instant::now();
        let later = now + s.hold;
        assert!(!s.key(0xa5, true, true, now));
        assert!(!s.tick(later));
        assert!(!s.key(0xa5, true, false, now));
        assert!(!s.key(0xa5, false, false, now));
        assert!(!s.tick(later));
        s.key(0xa5, true, false, now);
        s.key(9, true, false, now);
        assert!(!s.tick(later));
        s.key(0xa5, false, false, now);
        s.key(0xa5, true, false, now);
        assert!(s.tick(later));
        assert!(!s.tick(later));
        assert!(matches!(rx.try_recv(), Ok(Work::VoiceKey { down: true })));
        // 说话期间触发键的自动连发吞掉，松开照常放行
        assert!(s.key(0xa5, true, false, later));
        assert!(s.key(0xa5, true, false, later));
        assert!(!s.key(0xa5, false, false, later));
        assert!(matches!(rx.try_recv(), Ok(Work::VoiceKey { down: false })));
        s.key(0xa5, true, false, now);
        s.key(0xa2, true, false, now);
        assert!(s.key(70, true, false, now));
        assert!(s.key(70, true, false, now));
        assert!(s.key(70, false, false, now));
        assert!(matches!(rx.try_recv(), Ok(Work::OpenTranslator)));
        LISTENING.store(true, Ordering::Release);
        assert!(s.key(27, true, false, now));
        assert!(s.key(27, false, false, now));
        assert!(matches!(rx.try_recv(), Ok(Work::VoiceCancel)));
        LISTENING.store(false, Ordering::Release);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn hold_is_voided_by_other_modifiers_and_mouse_and_win_combos_are_masked() {
        let (work, rx) = channel();
        let mut s = State {
            work,
            voice: VoiceHotkey::Win,
            hold: Duration::from_millis(400),
            speak: None,
            translate: Some((MOD_WIN | 4, 70)),
            modifiers: 0,
            pending: None,
            held_voice: None,
            swallowed: vec![],
            mask: false,
            double_ctrl: true,
            double_shift: true,
            tap_press: None,
            last_tap: None,
        };
        let now = Instant::now();
        let later = now + s.hold;
        // Win 按住后加 Shift：作废；Win 自动重复不算新按下
        s.key(0x5b, true, false, now);
        s.key(0x5b, true, false, now);
        assert!(s.pending.is_some());
        s.key(0xa0, true, false, now);
        assert!(!s.tick(later));
        s.key(0xa0, false, false, now);
        s.key(0x5b, false, false, now);
        // 按住时点鼠标：作废
        s.key(0x5b, true, false, now);
        s.mouse();
        assert!(!s.tick(later));
        // Win + Shift + F 命中翻译组合：吞掉主键，并要求补发遮罩键
        s.key(0xa0, true, false, now);
        assert!(s.key(70, true, false, now));
        assert!(std::mem::take(&mut s.mask));
        assert!(s.key(70, false, false, now));
        assert!(matches!(rx.try_recv(), Ok(Work::OpenTranslator)));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn double_tap_right_ctrl_opens_the_translator() {
        let (work, rx) = channel();
        let mut s = State {
            work,
            voice: VoiceHotkey::RightAlt,
            hold: Duration::from_millis(400),
            speak: None,
            translate: None,
            modifiers: 0,
            pending: None,
            held_voice: None,
            swallowed: vec![],
            mask: false,
            double_ctrl: true,
            double_shift: true,
            tap_press: None,
            last_tap: None,
        };
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        let tap = |s: &mut State, down: u64, up: u64| {
            assert!(
                !s.key(VK_RCONTROL, true, false, at(down)),
                "right Ctrl is never swallowed"
            );
            assert!(!s.key(VK_RCONTROL, false, false, at(up)));
        };
        // 两下快速轻点：打开
        tap(&mut s, 0, 80);
        tap(&mut s, 200, 260);
        assert!(matches!(rx.try_recv(), Ok(Work::OpenTranslator)));
        // 第三下不连着算
        tap(&mut s, 330, 380);
        assert!(rx.try_recv().is_err());
        // 间隔太长：不算
        tap(&mut s, 2_000, 2_050);
        tap(&mut s, 2_600, 2_650);
        assert!(rx.try_recv().is_err());
        // 按太久（Ctrl+滚轮之类）：不算
        tap(&mut s, 4_000, 4_500);
        tap(&mut s, 4_600, 4_650);
        assert!(rx.try_recv().is_err());
        // 中间按了别的键（Ctrl+C 后再按 Ctrl）：不算
        tap(&mut s, 6_000, 6_050);
        s.key(0x43, true, false, at(6_100));
        s.key(0x43, false, false, at(6_120));
        tap(&mut s, 6_200, 6_250);
        assert!(rx.try_recv().is_err());
        // 中间点了鼠标：不算
        tap(&mut s, 8_000, 8_050);
        s.mouse();
        tap(&mut s, 8_200, 8_250);
        assert!(rx.try_recv().is_err());
        // 按着别的修饰键时的右 Ctrl：不算
        s.key(0xA0, true, false, at(10_000));
        tap(&mut s, 10_010, 10_050);
        tap(&mut s, 10_100, 10_150);
        s.key(0xA0, false, false, at(10_200));
        assert!(rx.try_recv().is_err());
        // 关掉开关：不算
        s.double_ctrl = false;
        tap(&mut s, 12_000, 12_050);
        tap(&mut s, 12_200, 12_250);
        assert!(rx.try_recv().is_err());
        // 右 Shift 双击：原地翻译；右 Ctrl 接右 Shift 不算双击
        let shift = |s: &mut State, down: u64, up: u64| {
            assert!(
                !s.key(VK_RSHIFT, true, false, at(down)),
                "right Shift is never swallowed"
            );
            assert!(!s.key(VK_RSHIFT, false, false, at(up)));
        };
        shift(&mut s, 14_000, 14_050);
        shift(&mut s, 14_200, 14_250);
        assert!(matches!(rx.try_recv(), Ok(Work::TranslateInPlace)));
        s.double_ctrl = true;
        tap(&mut s, 16_000, 16_050);
        shift(&mut s, 16_200, 16_250);
        assert!(rx.try_recv().is_err());
        s.double_shift = false;
        shift(&mut s, 18_000, 18_050);
        shift(&mut s, 18_200, 18_250);
        assert!(rx.try_recv().is_err());
    }
}
