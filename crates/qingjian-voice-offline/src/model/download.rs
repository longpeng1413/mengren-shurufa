//! 下载模型：逐个文件写 `<名字>.part`，再下载时带 Range 从断点续传；下完核对大小与 SHA-256 才改名，全部通过后写校验标记。
//! 阻塞到下完、出错或被取消，壳在后台线程调，进度经 [`DownloadProgress`] 轮询。

use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use sha2::{Digest, Sha256};

use super::{ModelFile, SpeechModel};
use crate::OfflineError;

/// 下载进度与取消开关，壳和下载线程共用一份。
#[derive(Debug, Default)]
pub struct DownloadProgress {
    received: AtomicU64,

    total: AtomicU64,

    cancelled: AtomicBool,
}

impl DownloadProgress {
    pub const fn new() -> Self {
        Self {
            received: AtomicU64::new(0),
            total: AtomicU64::new(0),
            cancelled: AtomicBool::new(false),
        }
    }

    pub fn received(&self) -> u64 {
        self.received.load(Ordering::Relaxed)
    }

    pub fn total(&self) -> u64 {
        self.total.load(Ordering::Relaxed)
    }

    /// 让正在进行的下载尽快停下（留着半截文件，下次续传）。
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    fn begin(&self, total: u64) {
        self.cancelled.store(false, Ordering::Relaxed);
        self.received.store(0, Ordering::Relaxed);
        self.total.store(total, Ordering::Relaxed);
    }

    fn check(&self) -> Result<(), OfflineError> {
        if self.cancelled.load(Ordering::Relaxed) {
            Err(OfflineError::Cancelled)
        } else {
            Ok(())
        }
    }
}

/// 进程里同一时间只下一份。
static RUNNING: AtomicBool = AtomicBool::new(false);

pub(super) fn running() -> bool {
    RUNNING.load(Ordering::Relaxed)
}

struct RunningGuard;

impl Drop for RunningGuard {
    fn drop(&mut self) {
        RUNNING.store(false, Ordering::Relaxed);
    }
}

impl SpeechModel {
    /// 下载缺的文件；`proxy` 为空不走代理（形如 `http://host:port`）。已装好的直接返回。
    pub fn download(&self, proxy: &str, progress: &DownloadProgress) -> Result<(), OfflineError> {
        if RUNNING.swap(true, Ordering::Relaxed) {
            return Err(OfflineError::Busy);
        }
        let _guard = RunningGuard;
        progress.begin(self.total_bytes());
        if self.installed() {
            progress
                .received
                .store(self.total_bytes(), Ordering::Relaxed);
            return Ok(());
        }
        let dir = self.dir();
        std::fs::create_dir_all(&dir)?;
        let marker = dir.join(Self::VERIFIED);
        if marker.exists() {
            std::fs::remove_file(&marker)?;
        }
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let client = client(proxy)?;
        let mut done = 0;
        for file in &self.files {
            let target = dir.join(file.name);
            if std::fs::metadata(&target).is_ok_and(|meta| meta.len() == file.size)
                && verify(&target, file).is_ok()
            {
                done += file.size;
                progress.received.store(done, Ordering::Relaxed);
                continue;
            }
            let url = format!("{}{}", self.base_url, file.name);
            let partial = dir.join(format!("{}.part", file.name));
            runtime.block_on(fetch(&client, &url, file, &partial, done, progress))?;
            if let Err(error) = verify(&partial, file) {
                let _ = std::fs::remove_file(&partial);
                return Err(error);
            }
            if target.exists() {
                std::fs::remove_file(&target)?;
            }
            std::fs::rename(&partial, &target)?;
            done += file.size;
        }
        std::fs::write(&marker, b"ok")?;
        tracing::info!(bytes = done, "speech model downloaded and verified");
        Ok(())
    }
}

async fn fetch(
    client: &reqwest::Client,
    url: &str,
    file: &ModelFile,
    partial: &Path,
    done: u64,
    progress: &DownloadProgress,
) -> Result<(), OfflineError> {
    let mut offset = std::fs::metadata(partial).map_or(0, |meta| meta.len());
    if offset > file.size {
        std::fs::remove_file(partial)?;
        offset = 0;
    }
    if offset == file.size {
        return Ok(());
    }
    let mut request = client.get(url);
    if offset > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={offset}-"));
    }
    let mut response = request.send().await?;
    match response.status().as_u16() {
        // 服务器不认 Range：从头来
        200 => offset = 0,
        206 => {}
        status => return Err(OfflineError::Http(status)),
    }
    let mut sink = OpenOptions::new()
        .create(true)
        .write(true)
        .append(offset > 0)
        .truncate(offset == 0)
        .open(partial)?;
    let mut received = offset;
    progress.received.store(done + received, Ordering::Relaxed);
    while let Some(chunk) = response.chunk().await? {
        progress.check()?;
        received += chunk.len() as u64;
        if received > file.size {
            drop(sink);
            std::fs::remove_file(partial)?;
            return Err(OfflineError::Integrity);
        }
        sink.write_all(&chunk)?;
        progress.received.store(done + received, Ordering::Relaxed);
    }
    sink.sync_all()?;
    Ok(())
}

fn verify(path: &Path, file: &ModelFile) -> Result<(), OfflineError> {
    let mut reader = std::fs::File::open(path)?;
    if reader.metadata()?.len() != file.size {
        return Err(OfflineError::Integrity);
    }
    let mut hash = Sha256::new();
    let mut buffer = vec![0u8; 1 << 16];
    loop {
        let n = reader.read(&mut buffer)?;
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
    if digest == file.sha256 {
        Ok(())
    } else {
        Err(OfflineError::Integrity)
    }
}

/// rustls + 内置根证书（安卓没有可链的系统 TLS）；GitHub 的下载会跳到 objects.githubusercontent.com。
fn client(proxy: &str) -> Result<reqwest::Client, OfflineError> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let roots = rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let tls = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|error| OfflineError::Network(error.to_string()))?
    .with_root_certificates(roots)
    .with_no_client_auth();
    let mut builder = reqwest::Client::builder()
        .tls_backend_preconfigured(tls)
        .https_only(true)
        .connect_timeout(Duration::from_secs(20))
        .read_timeout(STALL_TIMEOUT)
        .user_agent("mengren-voice-model");
    if !proxy.trim().is_empty() {
        builder = builder.proxy(reqwest::Proxy::all(proxy.trim())?);
    } else {
        builder = builder.no_proxy();
    }
    Ok(builder.build()?)
}

/// 这么久收不到数据当卡住了。
const STALL_TIMEOUT: Duration = Duration::from_secs(30);

#[cfg(test)]
mod tests {
    use super::verify;
    use crate::model::ModelFile;

    #[test]
    fn verify_checks_size_and_hash() {
        let dir = std::env::temp_dir().join(format!("mengren-voice-verify-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a");
        std::fs::write(&path, b"abc").unwrap();
        let file = ModelFile {
            name: "a",
            size: 3,
            sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        };
        assert!(verify(&path, &file).is_ok());
        std::fs::write(&path, b"abd").unwrap();
        assert!(verify(&path, &file).is_err());
        std::fs::write(&path, b"abcd").unwrap();
        assert!(verify(&path, &file).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
