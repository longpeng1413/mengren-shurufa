//! 有界线程服务；默认由 Caddy 前置，只监听回环地址。

use super::broker::Broker;
use std::net::{SocketAddr, TcpListener};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread::JoinHandle;
use std::time::Duration;

pub struct RelayServer {
    address: SocketAddr,

    stop: Arc<AtomicBool>,

    thread: Option<JoinHandle<()>>,
}

impl RelayServer {
    pub fn start(address: SocketAddr, token: String) -> std::io::Result<Self> {
        if !(24..=256).contains(&token.len()) || !token.bytes().all(|b| b.is_ascii_graphic()) {
            return Err(std::io::Error::other("invalid relay access token"));
        }
        let listener = TcpListener::bind(address)?;
        let address = listener.local_addr()?;
        listener.set_nonblocking(true)?;
        let stop = Arc::new(AtomicBool::new(false));
        let cancelled = stop.clone();
        let broker = Arc::new(Broker::default());
        let thread = std::thread::spawn(move || {
            while !cancelled.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((tcp, _)) => {
                        if broker.active.load(Ordering::Relaxed) >= 48 {
                            continue;
                        }
                        broker.active.fetch_add(1, Ordering::Relaxed);
                        let broker = broker.clone();
                        let token = token.clone();
                        let cancelled = cancelled.clone();
                        std::thread::spawn(move || {
                            super::handle(tcp, &token, &broker, &cancelled);
                            broker.active.fetch_sub(1, Ordering::Relaxed);
                        });
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(25))
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            address,
            stop,
            thread: Some(thread),
        })
    }
    pub fn address(&self) -> SocketAddr {
        self.address
    }
}

impl Drop for RelayServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
