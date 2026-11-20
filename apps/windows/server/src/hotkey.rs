//! Windows 全局语音及翻译快捷键。物理修饰键始终成对放行。
//!
//! 低级键盘 / 鼠标钩子装在专用线程上：钩子回调必须在 LowLevelHooksTimeout（约 1 秒）内返回，
//! 超时多次后系统会静默卸掉钩子，放在要画候选窗的 UI 线程上，机器一卡快捷键就永久失灵。
use crate::dispatch::VoiceInsertResult;
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
    GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
    KEYEVENTF_UNICODE, SendInput, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
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
}
fn combo(value: Option<KeyCombo>) -> Option<Combo> {
    let value = value?;
    let m = value.modifiers;
    let (mods, key) = global_hotkey::binding(value.key, m.control, m.shift, m.option, m.command)?;
    Some((mods.0, key))
}
impl State {
    fn configure(&mut self, voice: &VoiceConfig, translator: Option<KeyCombo>) {
        self.pending = None;
        if self.held_voice.take().is_some() {
            let _ = self.work.send(Work::VoiceCancel);
        }
        self.voice = voice.trigger();
        self.hold = Duration::from_millis(voice.hold_ms);
        self.speak = combo(voice.shortcut);
        self.translate = combo(translator);
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
pub fn install(work: Sender<Work>, voice: &VoiceConfig, translator: Option<KeyCombo>) {
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
    };
    state.configure(voice, translator);
    tracing::info!(
        trigger = ?state.voice,
        hold_ms = state.hold.as_millis() as u64,
        translator = state.translate.is_some(),
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
pub fn reconfigure(voice: &VoiceConfig, translator: Option<KeyCombo>) {
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

/// 补发一次未分配的虚拟键 0xE8（按下 + 抬起）：系统看到「修饰键之间夹了别的键」，
/// 松开 Alt 不激活菜单栏、松开 Win 不弹开始菜单、松开 Ctrl 不被当成单击。
fn send_mask() {
    {
        let mut key = KEYBDINPUT {
            wVk: VIRTUAL_KEY(0xe8),
            ..Default::default()
        };
        let down = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 { ki: key },
        };
        key.dwFlags = KEYEVENTF_KEYUP;
        let up = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 { ki: key },
        };
        unsafe { SendInput(&[down, up], size_of::<INPUT>() as i32) };
    }
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
}
