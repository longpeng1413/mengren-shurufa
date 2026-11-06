//! 登记连接与来电接入。断线后重连；身份、名称和内容仍由加密通道核验。

use super::{RelayConfig, RelayStream, socket};
use crate::{
    SyncError,
    service::{command::Command, lock, round, state::Shared},
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::Sender,
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tungstenite::Message;

pub(crate) fn dial(
    config: &RelayConfig,
    own: &str,
    target: &str,
) -> Result<RelayStream, SyncError> {
    if !super::valid_id(target) || own == target {
        return Err(super::invalid());
    }
    RelayStream::ready(socket::open(config, &format!("connect/{own}/{target}"))?)
}

pub(crate) fn spawn(
    shared: Arc<Shared>,
    commands: Sender<Command>,
    stop: Arc<AtomicBool>,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let config = lock(&shared.relay).clone();
        while !stop.load(Ordering::Relaxed) {
            let _ = listen(&shared, &commands, &stop, &config);
            shared.relay_connected.store(false, Ordering::Relaxed);
            lock(&shared.relay_peers).clear();
            shared.emit(crate::SyncEvent::NearbyChanged);
            for _ in 0..12 {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        }
    })
}

fn listen(
    shared: &Arc<Shared>,
    commands: &Sender<Command>,
    stop: &AtomicBool,
    config: &RelayConfig,
) -> Result<(), SyncError> {
    let own = shared.identity.device_id();
    let mut socket = socket::open(config, &format!("register/{own}"))?;
    socket
        .get_ref()
        .tcp()
        .set_read_timeout(Some(Duration::from_millis(250)))?;
    let mut ping = Instant::now() - Duration::from_secs(10);
    let mut last_frame = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        if ping.elapsed() >= Duration::from_secs(10) {
            socket
                .send(Message::Ping(Vec::new().into()))
                .map_err(|_| super::invalid())?;
            ping = Instant::now();
        }
        match socket.read() {
            Ok(Message::Text(text)) => {
                last_frame = Instant::now();
                let message: serde_json::Value = serde_json::from_str(&text)?;
                if let Some(peers) = message["peers"].as_array() {
                    let peers: Vec<String> = peers
                        .iter()
                        .filter_map(|id| id.as_str())
                        .filter(|id| super::valid_id(id) && *id != own)
                        .take(16)
                        .map(str::to_owned)
                        .collect();
                    let changed = {
                        let mut previous = lock(&shared.relay_peers);
                        let changed = *previous != peers;
                        *previous = peers;
                        changed
                    };
                    let connected = shared.relay_connected.swap(true, Ordering::Relaxed);
                    if changed || !connected {
                        shared.emit(crate::SyncEvent::NearbyChanged);
                        let _ = commands.send(Command::SyncAll);
                    }
                }
                if let Some(session) = message["incoming"]
                    .as_str()
                    .filter(|id| super::valid_id(id))
                {
                    if shared.relay_active.fetch_add(1, Ordering::Relaxed) >= 4 {
                        shared.relay_active.fetch_sub(1, Ordering::Relaxed);
                        continue;
                    }
                    let config = config.clone();
                    let shared = shared.clone();
                    let session = session.to_owned();
                    let own = own.clone();
                    std::thread::spawn(move || {
                        if let Ok(socket) =
                            socket::open(&config, &format!("accept/{own}/{session}"))
                            && let Ok(stream) = RelayStream::ready(socket)
                        {
                            round::handle_stream(stream, None, &shared);
                        }
                        shared.relay_active.fetch_sub(1, Ordering::Relaxed);
                    });
                }
            }
            Ok(Message::Ping(_) | Message::Pong(_)) => {
                last_frame = Instant::now();
                socket.flush().map_err(|_| super::invalid())?;
            }
            Ok(Message::Close(_)) => break,
            Ok(_) => return Err(super::invalid()),
            Err(error) if socket::timeout(&error) => {}
            Err(_) => return Err(super::invalid()),
        }
        if last_frame.elapsed() > Duration::from_secs(15) {
            return Err(super::invalid());
        }
    }
    let _ = socket.close(None);
    Ok(())
}
