//! 名称只能由已认证公钥更新，并验证落盘、未知设备拒绝及旧端兼容。

use super::*;
use crate::service::locked::LockedReplica;
use crate::sync::Replica;
use crate::{Identity, PeerBook, Record, Stamp};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Arc, mpsc::channel};

struct Empty;
impl Replica for Empty {
    fn stamps(&self) -> Result<Vec<Stamp>, SyncError> {
        Ok(vec![])
    }
    fn record(&self, _: &str) -> Result<Option<Record>, SyncError> {
        Ok(None)
    }
    fn apply(&mut self, _: Record) -> Result<(), SyncError> {
        unreachable!()
    }
}
fn device(name: &str) -> (Arc<Shared>, PathBuf) {
    let directory = std::env::temp_dir().join(format!(
        "qj-name-{}",
        Identity::generate().unwrap().device_id()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let identity = Identity::load_or_create(&directory.join("identity.key")).unwrap();
    let book = PeerBook::open(&directory.join("peers.json")).unwrap();
    (
        Arc::new(Shared::new(
            identity,
            name.into(),
            String::new(),
            0,
            book,
            LockedReplica::new(Box::new(Empty)),
            channel().0,
        )),
        directory,
    )
}
fn round(a: &Arc<Shared>, b: &Arc<Shared>, legacy: bool) -> Result<(), SyncError> {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let b = b.clone();
    let thread = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        assert_eq!(
            crate::channel::read_opening(&mut stream).unwrap(),
            Opening::Metadata
        );
        if legacy {
            return Ok(());
        }
        respond(stream, &b)
    });
    let stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(2)))
        .unwrap();
    let peer = lock(&a.book).peers()[0].clone();
    let result = initiate(stream, a, &peer);
    let remote = thread.join().unwrap();
    result.and(remote)
}

#[test]
fn authenticated_rename_preserves_pair_identity_and_persists() {
    let (a, ad) = device("四屏电脑-新名字");
    let (b, bd) = device("猛人的魅族 📱 Meizu");
    lock(&a.book)
        .add(
            &b.identity.public,
            "旧手机".into(),
            Some("127.0.0.1:1234".into()),
        )
        .unwrap();
    lock(&b.book)
        .add(&a.identity.public, "旧电脑".into(), None)
        .unwrap();
    round(&a, &b, false).unwrap();
    let book = PeerBook::open(&ad.join("peers.json")).unwrap();
    assert_eq!(book.peers().len(), 1);
    assert_eq!(book.peers()[0].name, b.name);
    assert_eq!(book.peers()[0].device_id, b.identity.device_id());
    assert_eq!(
        book.peers()[0].last_address.as_deref(),
        Some("127.0.0.1:1234")
    );
    assert_eq!(lock(&b.book).peers()[0].name, a.name);
    assert!(round(&a, &b, true).is_err());
    assert_eq!(lock(&a.book).peers().len(), 1);
    lock(&b.book).remove(&a.identity.device_id()).unwrap();
    assert!(round(&a, &b, false).is_err());
    assert!(lock(&b.book).peers().is_empty());
    drop((a, b));
    std::fs::remove_dir_all(ad).unwrap();
    std::fs::remove_dir_all(bd).unwrap();
}

#[test]
fn malformed_names_are_rejected_before_storage() {
    for name in [String::new(), "bad\nname".into(), "a".repeat(257)] {
        assert!(!DeviceHello { version: 1, name }.valid());
    }
}
