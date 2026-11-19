//! 每次接收占用独立目录，只有校验完成的文件才发布。缓存清理只处理自己的条目。
use std::path::Path;
use std::time::Duration;

pub(super) fn valid_name(name: &str) -> bool {
    let stem = name
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end()
        .to_ascii_uppercase();
    let device = matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || ["COM", "LPT"].iter().any(|prefix| {
        stem.strip_prefix(prefix).is_some_and(|n| {
            matches!(
                n,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        })
    });
    !name.is_empty()
        && name.len() <= 255
        && !name.ends_with([' ', '.'])
        && !device
        && !name
            .chars()
            .any(|c| c.is_control() || r#"/\:*?"<>|"#.contains(c))
}

pub(super) fn clean(root: &Path, startup: bool) {
    let Ok(entries) = root.read_dir() else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Ok(meta) = entry.path().symlink_metadata() else {
            continue;
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        let expired = meta
            .modified()
            .ok()
            .and_then(|t| t.elapsed().ok())
            .unwrap_or_default();
        if name.starts_with("item-") && meta.is_dir() && expired > Duration::from_secs(7 * 86400) {
            let _ = std::fs::remove_dir_all(entry.path());
        } else if name.starts_with("partial-")
            && meta.is_file()
            && (startup || expired > Duration::from_secs(86400))
        {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}
