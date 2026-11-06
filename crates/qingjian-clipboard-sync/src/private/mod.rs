//! 只在内存停留的私密内容，不接触 Replica、事件正文或离线队列。

mod secret;
mod slot;
#[cfg(test)]
mod tests;
mod vault;

pub use secret::Secret;
pub(crate) use slot::Slot;
pub use vault::PrivateVault;

pub const MAX_SECRET_BYTES: usize = 16 * 1024;
pub const LIFETIME: std::time::Duration = std::time::Duration::from_secs(120);
