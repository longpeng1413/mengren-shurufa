//! 模拟识别完成，走真实 Router 请求和回包。
use super::{Capture, Router};
use qingjian_core::{Engine, Transcriber, TranscriptEvent};
use qingjian_dictionary::Dictionary;
use qingjian_platform::foreground::InputFocus;
use qingjian_platform::protocol::{ClientMessage, PROTOCOL_VERSION, ServerMessage, SessionId};
use std::sync::{Arc, mpsc};
#[derive(Default)]
struct Final {
    event: Option<TranscriptEvent>,

    samples: Vec<i16>,
}
impl Transcriber for Final {
    fn available(&self) -> bool {
        true
    }
    fn start(&mut self, _: u32) {}
    fn feed(&mut self, samples: &[i16]) {
        self.samples.extend_from_slice(samples);
    }
    fn finish(&mut self) {
        let text = if self.samples.is_empty() {
            "语音文字".into()
        } else {
            format!("{:?}", self.samples)
        };
        self.event = Some(TranscriptEvent::Final(text));
    }
    fn cancel(&mut self) {
        self.event = None;
        self.samples.clear();
    }
    fn poll(&mut self) -> Option<TranscriptEvent> {
        self.event.take()
    }
}
fn router() -> Router {
    let mut r = Router::new(Engine::new(Dictionary::default()), Default::default());
    r.voice.transcriber = Box::<Final>::default();
    r.set_voice_capture(Some(Arc::new(|| {
        Ok(Capture {
            pcm: mpsc::channel().1,
            level: None,
            stop: Arc::default(),
            failure: None,
        })
    })));
    r
}

#[test]
fn final_is_delivered_once_with_legacy_poll_fallback() {
    let mut r = router();
    let session = SessionId(1);
    r.handle(ClientMessage::OpenSession {
        session,
        app: None,
        protocol: PROTOCOL_VERSION,
    });
    let sync = |r: &mut Router, accepts_commit| match r.handle(ClientMessage::SyncMode {
        session,
        accepts_commit,
        reads_selection: false,
        in_place_review: false,
    }) {
        Some(ServerMessage::ModeSync { commit, .. }) => commit,
        _ => panic!("mode sync"),
    };
    for modern in [true, false] {
        assert_eq!(sync(&mut r, modern), None);
        r.toggle_status_voice();
        assert!(r.recording());
        r.toggle_status_voice();
        r.advance_voice();
        let text = Some("语音文字".to_string());
        if modern {
            assert_eq!(sync(&mut r, true), text);
        } else {
            assert_eq!(sync(&mut r, false), None);
            assert!(
                matches!(r.handle(ClientMessage::Poll{session}),Some(ServerMessage::Update{commit,..}) if commit==text)
            );
        }
        assert_eq!(sync(&mut r, true), None);
    }
    r.handle_hotkey_voice(true);
    r.cancel_hotkey_voice();
    r.handle_hotkey_voice(false);
    r.advance_voice();
    assert_eq!(sync(&mut r, true), None);
}

#[test]
fn voice_uses_native_focus_and_drops_result_on_window_change() {
    use std::sync::atomic::{AtomicIsize, Ordering};
    static WINDOW: AtomicIsize = AtomicIsize::new(10);
    let mut r = router();
    let foreground = SessionId(1);
    let stale = SessionId(2);
    for session in [foreground, stale] {
        r.handle(ClientMessage::OpenSession {
            session,
            app: None,
            protocol: PROTOCOL_VERSION,
        });
    }
    r.set_voice_foreground(|| {
        Some(InputFocus {
            session: SessionId(1),
            window: WINDOW.load(Ordering::Relaxed),
        })
    });
    let sync = |r: &mut Router, session| match r.handle(ClientMessage::SyncMode {
        session,
        accepts_commit: true,
        reads_selection: false,
        in_place_review: false,
    }) {
        Some(ServerMessage::ModeSync { commit, .. }) => commit,
        _ => panic!("mode sync"),
    };
    sync(&mut r, stale);
    r.handle_hotkey_voice(true);
    r.handle_hotkey_voice(false);
    r.advance_voice();
    assert_eq!(sync(&mut r, stale), None);
    assert_eq!(sync(&mut r, foreground).as_deref(), Some("语音文字"));

    r.handle_hotkey_voice(true);
    r.handle_hotkey_voice(false);
    r.advance_voice();
    WINDOW.store(11, Ordering::Relaxed);
    assert_eq!(sync(&mut r, foreground), None);
    WINDOW.store(10, Ordering::Relaxed);
    assert_eq!(sync(&mut r, foreground), None);
}

#[test]
fn unclaimed_final_retries_modifiers_once_and_never_follows_focus() {
    use super::{PendingCommit, VoiceInsertResult};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    let mut router = router();
    let focus = InputFocus {
        session: SessionId(9),
        window: 70,
    };
    router.set_voice_foreground(|| {
        Some(InputFocus {
            session: SessionId(9),
            window: 70,
        })
    });
    router.set_voice_insertion(|target, text| {
        assert_eq!(target.window, 70);
        assert_eq!(text, "测试");
        if CALLS.fetch_add(1, Ordering::SeqCst) == 0 {
            VoiceInsertResult::Busy
        } else {
            VoiceInsertResult::Sent
        }
    });
    let now = Instant::now();
    let due = now + Duration::from_millis(520);
    let pending = || PendingCommit {
        session: focus.session,
        focus: Some(focus),
        fallback_at: due,
        expires: due + Duration::from_secs(4),
        text: "测试".into(),
    };
    router.voice.pending = Some(pending());
    router.deliver_voice_fallback(now);
    assert_eq!(CALLS.load(Ordering::SeqCst), 0);
    router.deliver_voice_fallback(due);
    router.deliver_voice_fallback(due);
    router.deliver_voice_fallback(due);
    assert!(router.take_async_commit(focus.session).is_none());
    router.voice.pending = Some(pending());
    assert_eq!(
        router.take_async_commit(focus.session).as_deref(),
        Some("测试")
    );
    router.deliver_voice_fallback(due);
    router.voice.pending = Some(pending());
    router.set_voice_foreground(|| {
        Some(InputFocus {
            session: SessionId(9),
            window: 71,
        })
    });
    router.deliver_voice_fallback(due);
    assert!(router.voice.pending.is_none());
    assert_eq!(CALLS.load(Ordering::SeqCst), 2);
}

#[test]
fn releasing_the_key_waits_for_the_last_capture_packet() {
    use std::sync::Mutex;

    let mut router = router();
    let session = SessionId(1);
    router.handle(ClientMessage::OpenSession {
        session,
        app: None,
        protocol: PROTOCOL_VERSION,
    });
    router.handle(ClientMessage::SyncMode {
        session,
        accepts_commit: true,
        reads_selection: false,
        in_place_review: false,
    });
    let (sender, receiver) = mpsc::channel();
    let receiver = Mutex::new(Some(receiver));
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let capture_stop = Arc::clone(&stop);
    router.set_voice_capture(Some(Arc::new(move || {
        Ok(Capture {
            pcm: receiver.lock().unwrap().take().unwrap(),
            level: None,
            stop: Arc::clone(&capture_stop),
            failure: None,
        })
    })));

    router.toggle_status_voice();
    sender.send(vec![11]).unwrap();
    router.advance_voice();
    router.toggle_status_voice();
    assert!(stop.load(std::sync::atomic::Ordering::Relaxed));
    router.advance_voice();
    assert!(router.take_async_commit(session).is_none());

    sender.send(vec![-23]).unwrap();
    drop(sender);
    router.advance_voice();
    assert_eq!(
        router.take_async_commit(session).as_deref(),
        Some("[11, -23]")
    );
    router.advance_voice();
    assert!(router.take_async_commit(session).is_none());
}

/// 记下面板收到的提示与收起。
#[derive(Clone, Default)]
struct Notes(Arc<std::sync::Mutex<Vec<String>>>);
impl super::VoiceSink for Notes {
    fn listening(&self, _: &str, _: f32, _: &str) {}
    fn recognizing(&self, _: &str, _: &str) {}
    fn error(&self, message: &str) {
        self.0.lock().unwrap().push(message.to_owned());
    }
    fn hide(&self) {}
}
impl Notes {
    fn last(&self) -> Option<String> {
        self.0.lock().unwrap().last().cloned()
    }
}

fn open(router: &mut Router) -> Notes {
    router.handle(ClientMessage::OpenSession {
        session: SessionId(1),
        app: None,
        protocol: PROTOCOL_VERSION,
    });
    router.handle(ClientMessage::SyncMode {
        session: SessionId(1),
        accepts_commit: true,
        reads_selection: false,
        in_place_review: false,
    });
    let notes = Notes::default();
    router.set_voice_sink(Box::new(notes.clone()));
    notes
}

#[test]
fn capture_problems_reach_the_panel_and_end_the_segment() {
    use qingjian_platform::windows_text::tr;
    use std::sync::Mutex;

    // 打不开：工厂给的原因原样显示，不进入录音
    let mut router = router();
    let notes = open(&mut router);
    router.set_voice_capture(Some(Arc::new(|| Err("麦克风被系统拒绝".to_owned()))));
    router.toggle_status_voice();
    assert!(!router.recording());
    assert_eq!(notes.last().as_deref(), Some("麦克风被系统拒绝"));

    // 录到一半报错：结束这一段并显示原因
    let (fail_tx, fail_rx) = mpsc::channel();
    let slot = Mutex::new(Some(fail_rx));
    let (_pcm_tx, pcm_rx) = mpsc::channel();
    let pcm = Mutex::new(Some(pcm_rx));
    router.set_voice_capture(Some(Arc::new(move || {
        Ok(Capture {
            pcm: pcm.lock().unwrap().take().unwrap(),
            level: None,
            stop: Arc::default(),
            failure: slot.lock().unwrap().take(),
        })
    })));
    router.toggle_status_voice();
    assert!(router.recording());
    fail_tx.send(Err("录音中断".to_owned())).unwrap();
    router.advance_voice();
    assert!(router.voice.active.is_none());
    assert_eq!(notes.last().as_deref(), Some("录音中断"));

    // 线程没留原因就退了（通道断开）：按中断处理
    let (fail_tx, fail_rx) = mpsc::channel::<Result<(), String>>();
    let slot = Mutex::new(Some(fail_rx));
    router.set_voice_capture(Some(Arc::new(move || {
        Ok(Capture {
            pcm: mpsc::channel().1,
            level: None,
            stop: Arc::default(),
            failure: slot.lock().unwrap().take(),
        })
    })));
    router.voice.notice_until = None;
    router.toggle_status_voice();
    drop(fail_tx);
    router.advance_voice();
    assert!(router.voice.active.is_none());
    assert_eq!(notes.last().as_deref(), Some(tr("mic_lost")));
}

#[test]
fn empty_results_explain_silence_or_ask_to_repeat() {
    use qingjian_platform::windows_text::tr;
    use std::sync::Mutex;

    struct Empty(Option<TranscriptEvent>);
    impl Transcriber for Empty {
        fn available(&self) -> bool {
            true
        }
        fn start(&mut self, _: u32) {}
        fn feed(&mut self, _: &[i16]) {}
        fn finish(&mut self) {
            self.0 = Some(TranscriptEvent::Final("  ".into()));
        }
        fn cancel(&mut self) {}
        fn poll(&mut self) -> Option<TranscriptEvent> {
            self.0.take()
        }
    }
    for (level, expected) in [(0.0, "mic_silent"), (0.2, "voice_empty")] {
        let mut router = router();
        router.voice.transcriber = Box::new(Empty(None));
        let notes = open(&mut router);
        let (level_tx, level_rx) = mpsc::channel();
        let slot = Mutex::new(Some(level_rx));
        router.set_voice_capture(Some(Arc::new(move || {
            Ok(Capture {
                pcm: mpsc::channel().1,
                level: slot.lock().unwrap().take(),
                stop: Arc::default(),
                failure: None,
            })
        })));
        router.toggle_status_voice();
        level_tx.send(level).unwrap();
        router.advance_voice();
        router.toggle_status_voice();
        router.advance_voice();
        router.advance_voice();
        assert!(router.voice.active.is_none());
        assert_eq!(notes.last().as_deref(), Some(tr(expected)));
        assert!(router.take_async_commit(SessionId(1)).is_none());
    }
}

#[test]
fn noise_like_results_are_not_typed() {
    use super::meaningful;
    for noise in ["ㄱ.", "ㄱ.ㄱ.", "。", "…", " ? ", "ᄀ"] {
        assert!(!meaningful(noise), "{noise}");
    }
    for text in ["你好", "ok", "3", "こんにちは", "안녕", "Ünïcode", "１"] {
        assert!(meaningful(text), "{text}");
    }
}

#[test]
fn text_from_dead_silence_is_discarded_with_a_mic_hint() {
    use qingjian_platform::windows_text::tr;
    use std::sync::Mutex;

    for (level, typed) in [(0.00003, false), (0.2, true)] {
        let mut router = router();
        let notes = open(&mut router);
        let (level_tx, level_rx) = mpsc::channel();
        let slot = Mutex::new(Some(level_rx));
        router.set_voice_capture(Some(Arc::new(move || {
            Ok(Capture {
                pcm: mpsc::channel().1,
                level: slot.lock().unwrap().take(),
                stop: Arc::default(),
                failure: None,
            })
        })));
        router.toggle_status_voice();
        level_tx.send(level).unwrap();
        router.advance_voice();
        router.toggle_status_voice();
        router.advance_voice();
        router.advance_voice();
        let delivered = router.take_async_commit(SessionId(1));
        assert_eq!(delivered.is_some(), typed, "level {level}");
        if !typed {
            assert_eq!(notes.last().as_deref(), Some(tr("mic_silent")));
        }
    }
}
