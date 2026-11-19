//! 已认证对端中途断开、撤销授权、破损摘要和断网时都回收临时文件。
use super::{address, device, wait_for};
use crate::SyncEvent;
use crate::channel::{Opening, connect_mode};
use std::net::TcpStream;
use std::time::{Duration, Instant};

#[test]
fn interrupted_or_revoked_receiver_discards_partial() {
    let a = device("file-failure","sender");
    let b = device("file-failure","receiver");
    a.service.join_pairing(address(&b),b.service.begin_pairing().unwrap());
    wait_for(&a,|e|matches!(e,SyncEvent::Paired{..}));
    wait_for(&b,|e|matches!(e,SyncEvent::Paired{..}));
    b.service.set_files_enabled(true).unwrap();
    let root=b.service.file_inbox();
    for fault in ["disconnect","revoke","network","digest","length"] {
        b.service.test_file_loopback(true);
        b.service.select_file_peer(&a.service.device_id(),true).unwrap();
        let tcp=TcpStream::connect(address(&b)).unwrap();
        tcp.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut ch=connect_mode(tcp,&a.service.shared.identity,|k|k==&b.service.shared.identity.public,Opening::LanFile).unwrap();
        assert_eq!(ch.receive_limited(32).unwrap(),b"lan-file-receiver-v1");
        ch.send(br#"["temporary.zip",4]"#).unwrap();
        ch.send(b"one").unwrap();
        let end=Instant::now()+Duration::from_secs(5);
        while !root.read_dir().is_ok_and(|mut entries|entries.next().is_some()) {
            assert!(Instant::now()<end); std::thread::sleep(Duration::from_millis(5));
        }
        match fault {
            "revoke" => { b.service.select_file_peer(&a.service.device_id(),false).unwrap();let _=ch.send(b"x"); }
            "network" => {b.service.test_file_loopback(false);let _=ch.send(b"x");}
            "digest" => {ch.send(b"x").unwrap();ch.send(b"").unwrap();let _=ch.send(&[0;40]);}
            "length" => {let _=ch.send(b"too long");}
            _ => {}
        }
        drop(ch);
        wait_for(&b,|e|matches!(e,SyncEvent::FileStatus("failed")));
        assert_eq!(root.read_dir().unwrap().count(),0,"{fault}");
        assert!(b.items.lock().unwrap().is_empty());
    }
}
