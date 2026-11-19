//! 单文件的独立直连操作。服务对账和中转都没有文件发送入口。
mod cache;
mod policy;
mod route;
#[cfg(test)]
mod tests;
mod wire;

use crate::SyncError;
use crate::channel::{Opening, SecureChannel, accept, connect_mode};
use crate::service::{SyncEvent, SyncService, device_id_of, lock, state::Shared};
pub(in crate::service) use policy::Policy;
use route::Route;
#[cfg(target_os = "android")]
pub use route::android::update as update_android_network;
use std::fs::File;
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;

pub const FILE_LIMIT: u64 = 300_000_000;

impl SyncService {
    #[cfg(test)]
    pub(crate) fn test_file_loopback(&self, enabled: bool) {
        lock(&self.shared.files).test_loopback = enabled;
    }

    #[cfg(test)]
    pub(crate) fn test_forget_direct_routes(&self) {
        let peers = self.peers();
        lock(&self.shared.nearby).clear();
        let mut book = lock(&self.shared.book);
        for peer in peers {
            book.touch(
                &peer.device_id,
                crate::service::state::now_millis(),
                Some("127.0.0.1:0".into()),
            )
            .unwrap();
        }
    }

    pub fn files_enabled(&self) -> bool {
        lock(&self.shared.files).enabled
    }
    pub fn file_peer_selected(&self, id: &str) -> bool {
        lock(&self.shared.files).peers.contains(id)
    }
    pub fn file_inbox(&self) -> PathBuf {
        lock(&self.shared.files).directory.join("received-files")
    }

    pub fn set_files_enabled(&self, enabled: bool) -> Result<(), SyncError> {
        let mut policy = lock(&self.shared.files);
        let peers = policy.peers.clone();
        policy.update(enabled, peers)
    }

    pub fn select_file_peer(&self, id: &str, selected: bool) -> Result<(), SyncError> {
        let mut policy = lock(&self.shared.files);
        let mut peers = policy.peers.clone();
        if selected {
            if !lock(&self.shared.book)
                .peers()
                .iter()
                .any(|peer| peer.device_id == id)
            {
                return Err(SyncError::UnknownPeer);
            }
            peers.insert(id.to_owned());
        } else {
            peers.remove(id);
        }
        let enabled = policy.enabled;
        policy.update(enabled, peers)
    }

    /// 平台只传已经打开的文件或 URI 描述符，不把路径解释为远端请求。
    pub fn send_file(&self, file: File, name: String, length: Option<u64>) {
        let shared = self.shared.clone();
        let policy = lock(&shared.files);
        if !policy.enabled {
            return;
        }
        if !cache::valid_name(&name) || length.is_some_and(|n| n >= FILE_LIMIT) {
            shared.emit(SyncEvent::FileStatus("invalid"));
            return;
        }
        let revision = policy.revision;
        let intended = policy.peers.len();
        let nearby = lock(&shared.nearby);
        let targets: Vec<_> = lock(&shared.book)
            .peers()
            .iter()
            .filter(|p| policy.peers.contains(&p.device_id))
            .filter_map(|p| {
                let address = nearby
                    .get(&p.device_id)
                    .map(|n| n.address)
                    .or_else(|| p.last_address.as_ref()?.parse().ok())?;
                Some((
                    p.device_id.clone(),
                    address,
                    Route::resolve(&policy, address)?,
                ))
            })
            .collect();
        drop(nearby);
        drop(policy);
        if targets.is_empty() {
            shared.emit(SyncEvent::FileStatus("unavailable"));
            return;
        }
        std::thread::spawn(move || {
            let mut outputs: Vec<_> = targets
                .into_iter()
                .filter_map(|(id, addr, route)| {
                    Outgoing::open(&shared, revision, id, addr, route, &name, length).ok()
                })
                .collect();
            if outputs.is_empty() {
                shared.emit(SyncEvent::FileStatus("unavailable"));
                return;
            }
            let result = wire::send(file, length, |frame| {
                outputs.retain_mut(|out| out.send(&shared, revision, frame).is_ok());
                if outputs.is_empty() {
                    Err(SyncError::Handshake)
                } else {
                    Ok(())
                }
            });
            let confirmed = if result.is_ok() {
                outputs
                    .iter_mut()
                    .filter_map(|o| {
                        if o.channel.receive_limited(16).ok()? != b"saved" {
                            return None;
                        }
                        let _ = o.channel.send(b"done");
                        Some(())
                    })
                    .count()
            } else {
                0
            };
            shared.emit(SyncEvent::FileStatus(
                if result.is_err_and(|e| matches!(e, SyncError::Protocol(_))) {
                    "invalid"
                } else if confirmed == 0 {
                    "unavailable"
                } else if confirmed < intended {
                    "partial"
                } else {
                    "sent"
                },
            ));
        });
    }
}

struct Outgoing {
    channel: SecureChannel<TcpStream>,
    peer: String,
    address: SocketAddr,
    route: Route,
}

impl Outgoing {
    fn open(
        shared: &Shared,
        revision: u64,
        id: String,
        address: SocketAddr,
        route: Route,
        name: &str,
        length: Option<u64>,
    ) -> Result<Self, SyncError> {
        check(shared, revision, &id, address, route)?;
        let stream = route.connect(address)?;
        let channel = connect_mode(
            stream,
            &shared.identity,
            |key| device_id_of(key) == id,
            Opening::LanFile,
        )?;
        let mut out = Self {
            channel,
            peer: id,
            address,
            route,
        };
        if out.channel.receive_limited(32)? != wire::READY {
            return Err(SyncError::Handshake);
        }
        out.send(shared, revision, &serde_json::to_vec(&(name, length))?)?;
        Ok(out)
    }
    fn send(&mut self, shared: &Shared, revision: u64, bytes: &[u8]) -> Result<(), SyncError> {
        check(shared, revision, &self.peer, self.address, self.route)?;
        self.channel.send(bytes)
    }
}

fn check(
    shared: &Shared,
    revision: u64,
    id: &str,
    address: SocketAddr,
    route: Route,
) -> Result<(), SyncError> {
    let policy = lock(&shared.files);
    if policy.enabled
        && policy.revision == revision
        && policy.peers.contains(id)
        && Route::resolve(&policy, address) == Some(route)
        && lock(&shared.book).peers().iter().any(|p| p.device_id == id)
    {
        Ok(())
    } else {
        Err(SyncError::Handshake)
    }
}

pub(in crate::service) fn incoming(stream: TcpStream, shared: &Shared) {
    let result = receive(stream, shared);
    if result.is_err() && lock(&shared.files).enabled {
        shared.emit(SyncEvent::FileStatus("failed"));
    }
}

fn receive(stream: TcpStream, shared: &Shared) -> Result<(), SyncError> {
    let address = stream.peer_addr()?;
    let (route, revision, root) = {
        let policy = lock(&shared.files);
        if !policy.enabled || policy.peers.is_empty() {
            return Err(SyncError::Handshake);
        }
        (
            Route::resolve(&policy, address).ok_or(SyncError::Handshake)?,
            policy.revision,
            policy.directory.join("received-files"),
        )
    };
    route.incoming(&stream)?;
    let mut channel = accept(stream, &shared.identity, |key| {
        lock(&shared.book).contains(key)
    })?;
    let id = device_id_of(channel.remote());
    let guard = || check(shared, revision, &id, address, route);
    guard()?;
    let stamp = clipboard_stamp();
    channel.send(wire::READY)?;
    let (name, length): (String, Option<u64>) =
        serde_json::from_slice(&channel.receive_limited(2048)?)?;
    if !cache::valid_name(&name) || length.is_some_and(|n| n >= FILE_LIMIT) {
        return Err(SyncError::Handshake);
    }
    std::fs::create_dir_all(&root)?;
    cache::clean(&root, false);
    let mut temp = tempfile::Builder::new()
        .prefix("partial-")
        .tempfile_in(&root)?;
    wire::receive(&mut temp, length, |limit| {
        let frame = channel.receive_limited(limit)?;
        guard()?;
        Ok(frame)
    })?;
    temp.as_file().sync_all()?;
    guard()?;
    let dir = tempfile::Builder::new().prefix("item-").tempdir_in(&root)?;
    let path = dir.path().join(name);
    temp.persist_noclobber(&path).map_err(|e| e.error)?;
    guard()?;
    let _ = dir.keep();
    shared.emit(SyncEvent::FileReceived {
        path,
        clipboard_stamp: stamp,
    });
    channel.send(b"saved")?;
    let _ = channel.receive_limited(4);
    Ok(())
}

fn clipboard_stamp() -> i64 {
    #[cfg(windows)]
    return i64::from(unsafe {
        windows_sys::Win32::System::DataExchange::GetClipboardSequenceNumber()
    });
    #[cfg(not(windows))]
    crate::service::state::now_millis()
}
