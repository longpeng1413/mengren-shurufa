//! 只查询焦点、控件类型和密码标记，不读取应用输入值。

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, UIA_DocumentControlTypeId,
    UIA_EditControlTypeId,
};
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

pub(super) struct Focus {
    automation: IUIAutomation,

    element: IUIAutomationElement,

    window: HWND,

    password: bool,
}

impl Focus {
    pub fn capture(password: bool) -> Option<Self> {
        let window = unsafe { GetForegroundWindow() };
        if window.is_invalid() {
            return None;
        }
        let automation: IUIAutomation =
            unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }.ok()?;
        let element = unsafe { automation.GetFocusedElement() }.ok()?;
        let kind = unsafe { element.CurrentControlType() }.ok()?;
        if kind != UIA_EditControlTypeId && kind != UIA_DocumentControlTypeId {
            return None;
        }
        if !unsafe { element.CurrentIsKeyboardFocusable() }
            .ok()?
            .as_bool()
            || !unsafe { element.CurrentIsEnabled() }.ok()?.as_bool()
        {
            return None;
        }
        let is_password = unsafe { element.CurrentIsPassword() }.ok()?.as_bool();
        if password && !is_password {
            return None;
        }
        Some(Self {
            automation,
            element,
            window,
            password: is_password,
        })
    }

    pub fn unchanged(&self) -> bool {
        if unsafe { GetForegroundWindow() } != self.window {
            return false;
        }
        let Ok(focused) = (unsafe { self.automation.GetFocusedElement() }) else {
            return false;
        };
        if self.password
            && !unsafe { focused.CurrentIsPassword() }.is_ok_and(|value| value.as_bool())
        {
            return false;
        }
        unsafe { self.automation.CompareElements(&self.element, &focused) }
            .is_ok_and(|value| value.as_bool())
    }

    pub fn password(&self) -> bool {
        self.password
    }
}
