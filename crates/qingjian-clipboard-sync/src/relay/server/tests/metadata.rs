//! 仅经中转的设备保留身份重启后同步中文新名称，无须再次配对。

use super::{Device, TOKEN, until};
use crate::{Record, SyncEvent, SyncOptions, SyncService};
use std::sync::mpsc::channel;

fn restart(device: &mut Device, url: &str, name: &str) {
    device.service.take();
    let (sender, events) = channel();
    device.service = Some(
        SyncService::start(
            SyncOptions {
                directory: device.directory.clone(),
                name: name.into(),
                model: String::new(),
                own_discovery: false,
                relay: crate::relay::RelayConfig {
                    enabled: true,
                    url: url.into(),
                    token: TOKEN.into(),
                },
            },
            Box::new(device.memory.clone()),
            sender,
        )
        .unwrap(),
    );
    device.events = events;
}

#[test]
fn renamed_devices_exchange_names_over_relay_without_pair_reset() {
    let relay =
        super::super::RelayServer::start("127.0.0.1:0".parse().unwrap(), TOKEN.into()).unwrap();
    let url = format!("ws://{}/v1/clipboard", relay.address());
    let mut pc = Device::new(&url, TOKEN);
    let mut phone = Device::new(&url, TOKEN);
    until(|| {
        phone
            .service()
            .relay_peers()
            .contains(&pc.service().device_id())
    });
    let code = pc.service().begin_pairing().unwrap();
    assert!(
        phone
            .service()
            .join_target(&format!("relay:{}", pc.service().device_id()), code)
    );
    phone.wait(|e| matches!(e, SyncEvent::Paired { .. }));
    pc.wait(|e| matches!(e, SyncEvent::Paired { .. }));
    let id = phone.service().device_id();
    restart(&mut phone, &url, "猛人的魅族手机");
    until(|| pc.service().peers()[0].name == "猛人的魅族手机");
    assert_eq!(phone.service().device_id(), id);
    restart(&mut pc, &url, "四屏电脑-新名称");
    until(|| phone.service().peers()[0].name == "四屏电脑-新名称");
    assert_eq!(phone.service().peers().len(), 1);
    assert_eq!(pc.service().peers().len(), 1);
    let record = Record::text("synthetic-copy-after-rename".into(), 42);
    phone.add(record.clone());
    until(|| pc.memory.0.lock().unwrap().contains_key(&record.stamp.id));
}
