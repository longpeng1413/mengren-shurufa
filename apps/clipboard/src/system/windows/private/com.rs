//! 自动填充后台线程的 COM 生命周期。

use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};

pub(super) struct Com;

impl Com {
    pub fn new() -> Option<Self> {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok().ok()?;
        }
        Some(Self)
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}
