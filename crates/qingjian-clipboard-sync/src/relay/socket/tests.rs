//! 合成回环服务器验证首个地址失败后继续另一地址族，不改变真实网络或 DNS。

use super::*;
use std::net::TcpListener;

fn fallback(first: &str, working: &str) {
    let rejected = TcpListener::bind(first).unwrap();
    let address = rejected.local_addr().unwrap();
    drop(rejected);
    let listener = TcpListener::bind(working).unwrap();
    let working = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (tcp, _) = listener.accept().unwrap();
        tcp.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let mut socket = tungstenite::accept(tcp).unwrap();
        socket
            .send(tungstenite::Message::text("synthetic-ready"))
            .unwrap();
        assert_eq!(socket.read().unwrap().into_text().unwrap(), "received");
    });
    let config = RelayConfig {
        enabled: true,
        url: format!("ws://127.0.0.1:{}/v1/clipboard", working.port()),
        token: "synthetic-dual-stack-test-token-123456".into(),
    };
    let mut socket = open_resolved(&config, "register/fixture", |_, _| {
        Ok(vec![address, working])
    })
    .unwrap();
    assert_eq!(
        socket.read().unwrap().into_text().unwrap(),
        "synthetic-ready"
    );
    socket.send(tungstenite::Message::text("received")).unwrap();
    server.join().unwrap();
}

#[test]
fn ipv4_unavailable_continues_to_ipv6() {
    fallback("127.0.0.1:0", "[::1]:0");
}

#[test]
fn ipv6_unavailable_continues_to_ipv4() {
    fallback("[::1]:0", "127.0.0.1:0");
}
