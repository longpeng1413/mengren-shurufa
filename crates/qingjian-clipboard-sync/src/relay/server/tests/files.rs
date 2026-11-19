//! 真实 TCP、Noise 和只记录首帧类型的测试中转。回环仅在测试构建中模拟物理接口。
use super::{Device, TOKEN, until};
use crate::relay::server::RelayServer;
use crate::{Record, SyncEvent};
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, Write};

fn pair(a: &Device, b: &Device) {
    a.service().join_pairing(
        ([127, 0, 0, 1], b.service().port()).into(),
        b.service().begin_pairing().unwrap(),
    );
    a.wait(|e| matches!(e, SyncEvent::Paired { .. }));
    b.wait(|e| matches!(e, SyncEvent::Paired { .. }));
}

fn allow(a: &Device, b: &Device) {
    a.service().set_files_enabled(true).unwrap();
    a.service()
        .select_file_peer(&b.service().device_id(), true)
        .unwrap();
}

fn send(a: &Device, name: &str, data: &[u8]) {
    let mut file = tempfile::tempfile().unwrap();
    file.write_all(data).unwrap();
    file.rewind().unwrap();
    a.service()
        .send_file(file, name.into(), Some(data.len() as u64));
}

fn received(b: &Device) -> std::path::PathBuf {
    let SyncEvent::FileReceived { path, .. } =
        b.wait(|e| matches!(e, SyncEvent::FileReceived { .. }))
    else {
        unreachable!()
    };
    path
}

#[test]
fn selected_only_no_relay_no_replay_and_history_still_syncs() {
    let server = RelayServer::start("127.0.0.1:0".parse().unwrap(), TOKEN.into()).unwrap();
    let address = server.address();
    let url = format!("ws://{address}/v1/clipboard");
    let a = Device::new(&url, TOKEN);
    let b = Device::new(&url, TOKEN);
    let c = Device::new(&url, TOKEN);
    for device in [&a, &b, &c] {
        assert!(!device.service().files_enabled());
        device.service().test_file_loopback(true);
    }
    pair(&a, &b);
    pair(&b, &c);
    pair(&a, &c);
    allow(&a, &b);
    allow(&b, &a);
    allow(&b, &c);
    allow(&c, &b);
    allow(&c, &a);
    for (source, destination, name, data) in [
        (&a, &b, "中文 图片.JpG", b"raw jpeg".as_slice()),
        (&b, &a, "资料 无后缀", b"".as_slice()),
        (&b, &c, "笔记.xmind", b"mind".as_slice()),
        (&c, &a, "混合.UnKnOwN", b"data".as_slice()),
    ] {
        if std::ptr::eq(source, &b) && std::ptr::eq(destination, &a) {
            b.service()
                .select_file_peer(&c.service().device_id(), false)
                .unwrap();
        }
        if std::ptr::eq(source, &b) && std::ptr::eq(destination, &c) {
            b.service()
                .select_file_peer(&a.service().device_id(), false)
                .unwrap();
            allow(&b, &c);
        }
        if std::ptr::eq(source, &c) && std::ptr::eq(destination, &a) {
            c.service()
                .select_file_peer(&b.service().device_id(), false)
                .unwrap();
            allow(&a, &c);
        }
        send(source, name, data);
        let file = received(destination);
        assert_eq!(file.file_name().unwrap().to_str(), Some(name));
        assert_eq!(std::fs::read(&file).unwrap(), data);
        assert_eq!(source.wait(|e| matches!(e,SyncEvent::FileStatus(_))),SyncEvent::FileStatus("sent"),"{name}");
        if std::ptr::eq(source, &a) {
            assert!(!c.service().file_inbox().exists());
            send(source, name, b"second");
            let next = received(destination);
            assert_ne!(file, next);
            assert_eq!(std::fs::read(file).unwrap(), data);
            source.wait(|e| *e == SyncEvent::FileStatus("sent"));
        }
    }
    assert!(
        [&a, &b, &c]
            .iter()
            .all(|d| d.memory.0.lock().unwrap().is_empty())
    );
    until(|| a.service().relay_connected() && b.service().relay_connected());
    for dev in [&a, &b, &c] {
        dev.service().test_file_loopback(false);
        dev.service().test_forget_direct_routes();
    }
    send(&a, "never-relay-private.pdf", b"local-file-canary");
    a.wait(|e| *e == SyncEvent::FileStatus("unavailable"));
    // 取消直接地址，使普通记录确实走测试中转；重启中转再对账。
    let count = b.service().file_inbox().read_dir().unwrap().count();
    let probe = Record::text("relay-before-reconnect".into(), 99);
    a.add(probe.clone());
    until(|| b.memory.0.lock().unwrap().contains_key(&probe.stamp.id));
    assert!(server.recorder.openings.lock().unwrap().contains(&1));
    assert!(!server.recorder.openings.lock().unwrap().contains(&8));
    drop(server);
    until(|| !a.service().relay_connected());
    let server = RelayServer::start(address, TOKEN.into()).unwrap();
    until(|| a.service().relay_connected() && b.service().relay_connected());
    a.service().test_file_loopback(true);
    a.service().notify_changed();
    for r in [
        Record::text("ordinary text".into(), 100),
        Record::image(vec![1, 2, 3], "png".into(), 200),
    ] {
        a.add(r.clone());
        until(|| b.memory.0.lock().unwrap().contains_key(&r.stamp.id));
    }
    assert_eq!(b.service().file_inbox().read_dir().unwrap().count(), count);
    assert_eq!(b.memory.0.lock().unwrap().len(), 3);
    assert!(server.recorder.openings.lock().unwrap().contains(&1));
    assert!(!server.recorder.openings.lock().unwrap().contains(&8));
}

#[test]
#[ignore = "explicit near-limit throughput and peak-memory run"]
fn stream_299999999_over_authenticated_tcp() {
    let server = RelayServer::start("127.0.0.1:0".parse().unwrap(), TOKEN.into()).unwrap();
    let url = format!("ws://{}/v1/clipboard", server.address());
    let a = Device::new(&url, TOKEN);
    let b = Device::new(&url, TOKEN);
    a.service().test_file_loopback(true);
    b.service().test_file_loopback(true);
    pair(&a, &b);
    allow(&a, &b);
    allow(&b, &a);
    let mut input = tempfile::tempfile().unwrap();
    let mut digest = Sha256::new();
    let buffer: Vec<_> = (0..65536).map(|i| (i % 251) as u8).collect();
    let mut left = 299_999_999;
    while left > 0 {
        let n = left.min(buffer.len());
        input.write_all(&buffer[..n]).unwrap();
        digest.update(&buffer[..n]);
        left -= n;
    }
    input.rewind().unwrap();
    a.service().send_file(input, "近上限.Mp4".into(), None);
    let p = received(&b);
    assert_eq!(std::fs::metadata(&p).unwrap().len(), 299_999_999);
    let mut file = std::fs::File::open(p).unwrap();
    let mut output = Sha256::new();
    let mut chunk = [0; 65536];
    loop {
        let n = file.read(&mut chunk).unwrap();
        if n == 0 {
            break;
        }
        output.update(&chunk[..n]);
    }
    let actual = output.finalize();
    let expected = digest.finalize();
    assert_eq!(actual, expected);
    println!("bytes=299999999 source={expected:x} received={actual:x}");
}
