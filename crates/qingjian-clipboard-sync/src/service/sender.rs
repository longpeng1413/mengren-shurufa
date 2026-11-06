//! 平台监听线程使用的弱发送句柄，不拥有服务、不排队内容。

use super::state::Shared;
use crate::private::Secret;
use std::sync::Weak;

#[derive(Clone)]
pub struct PrivateSender(pub(super) Weak<Shared>);

impl PrivateSender {
    pub fn send(&self, secret: Secret) -> bool {
        self.0
            .upgrade()
            .is_some_and(|shared| super::private::send_once(shared, secret))
    }
}
