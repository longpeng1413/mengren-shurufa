//! 取 Windows 真正有键盘焦点的窗口及线程。
use crate::protocol::SessionId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputFocus {
    pub session: SessionId,
    pub window: isize,
}

#[cfg(windows)]
impl InputFocus {
    pub fn current() -> Option<Self> {
        use windows::Win32::UI::WindowsAndMessaging::{
            GUITHREADINFO, GetGUIThreadInfo, GetWindowThreadProcessId,
        };
        let mut info = GUITHREADINFO {
            cbSize: size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        // 线程 0 指前台输入队列。框架窗口和真正的焦点控件可能由不同线程持有。
        unsafe { GetGUIThreadInfo(0, &mut info) }.ok()?;
        if info.hwndFocus.is_invalid() {
            return None;
        }
        let thread = unsafe { GetWindowThreadProcessId(info.hwndFocus, None) };
        (thread != 0).then_some(Self {
            session: SessionId(u64::from(thread)),
            window: info.hwndFocus.0 as isize,
        })
    }
}
