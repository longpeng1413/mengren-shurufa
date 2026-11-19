//! 语音设置页的后台活：豆包「测试连接」与离线「测试识别」都阻塞，放进自己的线程；
//! 离线模型下载带进度，同样在后台线程跑，进度从 [`DownloadProgress`] 读，叫醒窗口重画。
//! 结果放在共享槽里，`App::wake` 取走说成一句话。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use bamboo_slip::Waker;
use qingjian_platform::VoiceConfig;
use qingjian_voice::VolcengineConfig;
use qingjian_voice::volcengine;
use qingjian_voice_offline::{DownloadProgress, OfflineConfig, OfflineError, SpeechModel};

use crate::i18n::{tr, tr_args};

/// 一次测试的结果：`Ok` 是成功那句话，`Err` 是给用户看的失败原因。
type TestOutcome = Result<String, String>;

/// 豆包 / 离线测试连接共用的状态槽。
#[derive(Default)]
pub struct VoiceTest {
    result: Arc<Mutex<Option<TestOutcome>>>,
    running: bool,
}

impl VoiceTest {
    pub fn running(&self) -> bool {
        self.running
    }

    /// 豆包测试连接：用配置里的资源 ID 与密钥，识别半秒静音。
    pub fn start_volcengine(
        &mut self,
        config: &VoiceConfig,
        waker: Option<Waker>,
    ) -> Result<(), String> {
        let Some(key) = config.resolve_api_key() else {
            return Err(tr("voice.no_key").to_owned());
        };
        let volc = VolcengineConfig::new(key, &config.resource_id);
        self.launch(waker, move || {
            volcengine::test_connection(&volc)
                .map(|elapsed| tr_args("voice.test_ok", &[("ms", &elapsed.as_millis())]))
        });
        Ok(())
    }

    /// 离线测试识别：加载模型并识别半秒静音；需要引擎库随程序安装。
    pub fn start_offline(
        &mut self,
        model_root: &std::path::Path,
        waker: Option<Waker>,
    ) -> Result<(), String> {
        let model = SpeechModel::new(model_root);
        if !model.installed() {
            return Err(tr("voice.offline_missing").to_owned());
        }
        let offline = OfflineConfig::new(model, None);
        self.launch(waker, move || {
            qingjian_voice_offline::test_recognizer(&offline)
                .map(|elapsed| tr_args("voice.test_ok", &[("ms", &elapsed.as_millis())]))
                .map_err(|error| describe_offline(&error))
        });
        Ok(())
    }

    /// 起线程跑一次测试；结果写进共享槽并叫醒窗口。
    fn launch<F>(&mut self, waker: Option<Waker>, job: F)
    where
        F: FnOnce() -> TestOutcome + Send + 'static,
    {
        self.running = true;
        let result = Arc::clone(&self.result);
        let _ = std::thread::Builder::new()
            .name("qingjian-voice-test".to_owned())
            .spawn(move || {
                let outcome = job();
                if let Ok(mut slot) = result.lock() {
                    *slot = Some(outcome);
                }
                if let Some(waker) = waker {
                    waker.wake();
                }
            });
    }

    /// 结果到了就取走。
    pub fn finish(&mut self) -> Option<TestOutcome> {
        let outcome = self.result.lock().ok()?.take()?;
        self.running = false;
        Some(outcome)
    }
}

/// 把离线错误说成人话；`OfflineError::describe` 已带中文文案，直接用。
pub fn describe_offline(error: &OfflineError) -> String {
    error.describe()
}

/// 离线模型下载走到哪了。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stage {
    Downloading { done: u64, total: u64 },
    Done,
    Failed(String),
    Cancelled,
}

struct DownloadShared {
    progress: DownloadProgress,
    stage: Mutex<Stage>,
    done: AtomicBool,
    waker: Option<Waker>,
}

/// 离线模型下载后台任务。
#[derive(Default)]
pub struct VoiceDownload {
    current: Option<Arc<DownloadShared>>,
}

impl VoiceDownload {
    /// 正在下、或刚下完 / 刚失败的那一份进度。
    pub fn stage(&self) -> Option<Stage> {
        let shared = self.current.as_ref()?;
        Some(shared.stage.lock().ok()?.clone())
    }

    pub fn busy(&self) -> bool {
        matches!(self.stage(), Some(Stage::Downloading { .. }))
    }

    /// 开始下载到 `model_root`；`proxy` 可空。
    pub fn start(
        &mut self,
        model_root: &std::path::Path,
        proxy: &str,
        waker: Option<Waker>,
    ) -> Result<(), String> {
        if SpeechModel::downloading() {
            return Err(tr("voice.download_busy").to_owned());
        }
        let model = SpeechModel::new(model_root);
        let shared = Arc::new(DownloadShared {
            progress: DownloadProgress::new(),
            stage: Mutex::new(Stage::Downloading { done: 0, total: 0 }),
            done: AtomicBool::new(false),
            waker,
        });
        let worker = Arc::clone(&shared);
        let proxy = proxy.to_owned();
        let _ = std::thread::Builder::new()
            .name("qingjian-voice-download".to_owned())
            .spawn(move || {
                let outcome = model.download(&proxy, &worker.progress);
                let stage = match outcome {
                    Ok(()) => Stage::Done,
                    Err(OfflineError::Cancelled) => Stage::Cancelled,
                    Err(error) => Stage::Failed(describe_offline(&error)),
                };
                if let Ok(mut slot) = worker.stage.lock() {
                    *slot = stage;
                }
                worker.done.store(true, Ordering::Relaxed);
                if let Some(waker) = &worker.waker {
                    waker.wake();
                }
            })
            .map_err(|error| tr_args("voice.download_no_thread", &[("error", &error)]))?;
        self.current = Some(shared);
        Ok(())
    }

    /// 取消正在进行的下载（留半截，下次续传）。
    pub fn cancel(&self) {
        if let Some(shared) = &self.current {
            shared.progress.cancel();
        }
    }

    /// 下载中每报一次进度都叫醒窗口：由 `App::wake` 周期性调用。
    pub fn tick(&mut self) -> bool {
        let Some(shared) = &self.current else {
            return false;
        };
        if shared.done.load(Ordering::Relaxed) {
            return false;
        }
        if shared.progress.total() > 0 {
            if let Ok(mut slot) = shared.stage.lock() {
                *slot = Stage::Downloading {
                    done: shared.progress.received(),
                    total: shared.progress.total(),
                };
            }
            if let Some(waker) = &shared.waker {
                waker.wake();
            }
            return true;
        }
        false
    }

    /// 收掉结束的任务：`Done` 返回 true（界面切到「已安装」），其余清掉。
    pub fn finished(&mut self) -> bool {
        let Some(shared) = &self.current else {
            return false;
        };
        if !shared.done.load(Ordering::Relaxed) {
            return false;
        }
        let done = matches!(self.stage(), Some(Stage::Done));
        self.current = None;
        done
    }

    /// 下载进度供界面算百分比：返回 `(done, total)`。
    pub fn progress(&self) -> (u64, u64) {
        let Some(shared) = &self.current else {
            return (0, 0);
        };
        (shared.progress.received(), shared.progress.total())
    }
}

/// 删除离线模型：先放掉进程里加载着的识别器，再删目录。
pub fn remove_model(model_root: &std::path::Path) -> Result<(), String> {
    qingjian_voice_offline::release_shared();
    SpeechModel::new(model_root)
        .delete()
        .map_err(|error| error.to_string())
}

/// 模型占用字节数。
pub fn model_size(model_root: &std::path::Path) -> u64 {
    SpeechModel::new(model_root).size()
}

/// 模型是否装好。
pub fn model_installed(model_root: &std::path::Path) -> bool {
    SpeechModel::new(model_root).installed()
}

/// 要下载的总字节数。
pub fn model_total_bytes(model_root: &std::path::Path) -> u64 {
    SpeechModel::new(model_root).total_bytes()
}
