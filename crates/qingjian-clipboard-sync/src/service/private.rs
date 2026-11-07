//! 独立认证的私密消息，不进入普通同步协议、历史或重试队列。

use super::state::{Shared, now_millis};
use super::{CONNECT_TIMEOUT, IO_TIMEOUT, SyncEvent, SyncService, device_id_of, lock};
use crate::channel::{Opening, accept, connect_mode};
use crate::private::{LIFETIME, MAX_SECRET_BYTES, PrivateVault, Secret};
use crate::{Peer, SyncError};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, atomic::Ordering};
use std::time::Duration;
use zeroize::Zeroizing;

const REQUEST: &[u8] = b"private-capabilities-v1";
const AVAILABLE: &[u8] = b"private-receiver-v1";
const ACK: &[u8] = b"private-accepted-v1";

impl SyncService {
    pub fn set_private_receiver(&self, enabled: bool) {
        let _gate = lock(&self.shared.private_gate);
        self.shared
            .private_receiver
            .store(enabled, Ordering::SeqCst);
        update_availability(&self.shared);
    }

    pub fn set_private_available(&self, available: bool) {
        let _gate = lock(&self.shared.private_gate);
        self.shared
            .private_available
            .store(available, Ordering::SeqCst);
        update_availability(&self.shared);
    }

    pub fn private_sender(&self) -> super::PrivateSender {
        super::PrivateSender(Arc::downgrade(&self.shared))
    }

    pub fn private_vault(&self) -> Arc<PrivateVault> {
        self.shared.private_vault.clone()
    }

    /// 仅尝试此刻在线的接收端，后台一次发送，绝不排队或补发。
    pub fn send_private(&self, secret: Secret) -> bool {
        send_once(self.shared.clone(), secret)
    }
}

fn update_availability(shared: &Shared) {
    if shared.private_receiver.load(Ordering::SeqCst)
        && shared.private_available.load(Ordering::SeqCst)
    {
        shared.private_vault.resume();
    } else {
        shared.private_vault.suspend();
    }
}

pub(super) fn send_once(shared: Arc<Shared>, secret: Secret) -> bool {
    if !shared.private_running.load(Ordering::SeqCst) {
        return false;
    }
    if shared.private_sending.swap(true, Ordering::SeqCst) {
        return false;
    }
    let task = shared.clone();
    let spawned = std::thread::Builder::new()
        .name("clipboard-private-send".into())
        .spawn(move || {
            let shared = task;
            shared.emit(SyncEvent::PrivateSendStarted);
            let expires = now_millis() + LIFETIME.as_millis() as i64;
            let peers = lock(&shared.book).peers().to_vec();
            let mut count = 0;
            // 有界并行，各目标独立尝试；慢目标不能耗尽全局时间而跳过后面的设备。
            for group in peers.chunks(4) {
                count += std::thread::scope(|scope| {
                    let tasks: Vec<_> = group.iter().map(|peer| {
                        let shared = &shared;
                        let secret = &secret;
                        scope.spawn(move || {
                            let confirmed = match deliver(shared, peer, secret, expires) {
                                Ok(()) => true,
                                Err(error) => {
                                    tracing::debug!(peer = %crate::default_name::short_id(&peer.device_id), %error, "Private delivery unconfirmed");
                                    false
                                }
                            };
                            tracing::info!(peer = %crate::default_name::short_id(&peer.device_id), confirmed, "Private delivery confirmation");
                            shared.emit(SyncEvent::PrivateDelivery { peer: peer.device_id.clone(), confirmed });
                            usize::from(confirmed)
                        })
                    }).collect();
                    tasks.into_iter().map(|task| task.join().unwrap_or(0)).sum::<usize>()
                });
            }
            drop(secret);
            shared.private_sending.store(false, Ordering::SeqCst);
            shared.emit(if count == 0 {
                SyncEvent::PrivateFailed
            } else {
                SyncEvent::PrivateSent { count }
            });
        });
    if spawned.is_err() {
        shared.private_sending.store(false, Ordering::SeqCst);
    }
    spawned.is_ok()
}

pub(super) fn respond<S: Read + Write>(
    stream: S,
    shared: &Shared,
    private: bool,
) -> Result<(), SyncError> {
    let mut channel = accept(stream, &shared.identity, |key| {
        lock(&shared.book).contains(key)
    })?;
    if !private {
        if channel.receive_limited(64)? != REQUEST {
            return Err(SyncError::Handshake);
        }
        channel.send(
            if shared.private_receiver.load(Ordering::SeqCst) && shared.private_vault.accepting() {
                AVAILABLE
            } else {
                b"disabled"
            },
        )?;
        let _ = channel.receive_limited(8)?;
        return Ok(());
    }
    if !shared.private_receiver.load(Ordering::SeqCst) {
        return Err(SyncError::Handshake);
    }
    let bytes = Zeroizing::new(channel.receive_limited(MAX_SECRET_BYTES + 8)?);
    let (header, text) = bytes.split_at_checked(8).ok_or(SyncError::Handshake)?;
    let expires = i64::from_be_bytes(header.try_into().map_err(|_| SyncError::Handshake)?);
    let remaining = expires.saturating_sub(now_millis());
    if remaining <= 0 {
        return Err(SyncError::Handshake);
    }
    let text = std::str::from_utf8(text).map_err(|_| SyncError::Handshake)?;
    let secret = Secret::new(text.to_owned()).ok_or(SyncError::Handshake)?;
    // 关闭接收、锁屏或退出与网络回调并发时由保险箱的撤销状态阻止迟到内容。
    if !shared.private_receiver.load(Ordering::SeqCst) {
        return Err(SyncError::Handshake);
    }
    let permission = lock(&shared.private_gate);
    let book = lock(&shared.book);
    if !shared.private_receiver.load(Ordering::SeqCst)
        || !book.contains(channel.remote())
        || !shared
            .private_vault
            .store(secret, Duration::from_millis(remaining as u64))
    {
        return Err(SyncError::Handshake);
    }
    drop(book);
    drop(permission);
    shared.emit(SyncEvent::PrivateReceived);
    channel.send(ACK)?;
    let _ = channel.receive_limited(8)?;
    Ok(())
}

fn deliver(shared: &Shared, peer: &Peer, secret: &Secret, expires: i64) -> Result<(), SyncError> {
    if !shared.private_running.load(Ordering::SeqCst) || now_millis() >= expires {
        return Err(SyncError::Handshake);
    }
    let address = lock(&shared.nearby)
        .get(&peer.device_id)
        .map(|value| value.address)
        .or_else(|| peer.last_address.as_deref()?.parse().ok());
    if let Some(address) = address {
        let lan = || {
            let stream = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT)?;
            stream.set_read_timeout(Some(IO_TIMEOUT))?;
            stream.set_write_timeout(Some(IO_TIMEOUT))?;
            Ok::<_, SyncError>(stream)
        };
        if lan()
            .and_then(|stream| capable(shared, peer, stream))
            .unwrap_or(false)
        {
            // 发送开始后不再切换通道重试，避免对端已收但确认丢失造成重复。
            return send(shared, peer, lan()?, secret, expires);
        }
    }
    let config = lock(&shared.relay).clone();
    // 配对完成时在线清单可能尚未推送；让中转按此刻真实登记状态立即接通，不缓存或补发。
    if !config.enabled || !shared.relay_connected.load(Ordering::SeqCst) {
        return Err(SyncError::Handshake);
    }
    let relay =
        || crate::relay::client::dial(&config, &shared.identity.device_id(), &peer.device_id);
    if !capable(shared, peer, relay()?)? {
        return Err(SyncError::Handshake);
    }
    send(shared, peer, relay()?, secret, expires)
}

fn capable<S: Read + Write>(shared: &Shared, peer: &Peer, stream: S) -> Result<bool, SyncError> {
    let mut channel = connect_mode(
        stream,
        &shared.identity,
        |key| device_id_of(key) == peer.device_id,
        Opening::Capabilities,
    )?;
    channel.send(REQUEST)?;
    let enabled = channel.receive_limited(64)? == AVAILABLE;
    channel.send(b"done")?;
    Ok(enabled)
}

fn send<S: Read + Write>(
    shared: &Shared,
    peer: &Peer,
    stream: S,
    secret: &Secret,
    expires: i64,
) -> Result<(), SyncError> {
    if !shared.private_running.load(Ordering::SeqCst) || now_millis() >= expires {
        return Err(SyncError::Handshake);
    }
    let mut channel = connect_mode(
        stream,
        &shared.identity,
        |key| device_id_of(key) == peer.device_id,
        Opening::Private,
    )?;
    let mut payload = Zeroizing::new(Vec::with_capacity(8 + secret.as_str().len()));
    payload.extend_from_slice(&expires.to_be_bytes());
    payload.extend_from_slice(secret.as_str().as_bytes());
    channel.send(&payload)?;
    if channel.receive_limited(64)? != ACK {
        return Err(SyncError::Handshake);
    }
    // 对端已经确认入槽；收尾帧失败不能把已确认送达改报失败，更不能因此重发。
    let _ = channel.send(b"done");
    Ok(())
}
