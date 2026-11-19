//! 模拟识别完成，走真实 Router 请求和回包。
use super::{Capture, Router};
use qingjian_core::{Engine, Transcriber, TranscriptEvent};
use qingjian_dictionary::Dictionary;
use qingjian_platform::foreground::InputFocus;
use qingjian_platform::protocol::{ClientMessage, PROTOCOL_VERSION, ServerMessage, SessionId};
use std::sync::{Arc, mpsc};
struct Final(Option<TranscriptEvent>);
impl Transcriber for Final {
    fn available(&self) -> bool {
        true
    }
    fn start(&mut self, _: u32) {}
    fn feed(&mut self, _: &[i16]) {}
    fn finish(&mut self) {
        self.0 = Some(TranscriptEvent::Final("语音文字".into()));
    }
    fn cancel(&mut self) {
        self.0 = None;
    }
    fn poll(&mut self) -> Option<TranscriptEvent> {
        self.0.take()
    }
}
fn router() -> Router {
    let mut r = Router::new(Engine::new(Dictionary::default()), Default::default());
    r.voice.transcriber = Box::new(Final(None));
    r.set_voice_capture(Some(Arc::new(|| {
        Ok(Capture {
            pcm: mpsc::channel().1,
            level: None,
            stop: Arc::default(),
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
