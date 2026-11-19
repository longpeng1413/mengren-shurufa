//! Windows 全局语音及翻译快捷键。物理修饰键始终成对放行。
use crate::ipc::Work;
use qingjian_platform::{KeyCombo, VoiceConfig, VoiceHotkey, global_hotkey};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc::Sender,
};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, HC_ACTION, KBDLLHOOKSTRUCT, LLKHF_INJECTED, SetWindowsHookExW, WH_KEYBOARD_LL,
    WM_KEYDOWN, WM_SYSKEYDOWN,
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
            Work::OpenTranslator
        } else if self.voice == VoiceHotkey::Shortcut && self.speak == pressed {
            self.held_voice = Some(vk);
            Work::VoiceKey { down: true }
        } else {
            return false;
        };
        self.swallowed.push(vk);
        let _ = self.work.send(work);
        true
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
    };
    state.configure(voice, translator);
    *STATE.lock().unwrap() = Some(state);
    if let Err(error) = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook), None, 0) } {
        tracing::warn!(%error,"global shortcut hook unavailable");
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
        if STATE.lock().unwrap().as_mut().is_some_and(|s| {
            s.key(
                k.vkCode,
                w.0 == WM_KEYDOWN as usize || w.0 == WM_SYSKEYDOWN as usize,
                k.flags.contains(LLKHF_INJECTED),
                Instant::now(),
            )
        }) {
            return LRESULT(1);
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;
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
}
