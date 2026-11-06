//! [`SyncService`]：后台跑同步。打开同步时才建它，它一建就监听端口并公布自己；丢掉它就全停。
//! 线程三个：接受连接、mDNS 发现、主动同步。平台只提供 [`Replica`]，并把事件接到界面。

#[cfg(test)]
mod tests;

pub(crate) mod command;
mod event;
mod hello;
mod link;
mod locked;
mod options;
mod received_item;
pub(crate) mod round;
mod scan;
pub(crate) mod state;
mod worker;

pub use event::SyncEvent;
pub use link::{PeerState, PeerStatus};
pub use options::SyncOptions;
pub use received_item::ReceivedItem;
pub use state::Nearby;

use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::channel::{generate_pair_code, pair_join};
use crate::discovery::{Discovery, Found};
use crate::error::SyncError;
use crate::identity::Identity;
use crate::peers::{Peer, PeerBook, hex};
use crate::sync::Replica;
use command::Command;
use hello::PairHello;
use locked::LockedReplica;
use state::{PendingCode, Shared, now_millis};
use worker::{apply_found, spawn_accept, spawn_discovery, spawn_worker};

/// 监听端口的首选，被占了由系统分配，反正靠 mDNS 公布。
const PREFERRED_PORT: u16 = 53319;

/// 配对码多久过期。
const PAIR_CODE_LIFETIME: Duration = Duration::from_secs(120);

const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

/// 读写超时：连上之后对方不说话多久算断。
pub(crate) const IO_TIMEOUT: Duration = Duration::from_secs(15);

pub struct SyncService {
    shared: Arc<Shared>,

    commands: Sender<Command>,

    stop: Arc<AtomicBool>,

    threads: Vec<JoinHandle<()>>,

    port: u16,
}

impl SyncService {
    pub fn start(
        options: SyncOptions,
        replica: Box<dyn Replica + Send>,
        events: Sender<SyncEvent>,
    ) -> Result<Self, SyncError> {
        let identity = Identity::load_or_create(&options.directory.join("identity.key"))?;
        let book = PeerBook::open(&options.directory.join("peers.json"))?;
        let listener = TcpListener::bind(("0.0.0.0", PREFERRED_PORT))
            .or_else(|_| TcpListener::bind(("0.0.0.0", 0)))?;
        let port = listener.local_addr()?.port();
        listener.set_nonblocking(true)?;

        let discovery = if options.own_discovery {
            Some(Discovery::start(
                &identity.device_id(),
                &options.name,
                &options.model,
                port,
            )?)
        } else {
            None
        };
        let shared = Arc::new(Shared::new(
            identity,
            options.name,
            options.model,
            port,
            book,
            LockedReplica::new(replica),
            events,
        ));
        let stop = Arc::new(AtomicBool::new(false));
        if options.relay.enabled {
            if options.relay.validate().is_ok() {
                *lock(&shared.relay) = options.relay;
            } else {
                tracing::warn!("Invalid relay configuration; LAN sync remains available");
            }
        }
        let (commands, inbox) = channel();
        // 一启动就对一轮：状态不用等，离线时攒下的也马上补
        let _ = commands.send(Command::SyncAll);
        let mut threads = vec![
            spawn_accept(listener, shared.clone(), stop.clone()),
            spawn_worker(inbox, shared.clone(), commands.clone(), stop.clone()),
        ];
        if let Some(discovery) = discovery {
            threads.push(spawn_discovery(
                discovery,
                shared.clone(),
                commands.clone(),
                stop.clone(),
            ));
        }
        if lock(&shared.relay).enabled {
            threads.push(crate::relay::client::spawn(
                shared.clone(),
                commands.clone(),
                stop.clone(),
            ));
        }
        Ok(Self {
            shared,
            commands,
            stop,
            threads,
            port,
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// 交进来一条发现的结果（平台自己做发现时用）。
    pub fn report(&self, found: Found) {
        apply_found(&self.shared, &self.commands, found);
    }

    /// 本机内容变了（复制、删除、清空）：马上推给在线的设备。
    pub fn notify_changed(&self) {
        let _ = self.commands.send(Command::SyncAll);
    }

    /// 这台显示配对码：生成一个新码，旧码作废。
    pub fn begin_pairing(&self) -> Result<String, SyncError> {
        let code = generate_pair_code()?;
        *lock(&self.shared.pending) = Some(PendingCode {
            code: code.clone(),
            expires: Instant::now() + PAIR_CODE_LIFETIME,
        });
        Ok(code)
    }

    pub fn cancel_pairing(&self) {
        *lock(&self.shared.pending) = None;
    }

    /// 这台输入配对码。在后台连，结果走事件。
    pub fn join_pairing(&self, address: SocketAddr, code: String) {
        let shared = self.shared.clone();
        let commands = self.commands.clone();
        thread::spawn(move || {
            let event = match join(&shared, address, &code) {
                Ok(name) => {
                    let _ = commands.send(Command::SyncAll);
                    SyncEvent::Paired { name }
                }
                Err(error) => {
                    tracing::info!(%error, "pairing failed");
                    SyncEvent::PairFailed
                }
            };
            shared.emit(event);
        });
    }

    pub fn nearby(&self) -> Vec<Nearby> {
        lock(&self.shared.nearby).values().cloned().collect()
    }

    pub fn peers(&self) -> Vec<Peer> {
        lock(&self.shared.book).peers().to_vec()
    }

    /// 已配对设备和它们现在的状态。
    pub fn peer_states(&self) -> Vec<PeerState> {
        let now = now_millis();
        let links = lock(&self.shared.links).clone();
        self.peers()
            .into_iter()
            .map(|peer| PeerState {
                via_relay: lock(&self.shared.routes)
                    .get(&peer.device_id)
                    .copied()
                    .unwrap_or(false),
                status: links
                    .get(&peer.device_id)
                    .copied()
                    .unwrap_or_default()
                    .status(now),
                peer,
            })
            .collect()
    }

    /// 设备页开了或关了。开着时马上探一次、扫一遍，之后勤一些。
    pub fn set_watching(&self, watching: bool) {
        self.shared.watching.store(watching, Ordering::Relaxed);
        if watching {
            let _ = self.commands.send(Command::SyncAll);
            scan::start(self.shared.clone(), self.commands.clone());
        }
    }

    /// 解除配对。只影响本机名单，对方再连会被拒。
    pub fn unpair(&self, device_id: &str) -> Result<(), SyncError> {
        lock(&self.shared.book).remove(device_id)
    }

    pub fn device_id(&self) -> String {
        self.shared.identity.device_id()
    }

    pub fn relay_connected(&self) -> bool {
        self.shared.relay_connected.load(Ordering::Relaxed)
    }

    pub fn relay_peers(&self) -> Vec<String> {
        lock(&self.shared.relay_peers).clone()
    }

    /// 同时接受局域网地址或 relay:<设备ID>；配对码仍只经 SPAKE2 使用。
    pub fn join_target(&self, target: &str, code: String) -> bool {
        if code.len() != 6 || !code.bytes().all(|byte| byte.is_ascii_digit()) {
            return false;
        }
        if let Some(id) = target.strip_prefix("relay:") {
            if !crate::relay::valid_id(id) || !lock(&self.shared.relay).enabled {
                return false;
            }
            let id = id.to_owned();
            let shared = self.shared.clone();
            let commands = self.commands.clone();
            thread::spawn(move || {
                let result = (|| {
                    let config = lock(&shared.relay).clone();
                    let stream =
                        crate::relay::client::dial(&config, &shared.identity.device_id(), &id)?;
                    join_stream(&shared, stream, None, &code, Some(&id))
                })();
                let event = match result {
                    Ok(name) => {
                        let _ = commands.send(Command::SyncAll);
                        SyncEvent::Paired { name }
                    }
                    Err(_) => SyncEvent::PairFailed,
                };
                shared.emit(event);
            });
            true
        } else if let Ok(address) = target.parse() {
            self.join_pairing(address, code);
            true
        } else {
            false
        }
    }
}

impl Drop for SyncService {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for handle in self.threads.drain(..) {
            let _ = handle.join();
        }
    }
}

fn join(shared: &Shared, address: SocketAddr, code: &str) -> Result<String, SyncError> {
    let stream = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT)?;
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    join_stream(shared, stream, Some(address), code, None)
}

fn join_stream<S: std::io::Read + std::io::Write>(
    shared: &Shared,
    stream: S,
    address: Option<SocketAddr>,
    code: &str,
    expected: Option<&str>,
) -> Result<String, SyncError> {
    let mut channel = pair_join(stream, &shared.identity, code)?;
    if expected.is_some_and(|expected| device_id_of(channel.remote()) != expected) {
        return Err(SyncError::UnknownPeer);
    }
    let own = PairHello {
        name: shared.name.clone(),
        port: shared.port,
    };
    channel.send(&own.encode()?)?;
    let hello = PairHello::decode(&channel.receive()?)?;
    lock(&shared.book).add(
        channel.remote(),
        hello.name.clone(),
        address.map(|address| hello.address_from(address)),
    )?;
    shared.record_link(&device_id_of(channel.remote()), true);
    Ok(hello.name)
}

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(crate) fn device_id_of(public: &[u8; 32]) -> String {
    hex(public)
}
