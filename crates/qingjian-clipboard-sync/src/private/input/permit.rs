//! [`InputPermit`]：取消、失败或正常返回都释放输入许可。
use super::InputGate;
use std::sync::atomic::Ordering;

pub struct InputPermit<'a>(pub(super) &'a InputGate);

impl Drop for InputPermit<'_> {
    fn drop(&mut self) {
        self.0.active.store(false, Ordering::SeqCst);
    }
}
