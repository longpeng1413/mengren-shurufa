//! 回环公网中转集成测试，明确不提供 LAN 地址，保证数据实际经过中转。

mod device;
mod large;
mod memory;
mod private;
use crate::{Record, SyncEvent};
use device::Device;
use std::time::{Duration, Instant};

const TOKEN: &str = "synthetic-relay-test-token-123456789";

#[track_caller]
fn until(condition: impl Fn() -> bool) {
    let end = Instant::now() + Duration::from_secs(45);
    while !condition() {
        assert!(Instant::now() < end, "condition in time");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn token_comparison_rejects_wrong_and_short_values() {
    assert!(super::token_matches(
        "Bearer example-token",
        "Bearer example-token"
    ));
    assert!(!super::token_matches(
        "Bearer example-token2",
        "Bearer example-token"
    ));
    assert!(!super::token_matches("", "Bearer example-token"));
}

#[test]
fn relay_pairs_syncs_text_images_and_recovers() {
    let mut server =
        Some(super::RelayServer::start("127.0.0.1:0".parse().unwrap(), TOKEN.into()).unwrap());
    let address = server.as_ref().unwrap().address();
    let url = format!("ws://{address}/v1/clipboard");
    let host = Device::new(&url, TOKEN);
    let phone = Device::new(&url, TOKEN);
    until(|| host.service().relay_connected() && phone.service().relay_connected());
    until(|| {
        phone
            .service()
            .relay_peers()
            .contains(&host.service().device_id())
    });
    let target = format!("relay:{}", host.service().device_id());
    let code = host.service().begin_pairing().unwrap();
    assert!(phone.service().join_target(
        &target,
        if code == "000000" { "111111" } else { "000000" }.into()
    ));
    phone.wait(|event| *event == SyncEvent::PairFailed);
    assert!(host.service().peers().is_empty());
    let code = host.service().begin_pairing().unwrap();
    assert!(phone.service().join_target(&target, code));
    phone.wait(|event| matches!(event, SyncEvent::Paired { .. }));
    host.wait(|event| matches!(event, SyncEvent::Paired { .. }));
    assert!(phone.service().peers()[0].last_address.is_none());
    let first = Record::text("手机测试 https://example.com".into(), 100);
    phone.add(first.clone());
    until(|| host.memory.0.lock().unwrap().contains_key(&first.stamp.id));
    let second = Record::text("desktop reply".into(), 200);
    host.add(second.clone());
    until(|| {
        phone
            .memory
            .0
            .lock()
            .unwrap()
            .contains_key(&second.stamp.id)
    });
    let image = Record::image(vec![7; 2 * 1024 * 1024], "png".into(), 300);
    let large = Record::text("x".repeat(2 * 1024 * 1024), 400);
    host.add(image.clone());
    host.add(large.clone());
    until(|| phone.memory.0.lock().unwrap().contains_key(&image.stamp.id));
    until(|| phone.memory.0.lock().unwrap().contains_key(&large.stamp.id));
    server.take();
    until(|| !host.service().relay_connected());
    let offline = Record::text("queued on device only".into(), 500);
    phone.add(offline.clone());
    server = Some(super::RelayServer::start(address, TOKEN.into()).unwrap());
    until(|| {
        host.memory
            .0
            .lock()
            .unwrap()
            .contains_key(&offline.stamp.id)
    });
    assert!(server.is_some());
    host.service().unpair(&phone.service().device_id()).unwrap();
    phone.service().notify_changed();
    until(|| phone.service().peers().is_empty());
}

#[test]
fn wrong_token_never_registers() {
    let server = super::RelayServer::start("127.0.0.1:0".parse().unwrap(), TOKEN.into()).unwrap();
    let config = crate::relay::RelayConfig {
        enabled: true,
        url: format!("ws://{}/v1/clipboard", server.address()),
        token: "wrong-token-with-enough-length".into(),
    };
    let id = crate::Identity::generate().unwrap().device_id();
    assert!(super::super::socket::open(&config, &format!("register/{id}")).is_err());
}

#[test]
fn plain_http_health_check_works_without_websocket_headers() {
    use std::io::{Read, Write};
    let server = super::RelayServer::start("127.0.0.1:0".parse().unwrap(), TOKEN.into()).unwrap();
    let mut tcp = std::net::TcpStream::connect(server.address()).unwrap();
    tcp.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    tcp.write_all(b"GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut response = String::new();
    tcp.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 200 OK"));
    assert!(response.ends_with("{\"ok\":true,\"online\":0}"));
}
