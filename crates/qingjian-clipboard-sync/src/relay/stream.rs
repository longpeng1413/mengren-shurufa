//! 把二进制 WebSocket 帧接成字节流，既有 PAKE/Noise 协议保持不变。

use super::socket::Socket;
use std::io::{Read, Write};
use std::time::Duration;
use tungstenite::{Message, WebSocket};

pub(crate) struct RelayStream {
    socket: WebSocket<Socket>,

    unread: std::io::Cursor<Vec<u8>>,
}

impl RelayStream {
    pub(super) fn ready(mut socket: WebSocket<Socket>) -> Result<Self, crate::SyncError> {
        socket
            .get_ref()
            .tcp()
            .set_read_timeout(Some(Duration::from_secs(10)))?;
        loop {
            match socket.read() {
                Ok(Message::Text(text)) if text == "ready" => break,
                Ok(Message::Ping(_)) => socket.flush().map_err(|_| super::invalid())?,
                _ => return Err(super::invalid()),
            }
        }
        socket
            .get_ref()
            .tcp()
            .set_read_timeout(Some(super::super::service::IO_TIMEOUT))?;
        Ok(Self {
            socket,
            unread: std::io::Cursor::new(Vec::new()),
        })
    }
}

impl Read for RelayStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            let count = self.unread.read(buf)?;
            if count != 0 {
                return Ok(count);
            }
            match self.socket.read() {
                Ok(Message::Binary(bytes)) => self.unread = std::io::Cursor::new(bytes.to_vec()),
                Ok(Message::Close(_)) => return Ok(0),
                Ok(Message::Ping(_)) => self.socket.flush().map_err(std::io::Error::other)?,
                Ok(Message::Pong(_)) => {}
                Ok(_) => return Err(std::io::Error::other("invalid relay frame")),
                Err(error) => return Err(std::io::Error::other(error)),
            }
        }
    }
}

impl Write for RelayStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let count = buf.len().min(super::MAX_FRAME);
        if count > 0 {
            self.socket
                .send(Message::binary(buf[..count].to_vec()))
                .map_err(std::io::Error::other)?;
        }
        Ok(count)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.socket.flush().map_err(std::io::Error::other)
    }
}

impl Drop for RelayStream {
    fn drop(&mut self) {
        let _ = self.socket.close(None);
        // 完成关闭握手，避免 Windows 在尚有控制帧未读时用 TCP reset 丢掉末条响应。
        let _ = self
            .socket
            .get_ref()
            .tcp()
            .set_read_timeout(Some(Duration::from_millis(200)));
        let end = std::time::Instant::now() + Duration::from_millis(200);
        while std::time::Instant::now() < end {
            match self.socket.read() {
                Ok(Message::Close(_)) | Err(_) => break,
                Ok(_) => {}
            }
        }
    }
}
