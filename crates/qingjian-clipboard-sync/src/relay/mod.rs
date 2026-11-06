//! 公网中转传输：WSS 外层鉴权，内层复用原有 SPAKE2 配对和 Noise 加密。

pub(crate) mod client;
mod config;
mod replica;
#[cfg(any(test, feature = "relay-server"))]
pub mod server;
mod socket;
mod stream;

pub use config::RelayConfig;
pub(crate) use replica::BoundedReplica;
pub(crate) use stream::RelayStream;

pub const MAX_FRAME: usize = 64 * 1024;
pub const MAX_CONTENT_BYTES: usize = 100_000_000;

pub fn valid_id(id: &str) -> bool {
    id.len() == 64
        && id
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn invalid() -> crate::SyncError {
    crate::SyncError::Protocol("invalid relay configuration or response".into())
}
