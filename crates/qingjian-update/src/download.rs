//! 下载前重新获取并验证签名索引，流式写入、校验大小与 SHA256 后才交给安装器。

use crate::{Asset, Target, UpdateError};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

const MAX_PACKAGE: u64 = 512 * 1024 * 1024;
const ASSET_PREFIX: &str = "https://github.com/longpeng1413/mengren-shurufa/releases/download/";

pub fn download_release(
    current: &str,
    target: Target,
    beta: bool,
    expected: &str,
    cache: &Path,
    proxy: &str,
    progress: impl Fn(u64, u64) -> bool,
) -> Result<PathBuf, UpdateError> {
    let offer = crate::check_with_proxy(current, target, beta, proxy)?
        .filter(|offer| offer.version == expected)
        .ok_or(UpdateError::InvalidPackage)?;
    let asset = offer.asset.ok_or(UpdateError::InvalidPackage)?;
    validate(&asset, expected)?;
    if !progress(0, asset.size) {
        return Err(UpdateError::Cancelled);
    }
    std::fs::create_dir_all(cache)?;
    // 只清理本应用缓存中超过一天的安装包与残片，不涉及用户文件。
    if let Ok(entries) = std::fs::read_dir(cache) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Ok(meta) = entry.metadata()
                && meta.is_file()
                && !entry.file_type().is_ok_and(|kind| kind.is_symlink())
                && matches!(
                    path.extension().and_then(|extension| extension.to_str()),
                    Some("apk" | "exe" | "part")
                )
                && meta
                    .modified()
                    .ok()
                    .and_then(|time| time.elapsed().ok())
                    .is_some_and(|age| age > Duration::from_secs(86400))
            {
                let _ = std::fs::remove_file(path);
            }
        }
    }
    // 目的文件名取摘要，远程文件名、版本号均不参与本机路径拼接。
    let extension = match target.platform {
        "windows" => "exe",
        "android" => "apk",
        _ => return Err(UpdateError::InvalidPackage),
    };
    let destination = cache.join(format!(
        "{}.{}",
        asset.sha256.to_ascii_lowercase(),
        extension
    ));
    if verified(&destination, &asset).is_ok() {
        if !progress(asset.size, asset.size) {
            return Err(UpdateError::Cancelled);
        }
        return Ok(destination);
    }
    let partial = destination.with_extension(format!("{extension}.part"));
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async {
            let client = crate::http::builder(proxy)?
                .connect_timeout(Duration::from_secs(20))
                .read_timeout(Duration::from_secs(30))
                .timeout(Duration::from_secs(900))
                .user_agent("mengren-updater")
                .https_only(true)
                .build()?;
            let mut response = client.get(&asset.url).send().await?.error_for_status()?;
            let mut file = std::fs::File::create(&partial)?;
            let mut total = 0u64;
            while let Some(chunk) = response.chunk().await? {
                total = total.saturating_add(chunk.len() as u64);
                if total > asset.size {
                    return Err(UpdateError::Integrity);
                }
                file.write_all(&chunk)?;
                if !progress(total, asset.size) {
                    return Err(UpdateError::Cancelled);
                }
            }
            file.sync_all()?;
            drop(file);
            verified(&partial, &asset)?;
            if destination.exists() {
                std::fs::remove_file(&destination)?;
            }
            std::fs::rename(&partial, &destination)?;
            Ok(destination)
        });
    if result.is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    result
}

fn validate(asset: &Asset, version: &str) -> Result<(), UpdateError> {
    let url = reqwest::Url::parse(&asset.url).map_err(|_| UpdateError::InvalidPackage)?;
    if !asset.url.starts_with(ASSET_PREFIX)
        || !url.path().starts_with(&format!(
            "/longpeng1413/mengren-shurufa/releases/download/v{version}/"
        ))
        || url.scheme() != "https"
        || url.host_str() != Some("github.com")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || asset.size == 0
        || asset.size > MAX_PACKAGE
        || asset.sha256.len() != 64
        || !asset.sha256.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(UpdateError::InvalidPackage);
    }
    Ok(())
}

fn verified(path: &Path, asset: &Asset) -> Result<(), UpdateError> {
    let mut file = std::fs::File::open(path)?;
    if file.metadata()?.len() != asset.size {
        return Err(UpdateError::Integrity);
    }
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    let digest: String = hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    if !digest.eq_ignore_ascii_case(&asset.sha256) {
        return Err(UpdateError::Integrity);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{validate, verified};
    use crate::Asset;
    #[test]
    fn rejects_foreign_urls_and_tampered_packages() {
        let mut asset = Asset {
            url: "https://github.com/longpeng1413/mengren-shurufa/releases/download/v1/pkg.exe"
                .into(),
            sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".into(),
            size: 3,
            ..Asset::default()
        };
        assert!(validate(&asset, "1").is_ok());
        let path =
            std::env::temp_dir().join(format!("mengren-update-integrity-{}", std::process::id()));
        std::fs::write(&path, b"abc").unwrap();
        assert!(verified(&path, &asset).is_ok());
        std::fs::write(&path, b"abd").unwrap();
        assert!(verified(&path, &asset).is_err());
        asset.url = "https://github.com/other/repo/releases/download/v1/pkg.exe".into();
        assert!(validate(&asset, "1").is_err());
        let _ = std::fs::remove_file(path);
    }
}
