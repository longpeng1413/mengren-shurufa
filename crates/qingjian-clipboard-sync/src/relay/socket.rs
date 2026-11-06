//! 中转 WebSocket 的 TCP/TLS 连接；逐个尝试解析出的 IPv4 与 IPv6 地址。

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned, pki_types::ServerName};
use tungstenite::{WebSocket, client::IntoClientRequest, http::HeaderValue};

use super::{RelayConfig, invalid};
use crate::SyncError;

pub(super) enum Socket {
    Local(TcpStream),

    Secure(Box<StreamOwned<ClientConnection, TcpStream>>),
}

impl Socket {
    pub fn tcp(&self) -> &TcpStream {
        match self {
            Self::Local(tcp) => tcp,
            Self::Secure(tls) => &tls.sock,
        }
    }
}

impl Read for Socket {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::Local(tcp) => tcp.read(buf),
            Self::Secure(tls) => tls.read(buf),
        }
    }
}

impl Write for Socket {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Self::Local(tcp) => tcp.write(buf),
            Self::Secure(tls) => tls.write(buf),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Self::Local(tcp) => tcp.flush(),
            Self::Secure(tls) => tls.flush(),
        }
    }
}

pub(super) fn open(config: &RelayConfig, route: &str) -> Result<WebSocket<Socket>, SyncError> {
    config.validate()?;
    let endpoint = format!("{}/{}", config.url, route);
    let mut request = endpoint.into_client_request().map_err(|_| invalid())?;
    let uri = request.uri();
    let host = uri
        .host()
        .ok_or_else(invalid)?
        .trim_matches(['[', ']'])
        .to_owned();
    let secure = uri.scheme_str() == Some("wss");
    let port = uri.port_u16().unwrap_or(if secure { 443 } else { 80 });
    let addresses: Vec<_> = (host.as_str(), port).to_socket_addrs()?.take(8).collect();
    request.headers_mut().insert(
        "Authorization",
        HeaderValue::from_str(&format!("Bearer {}", config.token)).map_err(|_| invalid())?,
    );
    // 每个地址最多一秒；一个地址连上但 TLS 失败也继续尝试其余地址。
    for address in addresses {
        let connected = (|| {
            let tcp = TcpStream::connect_timeout(&address, Duration::from_secs(1))?;
            tcp.set_nodelay(true)?;
            tcp.set_read_timeout(Some(Duration::from_secs(5)))?;
            tcp.set_write_timeout(Some(Duration::from_secs(5)))?;
            let stream = if secure {
                let provider = Arc::new(rustls::crypto::ring::default_provider());
                let roots =
                    RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
                let tls = ClientConfig::builder_with_provider(provider)
                    .with_safe_default_protocol_versions()
                    .map_err(|_| invalid())?
                    .with_root_certificates(roots)
                    .with_no_client_auth();
                let connection = ClientConnection::new(
                    Arc::new(tls),
                    ServerName::try_from(host.clone()).map_err(|_| invalid())?,
                )
                .map_err(|_| invalid())?;
                Socket::Secure(Box::new(StreamOwned::new(connection, tcp)))
            } else {
                Socket::Local(tcp)
            };
            let limits = tungstenite::protocol::WebSocketConfig::default()
                .max_message_size(Some(super::MAX_FRAME))
                .max_frame_size(Some(super::MAX_FRAME));
            tungstenite::client::client_with_config(request.clone(), stream, Some(limits))
                .map(|(socket, _)| socket)
                .map_err(|_| invalid())
        })();
        if let Ok(socket) = connected {
            return Ok(socket);
        }
    }
    Err(SyncError::Io(std::io::Error::new(
        std::io::ErrorKind::ConnectionRefused,
        "relay unavailable or authorization rejected",
    )))
}

pub(super) fn timeout(error: &tungstenite::Error) -> bool {
    matches!(error, tungstenite::Error::Io(error) if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut))
}
