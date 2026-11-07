//! 独立的自定义短语协议：认证已配对公钥、摘要对账、限长合并，不触碰系统剪贴板。

mod source;
mod state;
#[cfg(test)]
mod tests;
pub use source::PhraseSource;
pub(crate) use state::PhraseState;

use super::{
    CONNECT_TIMEOUT, IO_TIMEOUT, SyncEvent, SyncService, device_id_of, lock, state::Shared,
};
use crate::channel::{Opening, SecureChannel, accept, connect_mode};
use crate::{Peer, SyncError};
use qingjian_learning::{Phrase, SyncedPhrases};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    net::TcpStream,
    path::Path,
};

const MAX_BYTES: usize = 8 * 1024 * 1024;

impl SyncService {
    /// 跟随设备同步开关；关闭设备同步会销毁服务和适配器。
    pub fn set_phrase_source(&self, directory: &Path, source: Box<dyn PhraseSource>) {
        *lock(&self.shared.phrases) = Some(PhraseState {
            directory: directory.into(),
            source,
            received: Default::default(),
        });
        self.notify_changed();
    }
}

fn snapshot(shared: &Shared) -> Result<Vec<u8>, SyncError> {
    let guard = lock(&shared.phrases);
    let Some(state) = guard.as_ref() else {
        return Ok(b"[]".to_vec());
    };
    let mut phrases = SyncedPhrases::new(&state.directory).merged(&state.source.snapshot()?)?;
    phrases.sort_by_key(SyncedPhrases::id);
    let bytes = serde_json::to_vec(&phrases).map_err(|_| SyncError::Handshake)?;
    if bytes.len() > MAX_BYTES {
        return Err(SyncError::Handshake);
    }
    Ok(bytes)
}

fn exchange<S: Read + Write>(
    shared: &Shared,
    channel: &mut SecureChannel<S>,
    initiator: bool,
) -> Result<(), SyncError> {
    let id = device_id_of(channel.remote());
    let bytes = snapshot(shared)?;
    let digest = Sha256::digest(&bytes);
    let enabled = lock(&shared.phrases).is_some();
    let mut hello = vec![u8::from(enabled)];
    hello.extend_from_slice(&digest);
    channel.send(&hello)?;
    let remote = channel.receive_limited(33)?;
    if remote.len() != 33 || remote[0] > 1 {
        return Err(SyncError::Handshake);
    }
    if !enabled || remote[0] == 0 {
        return Ok(());
    }
    let want = lock(&shared.phrases).as_ref().is_some_and(|s| {
        s.received
            .get(&id)
            .is_none_or(|old| old.as_slice() != &remote[1..])
    });
    channel.send(&[u8::from(want)])?;
    let request = channel.receive_limited(1)?;
    if request.len() != 1 || request[0] > 1 {
        return Err(SyncError::Handshake);
    }
    let send = |channel: &mut SecureChannel<S>| -> Result<(), SyncError> {
        if request[0] == 1 {
            channel.send(&bytes)?;
            if channel.receive_limited(1)? != [1] {
                return Err(SyncError::Handshake);
            }
        }
        Ok(())
    };
    let receive = |channel: &mut SecureChannel<S>| -> Result<(), SyncError> {
        if want {
            let bytes = channel.receive_limited(MAX_BYTES)?;
            if Sha256::digest(&bytes).as_slice() != &remote[1..] {
                return Err(SyncError::Handshake);
            }
            let phrases: Vec<Phrase> =
                serde_json::from_slice(&bytes).map_err(|_| SyncError::Handshake)?;
            // 解除配对与入库串行，认证后被撤销的连接不能再写短语。
            let book = lock(&shared.book);
            if !book.contains(channel.remote()) {
                return Err(SyncError::UnknownPeer);
            }
            let mut guard = lock(&shared.phrases);
            let state = guard.as_mut().ok_or(SyncError::Handshake)?;
            let count = SyncedPhrases::new(&state.directory).receive(&phrases)?;
            state.received.insert(id.clone(), remote[1..].to_vec());
            drop(guard);
            drop(book);
            if count > 0 {
                shared.emit(SyncEvent::PhrasesReceived { count });
            }
            channel.send(&[1])?;
        }
        Ok(())
    };
    if initiator {
        send(channel)?;
        receive(channel)?;
    } else {
        receive(channel)?;
        send(channel)?;
    }
    Ok(())
}

pub(in crate::service) fn respond<S: Read + Write>(
    stream: S,
    shared: &Shared,
) -> Result<(), SyncError> {
    let mut channel = accept(stream, &shared.identity, |key| {
        lock(&shared.book).contains(key)
    })?;
    exchange(shared, &mut channel, false)
}

pub(in crate::service) fn sync_peer(shared: &Shared, peer: &Peer) {
    if lock(&shared.phrases).is_none() {
        return;
    }
    let address = lock(&shared.nearby)
        .get(&peer.device_id)
        .map(|p| p.address)
        .or_else(|| peer.last_address.as_deref()?.parse().ok());
    let attempt = || -> Result<(), SyncError> {
        let address = address.ok_or(SyncError::Handshake)?;
        let stream = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT)?;
        stream.set_read_timeout(Some(IO_TIMEOUT))?;
        stream.set_write_timeout(Some(IO_TIMEOUT))?;
        initiate(stream, shared, peer)
    };
    if attempt().is_err() && lock(&shared.relay_peers).contains(&peer.device_id) {
        let config = lock(&shared.relay).clone();
        if config.enabled {
            let _ =
                crate::relay::client::dial(&config, &shared.identity.device_id(), &peer.device_id)
                    .and_then(|stream| initiate(stream, shared, peer));
        }
    }
    // 旧版不识别 mode=6；不影响普通/私密同步，也不删除配对。
}

fn initiate<S: Read + Write>(stream: S, shared: &Shared, peer: &Peer) -> Result<(), SyncError> {
    let mut channel = connect_mode(
        stream,
        &shared.identity,
        |key| device_id_of(key) == peer.device_id,
        Opening::Phrases,
    )?;
    exchange(shared, &mut channel, true)
}
