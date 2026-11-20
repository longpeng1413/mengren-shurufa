//! 翻译窗口快捷键带入选中文字：旧 DLL、新 DLL 回选区、超时、私密输入。

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use qingjian_core::Engine;
use qingjian_dictionary::Dictionary;
use qingjian_platform::foreground::InputFocus;
use qingjian_platform::protocol::{
    ClientMessage, PROTOCOL_VERSION, ScreenRect, ServerMessage, SessionId,
};

use crate::dispatch::{Router, StatusSink, StatusView, TranslatorOpen};

const SESSION: SessionId = SessionId(5);

/// 记下 Router 要求打开翻译窗口的每一次（带不带文字）。
#[derive(Clone, Default)]
struct Opened(Arc<Mutex<Vec<TranslatorOpen>>>);
impl StatusSink for Opened {
    fn open_translator(&self, open: TranslatorOpen) {
        self.0.lock().unwrap().push(open);
    }
    fn show_status(&self, _view: StatusView) {}
    fn hide_status(&self) {}
}
impl Opened {
    fn take(&self) -> Vec<TranslatorOpen> {
        std::mem::take(&mut self.0.lock().unwrap())
    }
}

fn router(reads_selection: bool) -> (Router, Opened) {
    let mut router = Router::new(Engine::new(Dictionary::default()), Default::default());
    let opened = Opened::default();
    router.set_status_sink(Box::new(opened.clone()));
    router.set_voice_foreground(|| {
        Some(InputFocus {
            session: SESSION,
            window: 1,
        })
    });
    router.handle(ClientMessage::OpenSession {
        session: SESSION,
        app: None,
        protocol: PROTOCOL_VERSION,
    });
    sync(&mut router, reads_selection);
    (router, opened)
}

fn sync(router: &mut Router, reads_selection: bool) -> Option<u64> {
    match router.handle(ClientMessage::SyncMode {
        accepts_commit: true,
        reads_selection,
        in_place_review: reads_selection,
        session: SESSION,
    }) {
        Some(ServerMessage::ModeSync { read_selection, .. }) => read_selection,
        _ => panic!("mode sync"),
    }
}

fn reply(router: &mut Router, request: u64, text: &str, unreadable: bool) {
    router.handle(ClientMessage::Selection {
        session: SESSION,
        request,
        text: text.to_owned(),
        unreadable,
        rect: ScreenRect {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        },
    });
}

#[test]
fn old_dll_lets_the_ui_probe_right_away() {
    let (mut router, opened) = router(false);
    router.open_translator();
    assert_eq!(opened.take(), [TranslatorOpen::Probe]);
    assert_eq!(sync(&mut router, false), None);
}

#[test]
fn selection_from_the_next_sync_goes_into_the_window() {
    let (mut router, opened) = router(true);
    router.open_translator();
    assert!(opened.take().is_empty(), "waits for the selection first");
    // 再按一次不重复发请求
    router.open_translator();
    let request = sync(&mut router, true).expect("read request rides on ModeSync");
    assert_eq!(sync(&mut router, true), None, "sent only once");
    // 别的请求号不算
    reply(&mut router, request + 1, "别的", false);
    assert!(opened.take().is_empty());
    reply(&mut router, request, "  Hello world \n", false);
    assert_eq!(
        opened.take(),
        [TranslatorOpen::Text("Hello world".to_owned())]
    );
    // 输入框说没选中（微信等经兼容层的程序总这么说）：同样交给界面层去取
    router.open_translator();
    let request = sync(&mut router, true).unwrap();
    reply(&mut router, request, "", false);
    assert_eq!(opened.take(), [TranslatorOpen::Probe]);
    // 文档不让读（网页正文）：界面层辅助接口加模拟复制
    router.open_translator();
    let request = sync(&mut router, true).unwrap();
    reply(&mut router, request, "", true);
    assert_eq!(opened.take(), [TranslatorOpen::Probe]);
}

#[test]
fn no_reply_in_time_opens_an_empty_window() {
    let (mut router, opened) = router(true);
    router.open_translator();
    sync(&mut router, true).unwrap();
    assert!(router.next_tick() <= Duration::from_millis(50));
    router.advance_translator_window();
    assert!(opened.take().is_empty());
    router.translator_window.pending.as_mut().unwrap().deadline = Instant::now();
    router.advance_translator_window();
    assert_eq!(opened.take(), [TranslatorOpen::Probe]);
}

#[test]
fn private_fields_and_closed_sessions_are_not_read() {
    let (mut private, opened) = router(true);
    private.handle(ClientMessage::Privacy {
        session: SESSION,
        private: true,
    });
    private.open_translator();
    assert_eq!(
        opened.take(),
        [TranslatorOpen::Plain],
        "private fields are never read"
    );

    let (mut closed, opened) = router(true);
    closed.handle(ClientMessage::CloseSession { session: SESSION });
    closed.open_translator();
    assert_eq!(opened.take(), [TranslatorOpen::Probe]);
}
