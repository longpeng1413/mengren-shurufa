//! 猛人输入法的独立在线中转。只读环境变量，不输出令牌或用户数据。

use qingjian_clipboard_sync::relay::server::RelayServer;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let address = std::env::var("MENGREN_IME_RELAY_BIND")
        .unwrap_or_else(|_| "127.0.0.1:18319".into())
        .parse()?;
    let token = std::env::var("MENGREN_IME_RELAY_TOKEN")
        .map_err(|_| "MENGREN_IME_RELAY_TOKEN is required")?;
    let server = RelayServer::start(address, token)?;
    println!("Mengren IME relay listening on {}", server.address());
    loop {
        std::thread::park();
    }
}
