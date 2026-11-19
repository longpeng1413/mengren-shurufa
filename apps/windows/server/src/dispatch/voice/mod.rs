//! 语音识别会话（Windows Server）：持有识别方、录音流与进行态，挂在下发消息 / tick 节拍上。
//!
//! 分工与安卓一致（docs/design/voice-and-swipe.md）：录音线程把单声道 16 位 PCM 放进通道，
//! 这里在 Router 线程上 `feed` 给识别方、定时 `poll` 事件；中间结果在自绘语音面板上显示，
//! 最终结果经 `Engine::commit_voice` 处理，再由发起会话取回。
//!
//! 录音源用 [`CaptureFactory`] 注入，保持本模块平台无关：Windows 的 `main` 注入 WASAPI 工厂，
//! 测试注入假工厂。识别方由 [`transcriber::build`] 按 `[voice]` 配置装配。

mod transcriber;

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use qingjian_core::{NoTranscriber, Transcriber, TranscriptEvent};
use qingjian_platform::protocol::SessionId;
use qingjian_platform::{VoiceConfig, dirs, foreground::InputFocus};

use super::Router;
use qingjian_platform::{KeyCombo, windows_text::tr};
type ConfigureKeys = fn(&VoiceConfig, Option<KeyCombo>);
type InsertText = fn(InputFocus, &str) -> VoiceInsertResult;

#[derive(Debug, PartialEq, Eq)]
pub enum VoiceInsertResult {
    Sent,
    Busy,
    Failed,
}

struct PendingCommit {
    session: SessionId,
    focus: Option<InputFocus>,
    fallback_at: Instant,
    expires: Instant,
    text: String,
}

/// 一段录音的采样率；豆包与 SenseVoice 都按 16 kHz 单声道。
pub const SAMPLE_RATE: u32 = 16_000;

/// 正常采集在下一次轮询就收尾。若驱动卡住，限制等尾包的时间。
const CAPTURE_DRAIN_TIMEOUT: Duration = Duration::from_millis(150);

/// 一次录音会话里，录音线程 -> Router 的东西。
pub struct Capture {
    /// 单声道 16 位 PCM 块。
    pub pcm: Receiver<Vec<i16>>,
    /// 音量电平（0..1），画话筒波形；没有就忽略。
    pub level: Option<Receiver<f32>>,
    /// 置 true 让录音线程停下；线程自行收尾并关闭发送端。
    pub stop: Arc<AtomicBool>,
}

/// 录音源工厂：开一段新录音，返回通道。注入的闭包在 Router 线程调用，不能阻塞太久。
pub type CaptureFactory = Arc<dyn Fn() -> Result<Capture, String> + Send + Sync>;

/// 语音面板输出端：中间文字 / 音量 / 出错提示，以及收起。窗口在 UI 线程自绘。
pub trait VoiceSink: Send {
    /// 正在听：显示中间结果（可以是空串）与音量。
    fn listening(&self, text: &str, level: f32, hint: &str);
    /// 松手后还在等最终结果：面板停留，文字不变。
    fn recognizing(&self, text: &str, hint: &str);
    /// 出错提示。
    fn error(&self, message: &str);
    /// 收起面板。
    fn hide(&self);
}

/// 没有面板时（测试 / UI 起不来）的空实现。
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopVoiceSink;
impl VoiceSink for NoopVoiceSink {
    fn listening(&self, _text: &str, _level: f32, _hint: &str) {}
    fn recognizing(&self, _text: &str, _hint: &str) {}
    fn error(&self, _message: &str) {}
    fn hide(&self) {}
}

/// 进行中的一段语音。
struct Active {
    /// 哪个会话发起的；失焦 / 换会话时作废。
    session: SessionId,
    focus: Option<InputFocus>,
    /// 录音流；`stop` 置位后录音线程会收尾。
    capture: Capture,
    /// 最近一次中间结果（整段，不是增量）。
    partial: String,
    /// 说完后还在等最终结果的起点，用于 UI 状态切换。
    finished_at: Option<Instant>,

    finish_sent: bool,
}

/// 语音运行状态：识别方常驻，一段录音的进行态在其下。
pub struct VoiceRuntime {
    transcriber: Box<dyn Transcriber>,
    /// 当前装配的是不是本机离线：私密输入的判断用（云端在私密框里不发声）。
    offline: bool,
    capture_factory: Option<CaptureFactory>,
    sink: Box<dyn VoiceSink>,
    active: Option<Active>,
    foreground: Option<fn() -> Option<InputFocus>>,
    pending: Option<PendingCommit>,
    insert_text: Option<InsertText>,
    /// 最近一次给面板的音量（平滑用）。
    last_level: f32,
    notice_until: Option<Instant>,
    from_status: bool,
    configure_keys: ConfigureKeys,
    listening_changed: fn(bool),
}

impl Default for VoiceRuntime {
    fn default() -> Self {
        Self {
            transcriber: Box::new(NoTranscriber),
            offline: false,
            capture_factory: None,
            sink: Box::new(NoopVoiceSink),
            active: None,
            foreground: None,
            pending: None,
            insert_text: None,
            last_level: 0.0,
            notice_until: None,
            from_status: false,
            configure_keys: |_, _| {},
            listening_changed: |_| {},
        }
    }
}

impl Router {
    pub fn set_voice_callbacks(&mut self, keys: ConfigureKeys, listening: fn(bool)) {
        self.voice.configure_keys = keys;
        self.voice.listening_changed = listening;
    }
    pub fn set_voice_foreground(&mut self, current: fn() -> Option<InputFocus>) {
        self.voice.foreground = Some(current);
    }

    pub fn set_voice_insertion(&mut self, insert: InsertText) {
        self.voice.insert_text = Some(insert);
    }

    pub(super) fn reload_voice(&mut self, next: &super::RouterConfig) {
        let old = &self.config.voice;
        let new = &next.voice;
        if (
            old.trigger(),
            old.shortcut,
            old.hold_ms,
            self.config.translator,
        ) != (new.trigger(), new.shortcut, new.hold_ms, next.translator)
        {
            self.cancel_hotkey_voice();
            (self.voice.configure_keys)(new, next.translator);
        }
        if old_provider_changed(&self.config.voice, new) {
            self.configure_voice(new);
        }
    }
    pub(super) fn recording(&self) -> bool {
        self.voice
            .active
            .as_ref()
            .is_some_and(|a| a.finished_at.is_none())
    }
    pub fn open_translator(&self) {
        self.status.open_translator();
    }
    pub fn cancel_hotkey_voice(&mut self) {
        if let Some(a) = &self.voice.active {
            self.handle_voice_cancel(a.session);
        }
    }
    pub(super) fn toggle_status_voice(&mut self) {
        if self.recording() {
            self.handle_hotkey_voice(false);
        } else if self.voice.active.is_none() {
            self.start_from_focus(true);
        }
    }
    fn start_from_focus(&mut self, from_status: bool) {
        if self.voice.active.is_some() {
            return;
        }
        self.voice.from_status = from_status;
        if !self.voice_available() {
            self.voice.notice(tr("off"));
            return;
        }
        let session = match self.voice.foreground {
            Some(current) => current().map(|focus| focus.session),
            None => self.mode_session.or(self.focused),
        };
        if let Some(session) = session.filter(|id| self.sessions.contains_key(id)) {
            self.handle_voice_start(session);
        } else {
            self.voice.notice(tr("voice_focus"));
        }
    }

    /// 按配置重建识别方；启动时与配置热加载（换了服务 / 密钥）都走这里。
    pub fn configure_voice(&mut self, config: &VoiceConfig) {
        let user_dir = dirs::user_dir();
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(Path::to_path_buf));
        self.voice.cancel_active("配置已更新");
        self.voice.offline = config.is_offline();
        self.voice.transcriber =
            transcriber::build(config, user_dir.as_deref(), exe_dir.as_deref());
    }

    /// 注入录音源工厂。Windows 的 `main` 传 WASAPI 工厂；`None` 表示没有录音（入口提示不可用）。
    pub fn set_voice_capture(&mut self, factory: Option<CaptureFactory>) {
        self.voice.capture_factory = factory;
    }

    /// 注入语音面板输出端。
    pub fn set_voice_sink(&mut self, sink: Box<dyn VoiceSink>) {
        self.voice.sink = sink;
    }

    /// 语音入口能不能用：识别方就绪 + 有录音源。DLL 下发的 `InputSettings.voice` 也据此决定。
    pub fn voice_available(&self) -> bool {
        self.voice.transcriber.available() && self.voice.capture_factory.is_some()
    }

    /// 开始一段语音：开录音、起识别方。已经在听或入口不可用时什么都不做。
    pub(in crate::dispatch) fn handle_voice_start(&mut self, session: SessionId) {
        if self.voice.active.is_some() || !self.voice_available() {
            return;
        }
        let focus = self.voice.foreground.and_then(|current| current());
        if self.voice.foreground.is_some() && focus.is_none_or(|target| target.session != session) {
            return;
        }
        self.voice.pending = None;
        let factory = match &self.voice.capture_factory {
            Some(factory) => Arc::clone(factory),
            None => return,
        };
        let private = self.sessions.get(&session).is_some_and(|info| info.private);
        // 私密输入不把声音送去云端：离线可用，豆包则直接拒绝（提示走面板）
        if private && !self.voice.transcriber_is_offline() {
            self.voice.notice(tr("private"));
            return;
        }
        let capture = match factory() {
            Ok(capture) => capture,
            Err(error) => {
                tracing::warn!(%error, "语音录音启动失败");
                self.voice.notice(tr("mic"));
                return;
            }
        };
        self.ensure_focus(session);
        self.end_translation();
        self.voice.transcriber.start(SAMPLE_RATE);
        self.voice.active = Some(Active {
            session,
            focus,
            capture,
            partial: String::new(),
            finished_at: None,
            finish_sent: false,
        });
        self.voice.last_level = 0.0;
        self.voice.notice_until = None;
        (self.voice.listening_changed)(true);
        self.voice.sink.listening("", 0.0, self.voice.hint());
        self.reconcile_status();
        tracing::debug!(?session, private, "语音开始");
    }

    /// 全局热键触发（长按 Win / 自定义键）：用当前聚焦会话开始 / 结束一段语音。
    /// 没有聚焦会话时开始无效（没有地方上屏）；结束则按正在录音的那段收尾。
    pub fn handle_hotkey_voice(&mut self, down: bool) {
        if down {
            self.start_from_focus(false);
        } else if let Some(session) = self.voice.active.as_ref().map(|a| a.session) {
            self.handle_voice_stop(session);
        }
    }

    /// 说完（按键松开）：停录音、收尾识别方，等最终结果。
    pub(in crate::dispatch) fn handle_voice_stop(&mut self, session: SessionId) {
        if self
            .voice
            .active
            .as_ref()
            .is_none_or(|a| a.session != session || a.finished_at.is_some())
        {
            return;
        }
        if let Some(active) = &mut self.voice.active {
            active.capture.stop.store(true, Ordering::Relaxed);
            active.finished_at = Some(Instant::now());
        }
        self.reconcile_status();
        let partial = self
            .voice
            .active
            .as_ref()
            .map(|a| a.partial.clone())
            .unwrap_or_default();
        self.voice.sink.recognizing(&partial, self.voice.hint());
        tracing::debug!(?session, "语音说完，等最终结果");
    }

    /// 取消：丢掉这段、撤掉中间结果、收起面板。
    pub(in crate::dispatch) fn handle_voice_cancel(&mut self, session: SessionId) {
        if self
            .voice
            .active
            .as_ref()
            .is_some_and(|a| a.session == session)
        {
            self.voice.cancel_active("取消");
            self.reconcile_status();
        }
    }

    /// 会话关闭 / 失焦时兜底收掉语音，避免录音线程悬着。
    pub(in crate::dispatch) fn voice_abort_session(&mut self, session: SessionId) {
        if self
            .voice
            .pending
            .as_ref()
            .is_some_and(|pending| pending.session == session)
        {
            self.voice.pending = None;
        }
        if self
            .voice
            .active
            .as_ref()
            .is_some_and(|a| a.session == session)
        {
            self.voice.cancel_active("会话结束");
            self.reconcile_status();
        }
    }

    /// 有没有一段语音在进行中（`next_tick` 据此缩短节拍）。
    pub(in crate::dispatch) fn voice_active(&self) -> bool {
        self.voice.active.is_some()
            || self.voice.notice_until.is_some()
            || self.voice.pending.is_some()
    }

    /// 语音和鼠标点选分别排队，避免在切窗口时把听写写入其他编辑框。
    pub(super) fn take_async_commit(&mut self, session: SessionId) -> Option<String> {
        let clicked = self.take_clicked(session);
        if let Some(pending) = &self.voice.pending {
            if Instant::now() >= pending.expires || !self.voice.focus_matches(pending.focus) {
                self.voice.pending = None;
            } else if pending.session == session {
                let voice = self.voice.pending.take()?.text;
                tracing::debug!(?session, "voice final delivered to input thread");
                return Some(clicked.unwrap_or_default() + &voice);
            }
        }
        clicked
    }

    fn deliver_voice_fallback(&mut self, now: Instant) {
        let Some(pending) = self.voice.pending.take() else {
            return;
        };
        if !self.voice.focus_matches(pending.focus) {
            return;
        }
        if now >= pending.expires {
            self.voice.notice(tr("voice_insert_failed"));
            return;
        }
        if let (Some(insert), Some(focus)) = (self.voice.insert_text, pending.focus)
            && now >= pending.fallback_at
        {
            match insert(focus, &pending.text) {
                VoiceInsertResult::Sent => {
                    tracing::debug!(session=?pending.session, "voice final sent as Unicode input");
                    return;
                }
                VoiceInsertResult::Failed => {
                    self.voice.notice(tr("voice_insert_failed"));
                    return;
                }
                VoiceInsertResult::Busy => {}
            }
        }
        self.voice.pending = Some(pending);
    }

    /// tick 里驱动一次语音（rescore 的 tick 调它）。
    pub(in crate::dispatch) fn voice_advance(&mut self) {
        self.advance_voice();
    }

    /// tick 节拍里驱动：喂录音、取事件、给面板、上屏。返回是否这一拍动了语音状态
    /// （用来缩短下个 tick 间隔——识别结果是异步来的，要尽快取）。
    pub(in crate::dispatch) fn advance_voice(&mut self) -> bool {
        if self.voice.notice_until.is_some_and(|d| Instant::now() >= d) {
            self.voice.notice_until = None;
            self.voice.sink.hide();
        }
        self.deliver_voice_fallback(Instant::now());
        if self
            .voice
            .active
            .as_ref()
            .is_some_and(|active| !self.voice.focus_matches(active.focus))
        {
            self.voice.cancel_active("焦点改变");
            self.reconcile_status();
            return true;
        }
        let hint = self.voice.hint();
        let Some(active) = self.voice.active.as_mut() else {
            return false;
        };
        // 先看音量（可能没有通道）
        let mut level = self.voice.last_level;
        if let Some(receiver) = &active.capture.level {
            while let Ok(value) = receiver.try_recv() {
                level = value.clamp(0.0, 1.0);
            }
        }
        self.voice.last_level = level * 0.8 + self.voice.last_level * 0.2;

        // 收尾信号发给录音线程后，它仍可能交出最后一包。先喂完再结束识别。
        if !active.finish_sent {
            let disconnected = loop {
                match active.capture.pcm.try_recv() {
                    Ok(chunk) => self.voice.transcriber.feed(&chunk),
                    Err(TryRecvError::Disconnected) => break true,
                    Err(TryRecvError::Empty) => break false,
                }
            };
            if active
                .finished_at
                .is_some_and(|at| disconnected || at.elapsed() >= CAPTURE_DRAIN_TIMEOUT)
            {
                active.finish_sent = true;
                self.voice.transcriber.finish();
            }
        }

        // 取事件（一次 tick 取完）
        let mut events = Vec::new();
        while let Some(event) = self.voice.transcriber.poll() {
            events.push(event);
        }
        if events.is_empty() {
            if active.finished_at.is_none() {
                self.voice
                    .sink
                    .listening(&active.partial, self.voice.last_level, hint);
            }
            return false;
        }

        let session = active.session;
        let focus = active.focus;
        let mut done = false;
        for event in events {
            match event {
                TranscriptEvent::Partial(text) => {
                    if let Some(active) = self.voice.active.as_mut() {
                        active.partial = text;
                    }
                }
                TranscriptEvent::Final(text) => {
                    let text = text.trim().to_owned();
                    if !text.is_empty() {
                        let commit = self.engine.commit_voice(&text);
                        if self.voice.focus_matches(focus) {
                            tracing::debug!(
                                ?session,
                                chars = commit.chars().count(),
                                "voice final ready"
                            );
                            let now = Instant::now();
                            self.voice.pending = Some(PendingCommit {
                                session,
                                focus,
                                fallback_at: now + Duration::from_millis(520),
                                expires: now + Duration::from_secs(5),
                                text: commit,
                            });
                        }
                    }
                    done = true;
                }
                TranscriptEvent::Error(message) => {
                    self.voice.notice(&message);
                    done = true;
                }
            }
        }
        if done {
            if let Some(active) = self.voice.active.take() {
                active.capture.stop.store(true, Ordering::Relaxed);
            }
            if self.voice.notice_until.is_none() {
                self.voice.sink.hide();
            }
            (self.voice.listening_changed)(false);
            self.reconcile_status();
        } else if let Some(active) = self.voice.active.as_ref() {
            // 中间结果更新面板
            if active.finished_at.is_some() {
                self.voice.sink.recognizing(&active.partial, hint);
            } else {
                self.voice
                    .sink
                    .listening(&active.partial, self.voice.last_level, hint);
            }
        }
        true
    }
}

fn old_provider_changed(old: &VoiceConfig, new: &VoiceConfig) -> bool {
    old.provider != new.provider
        || old.model_root != new.model_root
        || old.resource_id != new.resource_id
        || old.api_key != new.api_key
        || old.api_key_env != new.api_key_env
}
impl VoiceRuntime {
    fn focus_matches(&self, focus: Option<InputFocus>) -> bool {
        self.foreground
            .is_none_or(|current| focus.is_some() && current() == focus)
    }

    fn hint(&self) -> &'static str {
        tr(if self.from_status { "click" } else { "hold" })
    }
    fn notice(&mut self, message: &str) {
        self.sink.error(message);
        self.notice_until = Some(Instant::now() + std::time::Duration::from_millis(2500));
    }

    /// 当前是否离线识别：私密输入的判断用。
    fn transcriber_is_offline(&self) -> bool {
        // 装配时按配置记一份比探测 transcriber 类型可靠；这里用 capture 之外的标记。
        self.offline
    }

    /// 收掉进行中的一段：停录音、取消识别、清状态、收面板。
    fn cancel_active(&mut self, _reason: &str) {
        if let Some(active) = self.active.take() {
            active.capture.stop.store(true, Ordering::Relaxed);
        }
        (self.listening_changed)(false);
        self.notice_until = None;
        self.transcriber.cancel();
        self.last_level = 0.0;
        self.sink.hide();
    }
}

#[cfg(test)]
mod tests;
