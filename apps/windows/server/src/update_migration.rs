//! 首次迁移到猛人签名更新源时开启检查，此后保留用户开关选择。

pub fn enable_once(config: Option<&std::path::Path>) {
    let Some(path) = config else { return };
    let marker = path.with_file_name("mengren-update-source-v1");
    if marker.exists() {
        return;
    }
    if qingjian_platform::Config::set_bool(path, "update", "check", true).is_ok() {
        let _ = std::fs::write(marker, b"mengren-github-signed-v1");
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn enables_new_source_once_and_then_preserves_user_choice() {
        let directory =
            std::env::temp_dir().join(format!("mengren-update-migration-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("config.toml");
        std::fs::write(&path, "[update]\ncheck = false\n").unwrap();
        super::enable_once(Some(&path));
        assert!(qingjian_platform::Config::load(&path).unwrap().update.check);
        qingjian_platform::Config::set_bool(&path, "update", "check", false).unwrap();
        super::enable_once(Some(&path));
        assert!(!qingjian_platform::Config::load(&path).unwrap().update.check);
        let _ = std::fs::remove_file(directory.join("mengren-update-source-v1"));
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_dir(directory);
    }
}
