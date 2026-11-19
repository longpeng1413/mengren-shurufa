//! 文件连接限定在内核选中的物理链路，双向 TTL=1 阻止经路由器转发。
use super::Policy;
use crate::SyncError;
use socket2::{Domain, Protocol, SockRef, Socket, Type};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};

#[cfg(target_os = "android")]
pub mod android;
#[cfg(windows)]
mod windows;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Route {
    pub local: Ipv4Addr,
    pub network: u64,
}

impl Route {
    pub fn resolve(_policy: &Policy, address: SocketAddr) -> Option<Self> {
        let SocketAddr::V4(v4) = address else {
            return None;
        };
        let ip = *v4.ip();
        #[cfg(test)]
        if _policy.test_loopback && ip.is_loopback() {
            return Some(Self {
                local: ip,
                network: 0,
            });
        }
        if ip.is_loopback() || ip.is_unspecified() || ip.is_multicast() || ip.is_broadcast() {
            return None;
        }
        #[cfg(windows)]
        return windows::resolve(ip);
        #[cfg(target_os = "android")]
        return android::resolve(ip);
        #[cfg(not(any(windows, target_os = "android")))]
        None
    }

    fn pin(self, socket: &Socket) -> Result<(), SyncError> {
        socket.set_ttl_v4(1)?;
        #[cfg(test)]
        if self.network == 0 && self.local.is_loopback() {
            return Ok(());
        }
        #[cfg(windows)]
        windows::pin(socket, self.network as u32)?;
        #[cfg(target_os = "android")]
        android::pin(socket, self.network)?;
        Ok(())
    }

    pub fn connect(self, address: SocketAddr) -> Result<TcpStream, SyncError> {
        let socket = Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP))?;
        self.pin(&socket)?;
        socket.bind(&SocketAddr::from((self.local, 0)).into())?;
        socket.connect_timeout(&address.into(), crate::service::CONNECT_TIMEOUT)?;
        let stream = TcpStream::from(socket);
        stream.set_nodelay(true)?;
        stream.set_read_timeout(Some(crate::service::IO_TIMEOUT))?;
        stream.set_write_timeout(Some(crate::service::IO_TIMEOUT))?;
        Ok(stream)
    }

    pub fn incoming(self, stream: &TcpStream) -> Result<(), SyncError> {
        if stream.local_addr()?.ip() != self.local {
            return Err(SyncError::Handshake);
        }
        let socket = SockRef::from(stream);
        socket.set_ttl_v4(1)?;
        #[cfg(windows)]
        if self.network != 0 {
            windows::pin(&socket, self.network as u32)?;
        }
        Ok(())
    }
}
