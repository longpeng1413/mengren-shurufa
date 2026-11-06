//! 用两台无局域网地址的合成设备验证真实 WSS 配对、双向同步和解除配对。

mod memory;
use memory::Memory;
use qingjian_clipboard_sync::relay::RelayConfig;
use qingjian_clipboard_sync::{Identity, Record, SyncOptions, SyncService};
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn wait(condition: impl Fn() -> bool) -> Result<(), Box<dyn std::error::Error>> {
    let end = Instant::now() + Duration::from_secs(45);
    while !condition() {
        if Instant::now() > end {
            return Err("Live relay check timed out".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = RelayConfig {
        enabled: true,
        url: std::env::var("MENGREN_IME_RELAY_URL")?,
        token: std::env::var("MENGREN_IME_RELAY_TOKEN")?,
    };
    config.validate()?;
    let parent = std::env::temp_dir().join(format!(
        "mengren-ime-probe-{}",
        Identity::generate()?.device_id()
    ));
    let mut services = Vec::new();
    let mut replicas = Vec::new();
    let mut receivers = Vec::new();
    for index in 0..2 {
        let replica = Memory::default();
        let (sender, receiver) = mpsc::channel();
        let service = SyncService::start(
            SyncOptions {
                directory: parent.join(index.to_string()),
                name: format!("synthetic-{index}"),
                model: String::new(),
                own_discovery: false,
                relay: config.clone(),
            },
            Box::new(replica.clone()),
            sender,
        )?;
        services.push(service);
        replicas.push(replica);
        receivers.push(receiver);
    }
    let result = (|| {
        wait(|| services.iter().all(|service| service.relay_connected()))?;
        wait(|| services[1].relay_peers().contains(&services[0].device_id()))?;
        println!("WSS registration and discovery: verified");
        let code = services[0].begin_pairing()?;
        if !services[1].join_target(&format!("relay:{}", services[0].device_id()), code) {
            return Err("Pairing target rejected".into());
        }
        wait(|| services.iter().all(|service| service.peers().len() == 1))?;
        if services
            .iter()
            .any(|service| service.peers()[0].last_address.is_some())
        {
            return Err("Unexpected LAN route".into());
        }
        println!("Six-digit PAKE pairing and Noise identity verification: verified");
        for index in 0..2 {
            let record = Record::text(
                format!("synthetic private relay probe direction {index}"),
                index as i64 + 1,
            );
            let key = record.stamp.id.clone();
            replicas[index]
                .0
                .lock()
                .unwrap()
                .insert(key.clone(), record);
            services[index].notify_changed();
            wait(|| replicas[1 - index].0.lock().unwrap().contains_key(&key))?;
        }
        wait(|| {
            services
                .iter()
                .all(|service| service.peer_states()[0].via_relay)
        })?;
        println!("Bidirectional encrypted text via relay only: verified");
        services[0].unpair(&services[1].device_id())?;
        services[1].notify_changed();
        wait(|| services[1].peers().is_empty())?;
        println!("Unpair refusal: verified");
        Ok::<(), Box<dyn std::error::Error>>(())
    })();
    drop(services);
    drop(receivers);
    let _ = std::fs::remove_dir_all(parent);
    result
}
