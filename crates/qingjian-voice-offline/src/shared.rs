//! 进程里共用一个加载好的识别器：第一次说话时加载（一两秒），之后每段直接用；空闲 [`IDLE`] 后由后台线程释放，
//! 免得输入法进程一直占着两三百 MB 内存。设置里换识别方式只换 Transcriber，不会重新加载模型。

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::recognizer::Recognizer;
use crate::{OfflineConfig, OfflineError, library};

struct Loaded {
    dir: PathBuf,

    recognizer: Arc<Recognizer>,

    last_used: Instant,
}

static SHARED: Mutex<Option<Loaded>> = Mutex::new(None);

static REAPER: AtomicBool = AtomicBool::new(false);

/// 拿到识别器，没加载就加载（阻塞，在后台线程调）。加载时持着锁，同时来的第二段等它而不是再加载一份。
pub fn acquire(config: &OfflineConfig) -> Result<Arc<Recognizer>, OfflineError> {
    if !config.model.installed() {
        return Err(OfflineError::ModelMissing);
    }
    let api = library::api(config.library_dir.as_deref())?;
    let dir = config.model.dir();
    let mut shared = lock();
    if let Some(loaded) = shared.as_mut().filter(|loaded| loaded.dir == dir) {
        loaded.last_used = Instant::now();
        return Ok(Arc::clone(&loaded.recognizer));
    }
    let started = Instant::now();
    let recognizer = Arc::new(Recognizer::new(
        api,
        &config.model.model_path(),
        &config.model.tokens_path(),
        config.threads,
    )?);
    tracing::info!(
        elapsed_ms = started.elapsed().as_millis() as u64,
        threads = config.threads,
        "offline speech model loaded"
    );
    *shared = Some(Loaded {
        dir,
        recognizer: Arc::clone(&recognizer),
        last_used: Instant::now(),
    });
    drop(shared);
    spawn_reaper();
    Ok(recognizer)
}

/// 一段识别结束：从现在起重新计空闲时间。
pub fn touch() {
    if let Some(loaded) = lock().as_mut() {
        loaded.last_used = Instant::now();
    }
}

/// 立刻放掉（删除模型前调）；还有正在识别的那段等它结束时才真正释放。
pub fn release() {
    lock().take();
}

/// 现在有没有加载着（测试与诊断用）。
pub fn loaded() -> bool {
    lock().is_some()
}

fn lock() -> MutexGuard<'static, Option<Loaded>> {
    SHARED.lock().unwrap_or_else(|poison| poison.into_inner())
}

fn spawn_reaper() {
    if REAPER.swap(true, Ordering::Relaxed) {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("mengren-voice-reaper".to_owned())
        .spawn(|| {
            loop {
                std::thread::sleep(CHECK_EVERY);
                let mut shared = lock();
                let idle = shared.as_ref().is_some_and(|loaded| {
                    Arc::strong_count(&loaded.recognizer) == 1 && loaded.last_used.elapsed() >= IDLE
                });
                if idle {
                    shared.take();
                    tracing::info!("offline speech model released after idle");
                }
                if shared.is_none() {
                    REAPER.store(false, Ordering::Relaxed);
                    break;
                }
            }
        });
    if spawned.is_err() {
        REAPER.store(false, Ordering::Relaxed);
    }
}

/// 空闲这么久释放模型。
pub const IDLE: Duration = Duration::from_secs(180);

const CHECK_EVERY: Duration = Duration::from_secs(20);
