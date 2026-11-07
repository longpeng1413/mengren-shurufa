//! 已认证设备报告自己的显示名，不改变身份公钥或配对关系。

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(super) struct DeviceHello {
    pub version: u8,

    pub name: String,
}

impl DeviceHello {
    pub fn valid(&self) -> bool {
        self.version == 1
            && !self.name.trim().is_empty()
            && self.name.len() <= 256
            && !self.name.chars().any(char::is_control)
    }
}
