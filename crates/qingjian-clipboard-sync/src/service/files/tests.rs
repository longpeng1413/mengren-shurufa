//! 文件名、边界与认证直连回归。
use super::{FILE_LIMIT, cache, wire};

#[cfg(windows)]
#[test]
#[ignore = "requires the address of a real LAN test peer"]
fn physical_windows_file_socket() {
    let address = std::env::var("MENGREN_TEST_LAN_ENDPOINT")
        .unwrap()
        .parse()
        .unwrap();
    let route = super::Route::resolve(&super::Policy::default(), address)
        .expect("on-link physical adapter");
    let stream = route.connect(address).expect("pinned TCP socket");
    assert_eq!(stream.local_addr().unwrap().ip(), route.local);
    assert_eq!(stream.ttl().unwrap(), 1);
}

#[test]
fn filenames_are_single_portable_components() {
    for name in [
        "中文 未知.ABc",
        "无后缀",
        "a.tar.gz",
        ".secret",
        "文件.pdf",
        "a.docx",
        "a.xmind",
        "a.mp3",
        "a.mp4",
        "a.zip",
        "a.apk",
        "a.exe",
    ] {
        assert!(cache::valid_name(name), "{name}");
    }
    for name in [
        "",
        "..",
        "../escape",
        "C:evil",
        "a/b",
        "a\\b",
        "CON",
        "LPT1.txt",
        "a.",
    ] {
        assert!(!cache::valid_name(name), "{name}");
    }
}

#[test]
fn lengths_and_integrity() {
    for len in [FILE_LIMIT, FILE_LIMIT + 1] {
        assert!(wire::send(std::io::empty(), Some(len), |_| panic!("sent data")).is_err());
    }
    for bytes in [b"".as_slice(), b"pdf test"] {
        let mut frames = Vec::new();
        wire::send(bytes, None, |b| {
            frames.push(b.to_vec());
            Ok(())
        })
        .unwrap();
        let mut data = Vec::new();
        let mut it = frames.clone().into_iter();
        wire::receive(&mut data, Some(bytes.len() as u64), |_| {
            Ok(it.next().unwrap())
        })
        .unwrap();
        assert_eq!(data, bytes);
        frames.last_mut().unwrap()[8] ^= 1;
        let mut it = frames.into_iter();
        assert!(wire::receive(std::io::sink(), None, |_| Ok(it.next().unwrap())).is_err());
    }
}

#[test]
fn full_destination_and_source_permission_error_propagate() {
    struct Full;
    impl std::io::Write for Full {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::StorageFull.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    struct Revoked;
    impl std::io::Read for Revoked {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::PermissionDenied.into())
        }
    }
    assert!(wire::receive(Full, None, |_| Ok(vec![9])).is_err());
    assert!(wire::send(Revoked, None, |_| panic!("unexpected completed frame")).is_err());
}

#[test]
fn unknown_length_stops_at_the_first_forbidden_byte() {
    use std::io::Read;
    let mut input = std::io::repeat(123).take(FILE_LIMIT + 1);
    let mut sent = 0;
    assert!(
        wire::send(&mut input, None, |frame| {
            assert!(!frame.is_empty());
            sent += frame.len() as u64;
            Ok(())
        })
        .is_err()
    );
    assert_eq!(input.limit(), 1);
    assert!(sent < FILE_LIMIT);
}
