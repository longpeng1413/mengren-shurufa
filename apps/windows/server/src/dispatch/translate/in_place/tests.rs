//! 原地翻译：输入法读到、读不到改用兜底取法、兜底取不到、取的时候换了窗口、DLL 没接评审、非猛人会话。

use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use qingjian_core::{
    Engine, Prediction, PredictionKind, PredictionPolicy, PredictionRequest, Predictor,
};
use qingjian_dictionary::Dictionary;
use qingjian_platform::foreground::InputFocus;
use qingjian_platform::protocol::{
    ClientMessage, PROTOCOL_VERSION, ScreenRect, ServerMessage, SessionId,
};

use super::state::Stage;
use crate::dispatch::{Router, StatusSink, StatusView, TranslatorOpen};

const SESSION: SessionId = SessionId(7);

/// 当前前台线程；0 表示不是猛人会话。全局的，测试经 [`SERIAL`] 轮流用。
static FOREGROUND: AtomicIsize = AtomicIsize::new(7);

static SERIAL: Mutex<()> = Mutex::new(());

/// 翻译请求一来就回「Hello」。
#[derive(Default)]
struct Echo(Vec<Prediction>);
impl Predictor for Echo {
    fn policy(&self) -> PredictionPolicy {
        PredictionPolicy::default()
    }
    fn submit(&mut self, request: PredictionRequest) {
        if request.kind == PredictionKind::Translate {
            self.0.push(Prediction {
                sequence: request.sequence,
                words: Vec::new(),
                sentence: Some("Hello".to_owned()),
            });
        }
    }
    fn poll(&mut self) -> Option<Prediction> {
        self.0.pop()
    }
}

#[derive(Clone, Default)]
struct Opened(Arc<Mutex<Vec<TranslatorOpen>>>);
impl StatusSink for Opened {
    fn open_translator(&self, open: TranslatorOpen) {
        self.0.lock().unwrap().push(open);
    }
    fn show_status(&self, _view: StatusView) {}
    fn hide_status(&self) {}
}

struct Fixture {
    _serial: std::sync::MutexGuard<'static, ()>,
    router: Router,
    opened: Opened,
    probes: Arc<Mutex<Vec<u64>>>,
}

fn fixture(reads_selection: bool) -> Fixture {
    let serial = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    FOREGROUND.store(7, Ordering::Relaxed);
    let mut router = Router::new(Engine::new(Dictionary::default()), Default::default());
    router
        .engine_mut()
        .set_translation_predictor(Some(Box::new(Echo::default())));
    let opened = Opened::default();
    router.set_status_sink(Box::new(opened.clone()));
    router.set_voice_foreground(|| {
        let thread = FOREGROUND.load(Ordering::Relaxed);
        (thread != 0).then_some(InputFocus {
            session: SessionId(thread as u64),
            window: 1,
        })
    });
    let probes = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&probes);
    router.set_selection_probe(Box::new(move |request| seen.lock().unwrap().push(request)));
    router.handle(ClientMessage::OpenSession {
        session: SESSION,
        app: None,
        protocol: PROTOCOL_VERSION,
    });
    sync(&mut router, reads_selection);
    Fixture {
        _serial: serial,
        router,
        opened,
        probes,
    }
}

/// 一次空闲同步，回（读选区请求号, 是否原地翻译读, 是否进评审）。
fn sync(router: &mut Router, reads_selection: bool) -> (Option<u64>, bool, bool) {
    match router.handle(ClientMessage::SyncMode {
        accepts_commit: true,
        reads_selection,
        in_place_review: reads_selection,
        session: SESSION,
    }) {
        Some(ServerMessage::ModeSync {
            read_selection,
            review_selection,
            start_review,
            ..
        }) => (read_selection, review_selection, start_review),
        other => panic!("expected ModeSync, got {other:?}"),
    }
}

fn reply(router: &mut Router, request: u64, text: &str) {
    router.handle(ClientMessage::Selection {
        session: SESSION,
        request,
        text: text.to_owned(),
        rect: ScreenRect {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        },
        unreadable: false,
    });
}

/// 评审中按回车：回给 DLL 的上屏文字。
fn accept(router: &mut Router) -> Option<String> {
    router.handle(ClientMessage::Poll { session: SESSION });
    match router.handle(ClientMessage::Key {
        session: SESSION,
        event: qingjian_platform::protocol::KeyEvent::new(0x0D, None, Default::default()),
    }) {
        Some(ServerMessage::KeyResult { commit, .. }) => commit,
        other => panic!("expected KeyResult, got {other:?}"),
    }
}

#[test]
fn selection_read_by_the_input_method_goes_straight_to_review() {
    let mut f = fixture(true);
    f.router.translate_in_place();
    let (request, review, start) = sync(&mut f.router, true);
    let request = request.expect("review read rides on ModeSync");
    assert!(review && !start);
    reply(&mut f.router, request, "你好");
    assert!(f.probes.lock().unwrap().is_empty());
    assert_eq!(accept(&mut f.router).as_deref(), Some("Hello"));
}

#[test]
fn empty_selection_falls_back_to_the_probe_and_starts_the_review() {
    let mut f = fixture(true);
    f.router.translate_in_place();
    let request = sync(&mut f.router, true).0.unwrap();
    // 微信这类程序：输入法读到的是空选区
    reply(&mut f.router, request, "");
    assert_eq!(*f.probes.lock().unwrap(), [request]);
    f.router
        .selection_probed(request, Some("  你好 ".to_owned()), None);
    // 下一次空闲同步让 DLL 进评审；DLL 来轮询后不再带
    assert_eq!(sync(&mut f.router, true), (None, false, true));
    assert_eq!(accept(&mut f.router).as_deref(), Some("Hello"));
    assert_eq!(sync(&mut f.router, true), (None, false, false));
}

#[test]
fn old_dll_probes_right_away_and_nothing_found_ends_quietly() {
    let mut f = fixture(false);
    f.router.translate_in_place();
    let request = f.probes.lock().unwrap()[0];
    f.router.selection_probed(request, None, None);
    assert!(!f.router.in_place_waiting());
    assert_eq!(sync(&mut f.router, false), (None, false, false));
    assert!(f.opened.0.lock().unwrap().is_empty());
}

#[test]
fn switching_windows_while_probing_opens_the_translator_instead() {
    let mut f = fixture(false);
    f.router.translate_in_place();
    let request = f.probes.lock().unwrap()[0];
    FOREGROUND.store(0, Ordering::Relaxed);
    f.router
        .selection_probed(request, Some("你好".to_owned()), None);
    assert_eq!(
        *f.opened.0.lock().unwrap(),
        [TranslatorOpen::Text("你好".to_owned())]
    );
}

#[test]
fn review_that_is_never_taken_is_dropped_after_the_timeout() {
    let mut f = fixture(false);
    // 能接评审、但这次读不了选区的新 DLL
    f.router.handle(ClientMessage::SyncMode {
        accepts_commit: true,
        reads_selection: false,
        in_place_review: true,
        session: SESSION,
    });
    f.router.translate_in_place();
    let request = f.probes.lock().unwrap()[0];
    f.router
        .selection_probed(request, Some("你好".to_owned()), None);
    let pending = f.router.in_place.as_mut().unwrap();
    assert_eq!(pending.stage, Stage::Start);
    pending.deadline = Instant::now();
    f.router.advance_in_place();
    assert!(f.router.translation.is_none());
    assert!(!f.router.in_place_waiting());
}

#[test]
fn foreground_without_a_mengren_session_opens_the_translator() {
    let mut f = fixture(true);
    FOREGROUND.store(0, Ordering::Relaxed);
    f.router.translate_in_place();
    assert_eq!(*f.opened.0.lock().unwrap(), [TranslatorOpen::Probe]);
}

#[test]
fn old_dll_that_cannot_review_gets_the_translation_window() {
    // 0.4.14 之前的 DLL 能读选区（翻译窗口用）但认不得原地评审：不交给它评审，改开翻译窗口
    let mut f = fixture(false);
    f.router.handle(ClientMessage::SyncMode {
        accepts_commit: true,
        reads_selection: true,
        in_place_review: false,
        session: SESSION,
    });
    f.router.translate_in_place();
    let request = f.probes.lock().unwrap()[0];
    f.router
        .selection_probed(request, Some("你好".to_owned()), None);
    assert_eq!(
        *f.opened.0.lock().unwrap(),
        [TranslatorOpen::Text("你好".to_owned())]
    );
    assert!(f.router.translation.is_none());
}

#[test]
fn start_review_repeats_until_the_dll_polls_and_is_never_given_to_a_focus_sync() {
    let mut f = fixture(false);
    f.router.handle(ClientMessage::SyncMode {
        accepts_commit: true,
        reads_selection: false,
        in_place_review: true,
        session: SESSION,
    });
    f.router.translate_in_place();
    let request = f.probes.lock().unwrap()[0];
    f.router
        .selection_probed(request, Some("你好".to_owned()), None);
    // 失焦补的同步（不声明能读选区）不交进评审
    assert_eq!(sync(&mut f.router, false), (None, false, false));
    // 空闲同步一直带，直到 DLL 来轮询
    assert_eq!(sync(&mut f.router, true), (None, false, true));
    assert_eq!(sync(&mut f.router, true), (None, false, true));
    f.router.handle(ClientMessage::Poll { session: SESSION });
    assert_eq!(sync(&mut f.router, true), (None, false, false));
    assert!(!f.router.in_place_waiting());
}
