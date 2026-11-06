//! 更新安装包的后台下载任务，成功后才把校验过的路径交给平台安装器。

use crate::i18n::tr;
use bamboo_slip::Waker;
use qingjian_platform::UpdateChannel;
use qingjian_update::{Available, Target};
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

#[derive(Default)]
pub struct UpdateInstall {
    result: Arc<Mutex<Option<Result<PathBuf, String>>>>,

    percent: Arc<AtomicU64>,

    cancelled: Arc<AtomicBool>,

    running: bool,
}

impl UpdateInstall {
    pub fn running(&self) -> bool {
        self.running
    }
    pub fn percent(&self) -> u64 {
        self.percent.load(Ordering::Relaxed)
    }
    pub fn start(
        &mut self,
        offer: Available,
        channel: UpdateChannel,
        waker: Option<Waker>,
    ) -> Result<(), String> {
        if self.running {
            return Ok(());
        }
        let cache = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .ok_or(tr("common.no_data_dir"))?
            .join("MengrenShurufa/updates");
        self.running = true;
        self.cancelled.store(false, Ordering::Relaxed);
        self.percent.store(0, Ordering::Relaxed);
        let result = Arc::clone(&self.result);
        let percent = Arc::clone(&self.percent);
        let cancelled = Arc::clone(&self.cancelled);
        std::thread::spawn(move || {
            let outcome = qingjian_update::download_release(
                env!("CARGO_PKG_VERSION"),
                Target::current(),
                channel == UpdateChannel::Beta,
                &offer.version,
                &cache,
                "",
                |done, total| {
                    let next = done.saturating_mul(100) / total.max(1);
                    if percent.swap(next, Ordering::Relaxed) != next
                        && let Some(waker) = &waker
                    {
                        waker.wake();
                    }
                    !cancelled.load(Ordering::Relaxed)
                },
            )
            .map_err(|error| {
                match error {
                    qingjian_update::UpdateError::Cancelled => tr("update.cancelled"),
                    qingjian_update::UpdateError::Integrity => tr("update.integrity_failed"),
                    qingjian_update::UpdateError::InvalidPackage => tr("update.release_changed"),
                    qingjian_update::UpdateError::UntrustedSignature
                    | qingjian_update::UpdateError::MalformedSignature => tr("update.untrusted"),
                    _ => tr("update.download_failed"),
                }
                .to_owned()
            });
            *result.lock().unwrap_or_else(|poison| poison.into_inner()) = Some(outcome);
            if let Some(waker) = waker {
                waker.wake();
            }
        });
        Ok(())
    }
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
    pub fn finish(&mut self) -> Option<Result<PathBuf, String>> {
        let result = self.result.lock().ok()?.take()?;
        self.running = false;
        Some(if self.cancelled.load(Ordering::Relaxed) {
            Err(tr("update.cancelled").to_owned())
        } else {
            result
        })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn cancel_after_download_before_wake_does_not_launch_installer() {
        let mut job = super::UpdateInstall::default();
        *job.result.lock().unwrap() = Some(Ok(std::path::PathBuf::from("test-setup.exe")));
        job.cancel();
        assert!(job.finish().unwrap().is_err());
    }
}
