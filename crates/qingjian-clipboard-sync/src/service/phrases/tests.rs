//! 经真实 TCP 与 Noise 双向传输短语，重连去重并拒绝未配对发送端。

use super::*;
use crate::service::locked::LockedReplica;
use crate::sync::Replica;
use crate::{Identity, PeerBook};
use crate::{Record, Stamp};
use std::{
    net::TcpListener,
    sync::{Arc, Mutex, mpsc::channel},
};

struct Empty;
impl Replica for Empty {
    fn stamps(&self) -> Result<Vec<Stamp>, SyncError> {
        Ok(vec![])
    }
    fn record(&self, _: &str) -> Result<Option<Record>, SyncError> {
        Ok(None)
    }
    fn apply(&mut self, _: Record) -> Result<(), SyncError> {
        panic!("phrases must not enter clipboard")
    }
}

fn shared(name: &str) -> (Arc<Shared>, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("qj-phrase-wire-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let identity = Identity::load_or_create(&dir.join("identity.key")).unwrap();
    let book = PeerBook::open(&dir.join("peers.json")).unwrap();
    let shared = Shared::new(
        identity,
        name.into(),
        String::new(),
        0,
        book,
        LockedReplica::new(Box::new(Empty)),
        channel().0,
    );
    (Arc::new(shared), dir)
}

fn round(a: &Arc<Shared>, b: &Arc<Shared>) -> Result<(), SyncError> {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let remote = b.clone();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(3)))
            .unwrap();
        assert_eq!(
            crate::channel::read_opening(&mut stream).unwrap(),
            Opening::Phrases
        );
        respond(stream, &remote)
    });
    let stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(3)))
        .unwrap();
    let peer = Peer {
        device_id: b.identity.device_id(),
        name: "b".into(),
        last_address: None,
        last_seen: None,
    };
    let result = initiate(stream, a, &peer);
    let remote = server.join().unwrap();
    result.and(remote)
}

#[test]
fn two_way_dedup_offline_additions_and_revoked_pairing() {
    let (a, ad) = shared("a");
    let (b, bd) = shared("b");
    lock(&a.book)
        .add(&b.identity.public, "b".into(), None)
        .unwrap();
    lock(&b.book)
        .add(&a.identity.public, "a".into(), None)
        .unwrap();
    let pa = Arc::new(Mutex::new(vec![Phrase {
        text: "coded fixture".into(),
        group: "默认".into(),
        code: "hello".into(),
        weight: 77,
        enabled: true,
    }]));
    let pb = Arc::new(Mutex::new(vec![Phrase {
        text: "tap-only\nfixture".into(),
        group: "work".into(),
        code: String::new(),
        weight: 10,
        enabled: true,
    }]));
    for (s, d, p) in [(&a, &ad, &pa), (&b, &bd, &pb)] {
        let p = p.clone();
        *lock(&s.phrases) = Some(PhraseState {
            directory: d.clone(),
            source: Box::new(move || Ok(lock(&p).clone())),
            received: Default::default(),
        });
    }
    round(&a, &b).unwrap();
    round(&a, &b).unwrap();
    assert_eq!(SyncedPhrases::new(&ad).merged(&lock(&pa)).unwrap().len(), 2);
    assert_eq!(SyncedPhrases::new(&bd).merged(&lock(&pb)).unwrap().len(), 2);
    lock(&pa).push(Phrase {
        text: "offline fixture".into(),
        group: "默认".into(),
        code: "later".into(),
        weight: 19,
        enabled: true,
    });
    round(&a, &b).unwrap();
    assert_eq!(SyncedPhrases::new(&bd).merged(&lock(&pb)).unwrap().len(), 3);
    lock(&b.book).remove(&a.identity.device_id()).unwrap();
    assert!(round(&a, &b).is_err());
    drop(a);
    drop(b);
    std::fs::remove_dir_all(ad).unwrap();
    std::fs::remove_dir_all(bd).unwrap();
}
