//! CF_HDROP 仅接受用户复制的单个普通文件。
use qingjian_clipboard_sync::SyncService;
use std::{fs::File, path::Path};

pub(crate) fn send_files(service: &SyncService, paths: Vec<String>) {
    let [path] = paths.as_slice() else {
        super::listener::notice("files.single");
        return;
    };
    let path = Path::new(path);
    if !path.is_file() {
        super::listener::notice("files.single");
        return;
    }
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        super::listener::notice("files.invalid");
        return;
    };
    match File::open(path).and_then(|file| Ok((file.metadata()?.len(), file))) {
        Ok((size, file)) => service.send_file(file, name.to_owned(), Some(size)),
        Err(_) => super::listener::notice("files.failed"),
    }
}
