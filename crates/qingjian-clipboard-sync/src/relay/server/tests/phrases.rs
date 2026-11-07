//! 没有 LAN 地址的三台设备，经中转追补短语，无码分组及较大快照也能双向合并。

use super::{Device, TOKEN, until};
use qingjian_learning::{Phrase, SyncedPhrases};
use std::sync::{Arc, Mutex};

#[test]
fn relay_phrases_catch_up_for_a_new_device_without_clipboard_records() {
    let relay =
        super::super::RelayServer::start("127.0.0.1:0".parse().unwrap(), TOKEN.into()).unwrap();
    let url = format!("ws://{}/v1/clipboard", relay.address());
    let mut devices: Vec<Device> = Vec::new();
    let mut local = Vec::new();
    for index in 0..3 {
        let added = Device::new(&url, TOKEN);
        let phrases = Arc::new(Mutex::new(vec![Phrase {
            text: format!("fixture-{index}-{}", "x".repeat(40000)),
            group: "工作".into(),
            code: if index == 1 {
                String::new()
            } else {
                "hello".into()
            },
            weight: 10 + index,
            enabled: true,
        }]));
        let source = phrases.clone();
        added.service().set_phrase_source(
            &added.directory,
            Box::new(move || Ok(source.lock().unwrap().clone())),
        );
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
            added.wait(|event| matches!(event, crate::SyncEvent::Paired { .. }));
            known.wait(|event| matches!(event, crate::SyncEvent::Paired { .. }));
        }
        local.push(phrases);
        devices.push(added);
    }
    for device in &devices {
        device.service().notify_changed();
    }
    until(|| {
        devices.iter().zip(&local).all(|(device, local)| {
            SyncedPhrases::new(&device.directory)
                .merged(&local.lock().unwrap())
                .unwrap()
                .len()
                == 3
        })
    });
    for device in &devices {
        assert!(device.memory.0.lock().unwrap().is_empty());
    }
}
