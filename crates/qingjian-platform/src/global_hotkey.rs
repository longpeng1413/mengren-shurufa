//! Windows 全局快捷键的物理键与符号映射，两处注册入口共用当前键盘布局。

use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN, VkKeyScanW,
};

pub fn recorded_key(key: char, shift: bool) -> char {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyboardLayout, MAPVK_VK_TO_VSC, MapVirtualKeyW, ToUnicodeEx,
    };
    if !shift || key.is_ascii_alphabetic() {
        return key;
    }
    let Some((_, code)) = binding(key, false, false, false, false) else {
        return key;
    };
    let mut state = [0u8; 256];
    state[0x10] = 0x80;
    let mut text = [0u16; 8];
    let length = unsafe {
        ToUnicodeEx(
            code,
            MapVirtualKeyW(code, MAPVK_VK_TO_VSC),
            &state,
            &mut text,
            4,
            Some(GetKeyboardLayout(0)),
        )
    };
    if length == 1 {
        char::from_u32(u32::from(text[0]))
            .filter(char::is_ascii_graphic)
            .unwrap_or(key)
    } else {
        key
    }
}

pub fn binding(
    key: char,
    control: bool,
    shift: bool,
    alt: bool,
    win: bool,
) -> Option<(HOT_KEY_MODIFIERS, u32)> {
    let (code, implicit) = if key.is_ascii_alphanumeric() {
        (u32::from(key.to_ascii_uppercase()), 0)
    } else {
        let scan = unsafe { VkKeyScanW(u16::try_from(u32::from(key)).ok()?) };
        if scan == -1 {
            return None;
        }
        ((scan as u16 & 0xff) as u32, (scan as u16 >> 8) as u32)
    };
    let mut flags = HOT_KEY_MODIFIERS(0);
    for (on, modifier) in [
        (control || implicit & 2 != 0, MOD_CONTROL),
        (shift || implicit & 1 != 0, MOD_SHIFT),
        (alt || implicit & 4 != 0, MOD_ALT),
        (win, MOD_WIN),
    ] {
        if on {
            flags |= modifier;
        }
    }
    Some((flags, code))
}

#[cfg(test)]
mod tests {
    use super::binding;
    use windows::Win32::UI::Input::KeyboardAndMouse::{MOD_CONTROL, MOD_SHIFT};

    #[test]
    fn question_mark_maps_to_the_shifted_slash_key() {
        let question = binding('?', true, true, false, false).unwrap();
        assert_eq!(question.0, MOD_CONTROL | MOD_SHIFT);
        assert!(question.1 > 0);
        assert_eq!(binding('a', true, false, false, false).unwrap().1, 0x41);
    }
}
