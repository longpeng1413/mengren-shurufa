//! 三台设备全部开启接收，覆盖每个方向、逐台单次消费、禁收和禁止转发历史。

use super::device::Device;
use super::{TOKEN, until};
use crate::{SyncEvent, private::Secret};

#[test]
fn every_paired_device_can_send_and_receive_privately() {
    let server =
        super::super::RelayServer::start("127.0.0.1:0".parse().unwrap(), TOKEN.into()).unwrap();
    let url = format!("ws://{}/v1/clipboard", server.address());
    let devices = [
        Device::new(&url, TOKEN),
        Device::new(&url, TOKEN),
        Device::new(&url, TOKEN),
    ];
    for device in &devices {
        device.service().set_private_receiver(true);
    }
    for first in 0..3 {
        for second in first + 1..3 {
            until(|| {
                devices[second]
                    .service()
                    .relay_peers()
                    .contains(&devices[first].service().device_id())
            });
            let code = devices[first].service().begin_pairing().unwrap();
            assert!(devices[second].service().join_target(
                &format!("relay:{}", devices[first].service().device_id()),
                code
            ));
            devices[second].wait(|event| matches!(event, SyncEvent::Paired { .. }));
            devices[first].wait(|event| matches!(event, SyncEvent::Paired { .. }));
        }
    }
    for source in 0..3 {
        let text = format!("synthetic-private-matrix-{source}");
        assert!(
            devices[source]
                .service()
                .private_sender()
                .send(Secret::new(text.clone()).unwrap())
        );
        let mut receipts = std::collections::HashMap::new();
        devices[source].wait(|event| *event == SyncEvent::PrivateSendStarted);
        let event = loop {
            let event = devices[source]
                .events
                .recv_timeout(std::time::Duration::from_secs(30))
                .unwrap();
            if let SyncEvent::PrivateDelivery { peer, confirmed } = &event {
                receipts.insert(peer.clone(), *confirmed);
            }
            if matches!(
                event,
                SyncEvent::PrivateSent { .. } | SyncEvent::PrivateFailed
            ) {
                break event;
            }
        };
        assert_eq!(event, SyncEvent::PrivateSent { count: 2 });
        assert_eq!(receipts.len(), 2);
        for (index, target) in devices.iter().enumerate() {
            if index != source {
                assert_eq!(receipts.get(&target.service().device_id()), Some(&true));
            }
        }
        for (index, target) in devices.iter().enumerate() {
            if index != source {
                assert_eq!(
                    target.service().private_vault().take().unwrap().as_str(),
                    text
                );
            }
            assert!(!target.service().private_vault().pending());
            assert!(target.memory.0.lock().unwrap().is_empty());
        }
    }
    devices[2].service().set_private_available(false);
    assert!(
        devices[0]
            .service()
            .send_private(Secret::new("blocked-by-screen-off".into()).unwrap())
    );
    assert_eq!(
        devices[0].wait(|event| matches!(
            event,
            SyncEvent::PrivateSent { .. } | SyncEvent::PrivateFailed
        )),
        SyncEvent::PrivateSent { count: 1 }
    );
    devices[1].service().private_vault().clear();
    devices[2].service().set_private_receiver(false);
    devices[2].service().set_private_available(true);
    assert!(!devices[2].service().private_vault().accepting());
    devices[2].service().set_private_receiver(true);
    for device in &devices {
        device.service().notify_changed();
    }
    std::thread::sleep(std::time::Duration::from_secs(1));
    assert!(!devices[2].service().private_vault().pending());
    for device in &devices {
        for file in std::fs::read_dir(&device.directory).unwrap() {
            let bytes = std::fs::read(file.unwrap().path()).unwrap();
            assert!(
                !bytes
                    .windows(b"synthetic-private-matrix".len())
                    .any(|value| value == b"synthetic-private-matrix")
            );
        }
    }
}
