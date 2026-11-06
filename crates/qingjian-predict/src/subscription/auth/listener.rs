//! 仅监听本机的临时 OAuth 回调。

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener};
use std::time::{Duration, Instant};

use super::{AuthError, Authorization, Callback};

/// 先绑定端口，再打开浏览器；等待必须在后台线程执行。
pub struct LoginListener {
    listener: TcpListener,
    pub authorization: Authorization,
}

impl LoginListener {
    pub fn bind(host_id: String) -> Result<Self, AuthError> {
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
            .map_err(|_| AuthError::InvalidRedirect)?;
        listener
            .set_nonblocking(true)
            .map_err(|_| AuthError::InvalidRedirect)?;
        let port = listener
            .local_addr()
            .map_err(|_| AuthError::InvalidRedirect)?
            .port();
        Ok(Self {
            listener,
            authorization: Authorization::new(port, host_id)?,
        })
    }

    /// 限时等待本机浏览器；状态不匹配的请求不会终止当前登录。
    pub fn wait(&self, timeout: Duration) -> Result<Callback, AuthError> {
        let started = Instant::now();
        while started.elapsed() < timeout {
            let (mut stream, _) = match self.listener.accept() {
                Ok(connection) => connection,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(50));
                    continue;
                }
                Err(_) => return Err(AuthError::InvalidCallback),
            };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let mut bytes = [0u8; 8192];
            let count = stream.read(&mut bytes).unwrap_or(0);
            let request = std::str::from_utf8(&bytes[..count]).unwrap_or("");
            let path = request
                .lines()
                .next()
                .and_then(|line| line.strip_prefix("GET "))
                .and_then(|line| line.split_once(' ').map(|(path, _)| path));
            let result = path.map(|path| {
                self.authorization.callback(&format!(
                    "http://127.0.0.1:{}{path}",
                    self.listener
                        .local_addr()
                        .map(|address| address.port())
                        .unwrap_or(0)
                ))
            });
            let valid = matches!(result, Some(Ok(_)));
            let body = if valid {
                "Login received. You can close this window."
            } else {
                "Invalid login callback."
            };
            let status = if valid { "200 OK" } else { "400 Bad Request" };
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            match result {
                Some(Ok(callback)) => return Ok(callback),
                Some(Err(AuthError::Denied)) => return Err(AuthError::Denied),
                _ => {}
            }
        }
        Err(AuthError::Timeout)
    }
}
