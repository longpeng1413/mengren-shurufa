//! 大图片真实中转传输与三端普通同步，合成字节不访问用户剪贴板。

use super::device::Device;
use super::{TOKEN, until};
use crate::{Record, SyncEvent};

#[test]
#[ignore = "100 MB transfer; run explicitly in release mode"]
fn hundred_megabyte_boundary_and_three_device_images() {
    let server =
        super::super::RelayServer::start("127.0.0.1:0".parse().unwrap(), TOKEN.into()).unwrap();
    let url = format!("ws://{}/v1/clipboard", server.address());
    let host = Device::new(&url, TOKEN);
    let phone = Device::new(&url, TOKEN);
    let third = Device::new(&url, TOKEN);
    for device in [&phone, &third] {
        until(|| {
            device
                .service()
                .relay_peers()
                .contains(&host.service().device_id())
        });
        let code = host.service().begin_pairing().unwrap();
        assert!(
            device
                .service()
                .join_target(&format!("relay:{}", host.service().device_id()), code)
        );
        device.wait(|event| matches!(event, SyncEvent::Paired { .. }));
        host.wait(|event| matches!(event, SyncEvent::Paired { .. }));
    }
    let record = Record::image(
        vec![0x42; crate::relay::MAX_CONTENT_BYTES - 1],
        "png".into(),
        100,
    );
    let id = record.stamp.id.clone();
    host.add(record);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(240);
    while ![&phone, &third]
        .iter()
        .all(|device| device.memory.0.lock().unwrap().contains_key(&id))
    {
        assert!(
            std::time::Instant::now() < deadline,
            "large transfer completed in time"
        );
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    for device in [&phone, &third] {
        let memory = device.memory.0.lock().unwrap();
        let received = memory.get(&id).unwrap();
        assert_eq!(received.image.len(), crate::relay::MAX_CONTENT_BYTES - 1);
        assert!(received.is_consistent());
    }
}
