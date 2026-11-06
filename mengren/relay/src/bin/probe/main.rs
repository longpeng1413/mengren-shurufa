//! 用两台无局域网地址的合成设备验证真实 WSS 配对、双向同步和解除配对。

mod memory;
use memory::Memory;
use qingjian_clipboard_sync::relay::RelayConfig;
use qingjian_clipboard_sync::{Identity, Record, SyncOptions, SyncService};
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn wait(condition: impl Fn() -> bool) -> Result<(), Box<dyn std::error::Error>> {
    let end = Instant::now() + Duration::from_secs(300);
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
        let image_size = std::env::var("MENGREN_IME_PROBE_IMAGE_BYTES")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(2 * 1024 * 1024);
        if image_size >= qingjian_clipboard_sync::relay::MAX_CONTENT_BYTES {
            return Err("Probe image exceeds content limit".into());
        }
        let image = Record::image(vec![0x42; image_size], "png".into(), 300);
        let image_id = image.stamp.id.clone();
        replicas[0]
            .0
            .lock()
            .unwrap()
            .insert(image_id.clone(), image);
        services[0].notify_changed();
        wait(|| replicas[1].0.lock().unwrap().contains_key(&image_id))?;
        println!("Encrypted image payload ({image_size} bytes): verified");
        for service in &services {
            service.set_private_receiver(true);
        }
        for source in 0..2 {
            let expected = format!("synthetic-one-time-private-probe-{source}");
            if !services[source].private_sender().send(
                qingjian_clipboard_sync::private::Secret::new(expected.clone())
                    .ok_or("Invalid synthetic secret")?,
            ) {
                return Err("Private send not started".into());
            }
            wait(|| services[1 - source].private_vault().pending())?;
            let secret = services[1 - source]
                .private_vault()
                .take()
                .ok_or("Missing private payload")?;
            if secret.as_str() != expected || services[1 - source].private_vault().pending() {
                return Err("Private payload mismatch".into());
            }
            if replicas.iter().any(|replica| {
                replica
                    .0
                    .lock()
                    .unwrap()
                    .values()
                    .any(|record| record.text.contains("synthetic-one-time-private"))
            }) {
                return Err("Private payload leaked into replica".into());
            }
        }
        println!(
            "Bidirectional private single-use memory delivery, excluded from history: verified"
        );
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
