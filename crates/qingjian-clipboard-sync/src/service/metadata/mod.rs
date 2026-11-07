//! 公钥认证后的名称交换。旧客户端拒绝未知模式时保留原配对和普通同步。

mod hello;
#[cfg(test)]
mod tests;

use super::{CONNECT_TIMEOUT, IO_TIMEOUT, SyncEvent, device_id_of, lock, state::Shared};
use crate::channel::{Opening, SecureChannel, accept, connect_mode};
use crate::{Peer, SyncError};
use hello::DeviceHello;
use std::io::{Read, Write};
use std::net::TcpStream;

fn exchange<S: Read + Write>(
    channel: &mut SecureChannel<S>,
    shared: &Shared,
) -> Result<(), SyncError> {
    let own = DeviceHello {
        version: 1,
        name: shared.name.clone(),
    };
    if !own.valid() {
        return Err(SyncError::Protocol("invalid local device name".into()));
    }
    channel.send(&serde_json::to_vec(&own)?)?;
    let remote: DeviceHello = serde_json::from_slice(&channel.receive_limited(2048)?)?;
    if !remote.valid() {
        return Err(SyncError::Protocol("invalid remote device name".into()));
    }
    // 只更新仍在名单中的已认证公钥；发现广播或中转路由信息不能改配对显示名。
    let changed = lock(&shared.book).rename(channel.remote(), &remote.name)?;
    if changed {
        shared.emit(SyncEvent::NearbyChanged);
    }
    // 双方确认读完名称后再关连接，避免 Windows 最后一帧被 close 重置。
    channel.send(&[1])?;
    if channel.receive_limited(1)? != [1] {
        return Err(SyncError::Protocol(
            "invalid device name acknowledgement".into(),
        ));
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
    exchange(&mut channel, shared)
}

fn initiate<S: Read + Write>(stream: S, shared: &Shared, peer: &Peer) -> Result<(), SyncError> {
    let mut channel = connect_mode(
        stream,
        &shared.identity,
        |key| device_id_of(key) == peer.device_id,
        Opening::Metadata,
    )?;
    exchange(&mut channel, shared)
}

pub(in crate::service) fn sync_peer(shared: &Shared, peer: &Peer) {
    let address = lock(&shared.nearby)
        .get(&peer.device_id)
        .map(|value| value.address)
        .or_else(|| peer.last_address.as_deref()?.parse().ok());
    let lan = || -> Result<(), SyncError> {
        let address = address.ok_or(SyncError::Handshake)?;
        let stream = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT)?;
        stream.set_read_timeout(Some(IO_TIMEOUT))?;
        stream.set_write_timeout(Some(IO_TIMEOUT))?;
        initiate(stream, shared, peer)
    };
    if lan().is_err() && lock(&shared.relay_peers).contains(&peer.device_id) {
        let config = lock(&shared.relay).clone();
        if config.enabled {
            let _ =
                crate::relay::client::dial(&config, &shared.identity.device_id(), &peer.device_id)
                    .and_then(|stream| initiate(stream, shared, peer));
        }
    }
}
