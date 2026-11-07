//! 从一台逐步加入六台，验证第四台及后续设备不会被固定名单或私密并发上限遗漏。

use super::device::Device;
use super::{TOKEN, until};
use crate::{Record, SyncEvent, private::Secret};
use std::collections::HashMap;
use std::time::Duration;

#[test]
fn newly_paired_fourth_fifth_and_sixth_devices_exchange_normal_and_private_content() {
    let relay =
        super::super::RelayServer::start("127.0.0.1:0".parse().unwrap(), TOKEN.into()).unwrap();
    let url = format!("ws://{}/v1/clipboard", relay.address());
    let mut devices: Vec<Device> = Vec::new();
    let seed = Record::text("synthetic-history-before-expansion".into(), 1_000);
    for _ in 0..6 {
        let added = Device::new(&url, TOKEN);
        added.service().set_private_receiver(true);
        for known in &devices {
            until(|| {
                added
                    .service()
                    .relay_peers()
                    .contains(&known.service().device_id())
            });
            let code = known.service().begin_pairing().unwrap();
            assert!(
                added
                    .service()
                    .join_target(&format!("relay:{}", known.service().device_id()), code)
            );
            assert!(matches!(
                added.wait(|event| matches!(
                    event,
                    SyncEvent::Paired { .. } | SyncEvent::PairFailed
                )),
                SyncEvent::Paired { .. }
            ));
            known.wait(|event| matches!(event, SyncEvent::Paired { .. }));
        }
        devices.push(added);
        if devices.len() == 1 {
            devices[0].add(seed.clone());
        }
        for device in &devices {
            assert_eq!(device.service().peers().len(), devices.len() - 1);
        }
        until(|| {
            devices
                .last()
                .unwrap()
                .memory
                .0
                .lock()
                .unwrap()
                .contains_key(&seed.stamp.id)
        });
    }
    for (index, source) in devices.iter().enumerate() {
        let ordinary = Record::text(
            format!("synthetic-normal-source-{index}"),
            2_000 + index as i64,
        );
        source.add(ordinary.clone());
        until(|| {
            devices.iter().all(|target| {
                target
                    .memory
                    .0
                    .lock()
                    .unwrap()
                    .contains_key(&ordinary.stamp.id)
            })
        });

        let text = format!("synthetic-private-expansion-{index}");
        assert!(
            source
                .service()
                .send_private(Secret::new(text.clone()).unwrap())
        );
        source.wait(|event| *event == SyncEvent::PrivateSendStarted);
        let mut receipts = HashMap::new();
        loop {
            let event = source.events.recv_timeout(Duration::from_secs(45)).unwrap();
            match event {
                SyncEvent::PrivateDelivery { peer, confirmed } => {
                    receipts.insert(peer, confirmed);
                }
                SyncEvent::PrivateSent { count } => {
                    assert_eq!(count, devices.len() - 1);
                    break;
                }
                SyncEvent::PrivateFailed => panic!("private delivery failed in expanded mesh"),
                _ => {}
            }
        }
        assert_eq!(receipts.len(), devices.len() - 1);
        for (target_index, target) in devices.iter().enumerate() {
            if target_index != index {
                assert_eq!(receipts.get(&target.service().device_id()), Some(&true));
                assert_eq!(
                    target.service().private_vault().take().unwrap().as_str(),
                    text
                );
            }
            assert!(!target.service().private_vault().pending());
            assert!(
                target
                    .memory
                    .0
                    .lock()
                    .unwrap()
                    .values()
                    .all(|record| !record.text.contains("synthetic-private-expansion"))
            );
        }
    }
}
