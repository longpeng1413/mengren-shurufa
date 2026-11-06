//! 在线字节流中转；不落盘、不解密，限连接数、帧大小、时长和总流量。

mod broker;
mod running;
#[cfg(test)]
mod tests;

pub use running::RelayServer;

use crate::service::lock;
use broker::Broker;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, atomic::Ordering, mpsc};
use std::time::{Duration, Instant};
use tungstenite::{
    Message, WebSocket,
    handshake::server::{ErrorResponse, Request, Response},
};

fn token_matches(actual: &str, expected: &str) -> bool {
    use sha2::{Digest, Sha256};
    let a = Sha256::digest(actual.as_bytes());
    let b = Sha256::digest(expected.as_bytes());
    a.iter().zip(b).fold(0u8, |diff, (a, b)| diff | (*a ^ b)) == 0
}

fn response(status: u16, body: &str) -> ErrorResponse {
    tungstenite::http::Response::builder()
        .status(status)
        .header("Content-Type", "application/json")
        .body(Some(body.to_owned()))
        .expect("valid constant status")
}

// tungstenite 的回调规定直接返回 HTTP 响应，无法在这里装箱错误。
#[allow(clippy::result_large_err)]
fn handle(
    mut tcp: TcpStream,
    token: &str,
    broker: &Arc<Broker>,
    stop: &std::sync::atomic::AtomicBool,
) {
    let mut route = String::new();
    let _ = tcp.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = tcp.set_write_timeout(Some(Duration::from_secs(3)));
    let mut prefix = [0u8; 256];
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match tcp.peek(&mut prefix) {
            Ok(0) | Err(_) => return,
            Ok(length) if prefix[..length].contains(&b'\n') => {
                if prefix[..length].starts_with(b"GET /healthz ") {
                    let mut request = Vec::new();
                    while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                        let mut chunk = [0u8; 1024];
                        let Ok(count) = tcp.read(&mut chunk) else {
                            return;
                        };
                        if count == 0 || request.len() + count > 8192 {
                            return;
                        }
                        request.extend_from_slice(&chunk[..count]);
                    }
                    let body = format!("{{\"ok\":true,\"online\":{}}}", lock(&broker.online).len());
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = tcp.write_all(response.as_bytes());
                    return;
                }
                break;
            }
            Ok(length) if length == prefix.len() || Instant::now() >= deadline => return,
            Ok(_) => std::thread::sleep(Duration::from_millis(2)),
        }
    }
    let limits = tungstenite::protocol::WebSocketConfig::default()
        .max_message_size(Some(super::MAX_FRAME))
        .max_frame_size(Some(super::MAX_FRAME));
    let accepted = tungstenite::accept_hdr_with_config(
        tcp,
        |request: &Request, reply: Response| {
            if request.uri().path() == "/healthz" {
                return Err(response(
                    200,
                    &format!("{{\"ok\":true,\"online\":{}}}", lock(&broker.online).len()),
                ));
            }
            let auth = request
                .headers()
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default();
            if !token_matches(auth, &format!("Bearer {token}")) {
                return Err(response(401, "{\"error\":\"unauthorized\"}"));
            }
            if request.uri().query().is_some() {
                return Err(response(400, "{}"));
            }
            route = request
                .uri()
                .path()
                .strip_prefix("/v1/clipboard/")
                .unwrap_or_default()
                .to_owned();
            let parts: Vec<_> = route.split('/').collect();
            if !matches!(parts.as_slice(), ["register", id] if super::valid_id(id))
                && !matches!(parts.as_slice(), ["connect", source, target] if super::valid_id(source) && super::valid_id(target) && source != target)
                && !matches!(parts.as_slice(), ["accept", target, session] if super::valid_id(target) && super::valid_id(session))
            {
                return Err(response(404, "{}"));
            }
            Ok(reply)
        },
        Some(limits),
    );
    let Ok(mut socket) = accepted else {
        return;
    };
    let parts: Vec<_> = route.split('/').collect();
    match parts.as_slice() {
        ["register", id] => register(socket, id, broker, stop),
        ["connect", source, target] => {
            let Ok(identity) = crate::Identity::generate() else {
                return;
            };
            let session = identity.device_id();
            let (sender, receiver) = mpsc::sync_channel(1);
            let recipient = lock(&broker.online)
                .get(*target)
                .map(|(_, sender)| sender.clone());
            if !lock(&broker.online).contains_key(*source) {
                return;
            }
            {
                let mut pending = lock(&broker.pending);
                if pending.len() >= 16 {
                    return;
                }
                pending.insert(session.clone(), (target.to_string(), sender));
            }
            let notified = recipient.is_some_and(|sender| {
                sender
                    .try_send(serde_json::json!({"incoming":session}).to_string())
                    .is_ok()
            });
            let peer = notified
                .then(|| receiver.recv_timeout(Duration::from_secs(8)).ok())
                .flatten();
            lock(&broker.pending).remove(&session);
            if let Some(mut peer) = peer {
                let _ = socket.send(Message::text("ready"));
                let _ = peer.send(Message::text("ready"));
                bridge(socket, peer, stop);
            }
        }
        ["accept", target, session] => {
            let entry = {
                let mut pending = lock(&broker.pending);
                if pending
                    .get(*session)
                    .is_some_and(|(expected, _)| expected == target)
                {
                    pending.remove(*session)
                } else {
                    None
                }
            };
            if let Some((_, sender)) = entry {
                let _ = sender.try_send(socket);
            }
        }
        _ => {}
    }
}

fn register(
    mut socket: WebSocket<TcpStream>,
    id: &str,
    broker: &Broker,
    stop: &std::sync::atomic::AtomicBool,
) {
    let Ok(nonce) = crate::Identity::generate().map(|id| id.device_id()) else {
        return;
    };
    let (sender, receiver) = mpsc::sync_channel(8);
    {
        let mut online = lock(&broker.online);
        if online.len() >= 16 && !online.contains_key(id) {
            return;
        }
        if let Some((_, old)) = online.insert(id.to_owned(), (nonce.clone(), sender)) {
            let _ = old.try_send("replaced".into());
        }
    }
    let _ = socket
        .get_ref()
        .set_read_timeout(Some(Duration::from_millis(200)));
    let mut published = Instant::now() - Duration::from_secs(5);
    let mut alive = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        if published.elapsed() >= Duration::from_secs(2) {
            let peers: Vec<_> = lock(&broker.online)
                .keys()
                .filter(|peer| peer.as_str() != id)
                .cloned()
                .collect();
            if socket
                .send(Message::text(
                    serde_json::json!({"peers":peers}).to_string(),
                ))
                .is_err()
            {
                break;
            }
            published = Instant::now();
        }
        while let Ok(message) = receiver.try_recv() {
            if message == "replaced" {
                let _ = socket.close(None);
                break;
            }
            if socket.send(Message::text(message)).is_err() {
                break;
            }
        }
        match socket.read() {
            Ok(Message::Ping(_) | Message::Pong(_)) => {
                alive = Instant::now();
                let _ = socket.flush();
            }
            Ok(Message::Close(_)) => break,
            Ok(_) => break,
            Err(error) if super::socket::timeout(&error) => {}
            Err(_) => break,
        }
        if alive.elapsed() > Duration::from_secs(40) {
            break;
        }
    }
    let mut online = lock(&broker.online);
    if online.get(id).is_some_and(|(current, _)| *current == nonce) {
        online.remove(id);
    }
}

fn bridge(
    mut a: WebSocket<TcpStream>,
    mut b: WebSocket<TcpStream>,
    stop: &std::sync::atomic::AtomicBool,
) {
    let _ = a.get_ref().set_read_timeout(Some(Duration::from_millis(2)));
    let _ = b.get_ref().set_read_timeout(Some(Duration::from_millis(2)));
    let start = Instant::now();
    let mut bytes = 0usize;
    'session: while start.elapsed() < Duration::from_secs(30) && !stop.load(Ordering::Relaxed) {
        if !pump(&mut a, &mut b, &mut bytes) || !pump(&mut b, &mut a, &mut bytes) {
            break 'session;
        }
    }
    let _ = a.close(None);
    let _ = b.close(None);
}

fn pump(from: &mut WebSocket<TcpStream>, to: &mut WebSocket<TcpStream>, bytes: &mut usize) -> bool {
    match from.read() {
        Ok(Message::Binary(data)) => {
            *bytes = bytes.saturating_add(data.len());
            *bytes <= 16 * 1024 * 1024 && to.send(Message::Binary(data)).is_ok()
        }
        Ok(Message::Ping(_)) => from.flush().is_ok(),
        Ok(Message::Pong(_)) => true,
        Err(error) if super::socket::timeout(&error) => true,
        _ => false,
    }
}
