//! 一条连接从头到尾：进来的连接分流配对与同步，主动连已配对的设备跑一轮，并把结果翻译成事件。

use std::net::{SocketAddr, TcpStream};
use std::time::Instant;

use super::event::SyncEvent;
use super::hello::PairHello;
use super::received_item::ReceivedItem;
use super::state::Shared;
use super::state::now_millis;
use super::{CONNECT_TIMEOUT, IO_TIMEOUT, device_id_of, lock, scan};
use crate::channel::{Opening, accept, connect, pair_host, read_opening};
use crate::error::SyncError;
use crate::peers::Peer;
use crate::sync::{Progress, Replica, run_initiator, run_responder};

pub(super) fn handle_incoming(stream: TcpStream, address: SocketAddr, shared: &Shared) {
    let _ = stream.set_nodelay(true);
    if stream
        .set_nonblocking(false)
        .and_then(|()| stream.set_read_timeout(Some(IO_TIMEOUT)))
        .and_then(|()| stream.set_write_timeout(Some(IO_TIMEOUT)))
        .is_ok()
    {
        handle_stream(stream, Some(address), shared);
    }
}

pub(crate) fn handle_stream<S: std::io::Read + std::io::Write>(
    mut stream: S,
    address: Option<SocketAddr>,
    shared: &Shared,
) {
    let result = (|| match read_opening(&mut stream)? {
        Opening::Pair => host_pairing(stream, address, shared),
        Opening::Sync => respond(stream, address, shared),
        Opening::Capabilities => super::private::respond(stream, shared, false),
        Opening::Private => super::private::respond(stream, shared, true),
        Opening::Phrases => super::phrases::respond(stream, shared),
        Opening::Metadata => super::metadata::respond(stream, shared),
        Opening::Info if address.is_some() => scan::answer(&mut stream, shared),
        Opening::Info => Err(SyncError::Handshake),
    })();
    if result.is_err() {
        tracing::debug!(relay = address.is_none(), "Incoming sync ended");
    }
}

fn host_pairing<S: std::io::Read + std::io::Write>(
    stream: S,
    address: Option<SocketAddr>,
    shared: &Shared,
) -> Result<(), SyncError> {
    // 先取走再握手：一个码只给一次机会，猜错就作废
    let pending = lock(&shared.pending).take();
    let code = pending
        .filter(|pending| pending.expires > Instant::now())
        .ok_or(SyncError::Handshake)?
        .code;
    let result = (|| {
        let mut channel = pair_host(stream, &shared.identity, &code)?;
        let hello = PairHello::decode(&channel.receive()?)?;
        let own = PairHello {
            name: shared.name.clone(),
            port: shared.port,
        };
        channel.send(&own.encode()?)?;
        lock(&shared.book).add(
            channel.remote(),
            hello.name.clone(),
            address.map(|address| hello.address_from(address)),
        )?;
        shared.record_link(&device_id_of(channel.remote()), true);
        Ok(hello.name)
    })();
    shared.emit(match &result {
        Ok(name) => SyncEvent::Paired { name: name.clone() },
        Err(_) => SyncEvent::PairFailed,
    });
    result.map(drop)
}

fn respond<S: std::io::Read + std::io::Write>(
    stream: S,
    address: Option<SocketAddr>,
    shared: &Shared,
) -> Result<(), SyncError> {
    let mut channel = accept(stream, &shared.identity, |key| {
        lock(&shared.book).contains(key)
    })?;
    let device_id = device_id_of(channel.remote());
    let peer = peer_name(shared, &device_id);
    if let Some(address) = address {
        remember_new_ip(shared, &device_id, address);
    }
    let mut replica = shared.replica.clone();
    let before = newest_at(&replica);
    let mut watcher = Watcher::new(shared, peer);
    let mut filtered = crate::relay::BoundedReplica(&mut replica);
    let selected: &mut dyn Replica = if address.is_none() {
        &mut filtered
    } else {
        filtered.0
    };
    let result = run_responder(&mut channel, selected, &mut |progress| {
        watcher.observe(progress)
    });
    watcher.finish(&result, &replica, before);
    if result.is_ok() {
        lock(&shared.routes).insert(device_id.clone(), address.is_none());
    }
    shared.record_link(&device_id, result.is_ok());
    result.map(drop)
}

/// 对方换了 IP 又连进来：端口不变，记下新 IP，下次这边也连得回去。
fn remember_new_ip(shared: &Shared, device_id: &str, source: SocketAddr) {
    let mut book = lock(&shared.book);
    let port = book
        .peers()
        .iter()
        .find(|peer| peer.device_id == device_id)
        .and_then(|peer| peer.last_address.as_deref()?.parse::<SocketAddr>().ok())
        .map(|known| known.port());
    if let Some(port) = port {
        let address = SocketAddr::new(source.ip(), port).to_string();
        if let Err(error) = book.touch(device_id, now_millis(), Some(address)) {
            tracing::warn!(%error, "cannot save peer address");
        }
    }
}

/// 主动连一台已配对的设备跑一轮。
pub(super) fn sync_peer(shared: &Shared, peer: &Peer) {
    let online = lock(&shared.nearby)
        .get(&peer.device_id)
        .map(|nearby| nearby.address);
    let address = online.or_else(|| peer.last_address.as_deref()?.parse().ok());
    let mut result = address.map_or_else(
        || Err(SyncError::Io(std::io::Error::other("No LAN address"))),
        |address| {
            let stream = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT)?;
            stream.set_read_timeout(Some(IO_TIMEOUT))?;
            stream.set_write_timeout(Some(IO_TIMEOUT))?;
            initiate(shared, peer, stream, false)
        },
    );
    if result.is_err()
        && !matches!(result, Err(SyncError::NotPaired))
        && lock(&shared.relay_peers).contains(&peer.device_id)
    {
        let config = lock(&shared.relay).clone();
        if config.enabled {
            result =
                crate::relay::client::dial(&config, &shared.identity.device_id(), &peer.device_id)
                    .and_then(|stream| initiate(shared, peer, stream, true));
        }
    }
    if matches!(result, Err(SyncError::NotPaired)) {
        // 对方取消了配对：这边也删掉，不再连、不亮红灯
        tracing::info!(peer = %peer.name, "peer unpaired this device, forgetting it");
        if let Err(error) = lock(&shared.book).remove(&peer.device_id) {
            tracing::warn!(%error, "cannot forget peer");
        }
        shared.emit(SyncEvent::NearbyChanged);
        return;
    }
    shared.record_link(&peer.device_id, result.is_ok());
    if result.is_ok() {
        super::metadata::sync_peer(shared, peer);
        super::phrases::sync_peer(shared, peer);
    }
    if let Err(error) = result {
        tracing::info!(%error, peer = %peer.name, "sync failed");
        // 只有 mDNS 还看得见它才算失败；看不见就是离线，上线后补传
        if online.is_some() {
            shared.emit(SyncEvent::SendFailed {
                peer: peer.name.clone(),
            });
        }
    }
}

fn initiate<S: std::io::Read + std::io::Write>(
    shared: &Shared,
    peer: &Peer,
    stream: S,
    relay: bool,
) -> Result<usize, SyncError> {
    let mut channel = connect(stream, &shared.identity, |key| {
        device_id_of(key) == peer.device_id
    })?;
    let mut replica = shared.replica.clone();
    let before = newest_at(&replica);
    let mut watcher = Watcher::new(shared, peer.name.clone());
    let mut filtered = crate::relay::BoundedReplica(&mut replica);
    let selected: &mut dyn Replica = if relay { &mut filtered } else { filtered.0 };
    let result = run_initiator(&mut channel, selected, &mut |progress| {
        watcher.observe(progress)
    });
    watcher.finish(&result, &replica, before);
    if result.is_ok() {
        lock(&shared.routes).insert(peer.device_id.clone(), relay);
    }
    tracing::debug!(relay, success = result.is_ok(), "Sync round completed");
    result
}

fn peer_name(shared: &Shared, device_id: &str) -> String {
    lock(&shared.book)
        .peers()
        .iter()
        .find(|peer| peer.device_id == device_id)
        .map(|peer| peer.name.clone())
        .unwrap_or_default()
}

fn newest_at(replica: &dyn Replica) -> i64 {
    replica
        .stamps()
        .ok()
        .and_then(|stamps| {
            stamps
                .iter()
                .filter(|stamp| !stamp.deleted && !stamp.forgotten)
                .map(|stamp| stamp.at)
                .max()
        })
        .unwrap_or(i64::MIN)
}

/// 把一轮的进度翻成事件。
struct Watcher<'a> {
    shared: &'a Shared,

    peer: String,

    started: Option<Instant>,

    /// 已经收完的那些消息的字节数，加上当前这条的进度才是总进度。
    done: u64,
}

impl<'a> Watcher<'a> {
    fn new(shared: &'a Shared, peer: String) -> Self {
        Self {
            shared,
            peer,
            started: None,
            done: 0,
        }
    }

    fn observe(&mut self, progress: Progress) {
        match progress {
            Progress::Receiving => {
                self.started = Some(Instant::now());
                self.shared.emit(SyncEvent::ReceiveStarted {
                    peer: self.peer.clone(),
                });
            }
            Progress::Bytes { received, total } => {
                let received_all = self.done + received;
                if received == total {
                    self.done += total;
                }
                let elapsed = self.started.map_or(0.0, |at| at.elapsed().as_secs_f64());
                let bytes_per_second = if elapsed > 0.05 {
                    (received_all as f64 / elapsed) as u64
                } else {
                    0
                };
                self.shared.emit(SyncEvent::ReceiveProgress {
                    peer: self.peer.clone(),
                    received,
                    total,
                    bytes_per_second,
                });
            }
        }
    }

    fn finish(&self, result: &Result<usize, SyncError>, replica: &dyn Replica, before: i64) {
        if self.started.is_none() {
            return;
        }
        let peer = self.peer.clone();
        self.shared.emit(match result {
            Ok(_) => SyncEvent::ReceiveFinished {
                peer,
                newest: newest_item(replica, before),
            },
            Err(_) => SyncEvent::ReceiveFailed { peer },
        });
    }
}

/// 比 `before` 还新的那一条，就是这轮收到的最新内容。
fn newest_item(replica: &dyn Replica, before: i64) -> Option<ReceivedItem> {
    let stamps = replica.stamps().ok()?;
    let newest = stamps
        .iter()
        .filter(|stamp| !stamp.deleted && !stamp.forgotten && stamp.at > before)
        .max_by_key(|stamp| stamp.at)?;
    let record = replica.record(&newest.id).ok()??;
    Some(ReceivedItem {
        id: newest.id.clone(),
        kind: record.kind,
    })
}
