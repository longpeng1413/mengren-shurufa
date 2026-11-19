//! 只保存在内存的在线设备与等待接通记录。

use std::collections::HashMap;
use std::net::TcpStream;
use std::sync::{Mutex, atomic::AtomicUsize, mpsc::SyncSender};
use tungstenite::WebSocket;

type PendingConnection = (String, SyncSender<WebSocket<TcpStream>>);

#[derive(Default)]
pub(super) struct Broker {
    pub online: Mutex<HashMap<String, (String, SyncSender<String>)>>,

    pub pending: Mutex<HashMap<String, PendingConnection>>,

    #[cfg(test)]
    pub openings: Mutex<Vec<u8>>,

    pub active: AtomicUsize,
}
