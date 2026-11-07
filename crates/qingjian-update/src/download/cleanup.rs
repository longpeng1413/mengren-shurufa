//! 只回收本应用摘要命名、超过一天的更新缓存；不跟随符号链接或递归删目录。

use std::path::Path;
use std::time::{Duration, SystemTime};

fn owned(name: &str) -> bool {
    let name = name.strip_suffix(".part").unwrap_or(name);
    let Some(hash) = name
        .strip_suffix(".apk")
        .or_else(|| name.strip_suffix(".exe"))
    else {
        return false;
    };
    hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit())
}

pub(super) fn prune(cache: &Path, now: SystemTime) {
    let Ok(entries) = std::fs::read_dir(cache) else {
        return;
    };
    for entry in entries.flatten() {
        if !entry.file_name().to_str().is_some_and(owned)
            || !entry.file_type().is_ok_and(|kind| kind.is_file())
        {
            continue;
        }
        if entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|at| now.duration_since(at).ok())
            .is_some_and(|age| age > Duration::from_secs(86400))
        {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interrupted_download_cleanup_preserves_recent_packages_and_unrelated_files() {
        let dir = std::env::temp_dir().join(format!("qj-update-prune-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let now = SystemTime::now();
        let old = now - Duration::from_secs(172800);
        let expired = format!("{}.apk", "a".repeat(64));
        let partial = format!("{}.exe.part", "b".repeat(64));
        let recent = format!("{}.exe", "c".repeat(64));
        for name in [
            &expired,
            &partial,
            &recent,
            "personal-backup.exe",
            "config.json",
        ] {
            let file = std::fs::File::create(dir.join(name)).unwrap();
            file.set_times(std::fs::FileTimes::new().set_modified(if *name == recent {
                now
            } else {
                old
            }))
            .unwrap();
        }
        let nested = dir.join(format!("{}.apk", "d".repeat(64)));
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("keep.txt"), b"fixture").unwrap();
        prune(&dir, now);
        assert!(!dir.join(expired).exists());
        assert!(!dir.join(partial).exists());
        for name in [&recent, "personal-backup.exe", "config.json"] {
            assert!(dir.join(name).exists());
        }
        assert!(nested.join("keep.txt").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
