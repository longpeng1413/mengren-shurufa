//! winit 的输入 → 交互层的事件。

use winit::event::MouseScrollDelta;
use winit::keyboard::{Key, ModifiersState, NamedKey};

use crate::ui::{KeyCode, Modifiers};

/// 滚轮一格滚多远（点）。
const LINE: f32 = 40.0;

/// NumLock 开启时，小键盘的逻辑键是数字，但无修饰键值可能仍是 Home / 方向键。
/// 只在两者都是字符时剥离修饰键，避免先移动光标、再插入数字。
pub(super) fn event_key(logical: &Key, unmodified: &Key) -> Option<KeyCode> {
    let key = match (logical, unmodified) {
        (Key::Character(_), Key::Character(_)) => unmodified,
        _ => logical,
    };
    key_code(key)
}

/// 字符键按不带修饰键时的字算（⌥1 是 1 不是 ¡，⇧/ 是 / 不是 ?），快捷键才认得出来；打出来的字另走文字输入。
pub(super) fn key_code(key: &Key) -> Option<KeyCode> {
    Some(match key {
        Key::Named(NamedKey::Tab) => KeyCode::Tab,
        Key::Named(NamedKey::Enter) => KeyCode::Enter,
        Key::Named(NamedKey::Space) => KeyCode::Space,
        Key::Named(NamedKey::Escape) => KeyCode::Escape,
        Key::Named(NamedKey::ArrowLeft) => KeyCode::ArrowLeft,
        Key::Named(NamedKey::ArrowRight) => KeyCode::ArrowRight,
        Key::Named(NamedKey::ArrowUp) => KeyCode::ArrowUp,
        Key::Named(NamedKey::ArrowDown) => KeyCode::ArrowDown,
        Key::Named(NamedKey::Home) => KeyCode::Home,
        Key::Named(NamedKey::End) => KeyCode::End,
        Key::Named(NamedKey::Backspace) => KeyCode::Backspace,
        Key::Named(NamedKey::Delete) => KeyCode::Delete,
        Key::Character(text) => KeyCode::Character(text.to_string()),
        _ => return None,
    })
}

/// `command` 是 macOS 的 ⌘、其他平台的 Ctrl。
pub(super) fn modifiers(state: ModifiersState) -> Modifiers {
    Modifiers {
        shift: state.shift_key(),
        alt: state.alt_key(),
        command: if cfg!(target_os = "macos") {
            state.super_key()
        } else {
            state.control_key()
        },
        control: state.control_key(),
        logo: state.super_key(),
    }
}

/// 滚动位置的增量（像素）：winit 给的是内容该挪的方向，反过来；行增量按行高换成像素。
pub(super) fn scroll(delta: MouseScrollDelta, scale: f32) -> (f32, f32) {
    let (dx, dy) = match delta {
        MouseScrollDelta::PixelDelta(position) => (position.x as f32, position.y as f32),
        MouseScrollDelta::LineDelta(x, y) => (x * LINE * scale, y * LINE * scale),
    };
    (-dx, -dy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numpad_digits_do_not_move_the_caret() {
        for (digit, named) in [
            ("7", NamedKey::Home),
            ("4", NamedKey::ArrowLeft),
            ("6", NamedKey::ArrowRight),
            ("1", NamedKey::End),
            ("8", NamedKey::ArrowUp),
        ] {
            assert_eq!(
                event_key(&Key::Character(digit.into()), &Key::Named(named)),
                Some(KeyCode::Character(digit.into()))
            );
        }
    }

    #[test]
    fn navigation_and_unmodified_shortcuts_are_preserved() {
        assert_eq!(
            event_key(
                &Key::Named(NamedKey::ArrowLeft),
                &Key::Named(NamedKey::ArrowLeft)
            ),
            Some(KeyCode::ArrowLeft)
        );
        assert_eq!(
            event_key(&Key::Character("?".into()), &Key::Character("/".into())),
            Some(KeyCode::Character("/".into()))
        );
    }
}
