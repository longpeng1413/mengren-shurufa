//! 三台无 LAN 路由设备：私密内容只到明确开启接收的设备，不进任何普通副本或文件。

use super::device::Device;
use super::{TOKEN, until};
use crate::{SyncEvent, private::Secret};

fn pair(host: &Device, phone: &Device) {
    until(|| {
        phone
            .service()
            .relay_peers()
            .contains(&host.service().device_id())
    });
    let code = host.service().begin_pairing().unwrap();
    assert!(
        phone
            .service()
            .join_target(&format!("relay:{}", host.service().device_id()), code)
    );
    let paired =
        phone.wait(|event| matches!(event, SyncEvent::Paired { .. } | SyncEvent::PairFailed));
    assert!(matches!(paired, SyncEvent::Paired { .. }), "{paired:?}");
    host.wait(|event| matches!(event, SyncEvent::Paired { .. }));
}

#[test]
fn private_delivery_is_authenticated_online_single_use_and_never_history() {
    let server =
        super::super::RelayServer::start("127.0.0.1:0".parse().unwrap(), TOKEN.into()).unwrap();
    let url = format!("ws://{}/v1/clipboard", server.address());
    let host = Device::new(&url, TOKEN);
    let phone = Device::new(&url, TOKEN);
    let other_phone = Device::new(&url, TOKEN);
    host.service().set_private_receiver(true);
    pair(&host, &phone);
    pair(&other_phone, &phone);
    assert!(
        phone
            .service()
            .send_private(Secret::new("synthetic-secret-甲-42".into()).unwrap())
    );
    let sent = phone.wait(|event| {
        matches!(
            event,
            SyncEvent::PrivateSent { .. } | SyncEvent::PrivateFailed
        )
    });
    assert_eq!(sent, SyncEvent::PrivateSent { count: 1 });
    assert_eq!(
        host.service().private_vault().take().unwrap().as_str(),
        "synthetic-secret-甲-42"
    );
    assert!(!host.service().private_vault().pending());
    assert!(!other_phone.service().private_vault().pending());
    for device in [&host, &phone, &other_phone] {
        assert!(device.memory.0.lock().unwrap().is_empty());
        for file in std::fs::read_dir(&device.directory).unwrap() {
            let bytes = std::fs::read(file.unwrap().path()).unwrap();
            assert!(
                !bytes
                    .windows(b"synthetic-secret".len())
                    .any(|part| part == b"synthetic-secret")
            );
        }
    }
    host.service().private_vault().suspend();
    assert!(
        phone
            .service()
            .send_private(Secret::new("locked".into()).unwrap())
    );
    phone.wait(|event| *event == SyncEvent::PrivateFailed);
    assert!(!host.service().private_vault().pending());
    host.service().private_vault().resume();
    host.service().set_private_receiver(false);
    assert!(
        phone
            .service()
            .send_private(Secret::new("disabled".into()).unwrap())
    );
    phone.wait(|event| *event == SyncEvent::PrivateFailed);
    host.service().set_private_receiver(true);
    phone.service().notify_changed();
    std::thread::sleep(std::time::Duration::from_secs(1));
    assert!(!host.service().private_vault().pending());
    host.service().unpair(&phone.service().device_id()).unwrap();
    assert!(
        phone
            .service()
            .send_private(Secret::new("unpaired".into()).unwrap())
    );
    phone.wait(|event| *event == SyncEvent::PrivateFailed);
    assert!(!host.service().private_vault().pending());
}
