//! 数字短语：敲号码开头几位出短语，空格上屏短语、回车原样上屏，对不上的号码照常打出来。
use super::tests::{compose, key};
use crate::dispatch::{Router, RouterConfig};
use qingjian_core::{CustomPhrase, Engine};
use qingjian_dictionary::Dictionary;
use qingjian_platform::protocol::{
    ClientMessage, KeyModifiers, KeyOutcome, PROTOCOL_VERSION, SessionId,
};

const PHONE: &str = "19813451919";

fn router() -> Router {
    let mut engine = Engine::new(Dictionary::parse("你\tni\t100\n").unwrap());
    engine
        .set_custom_phrases(vec![CustomPhrase::new("1981", PHONE)])
        .unwrap();
    let mut router = Router::new(engine, RouterConfig::default());
    router.handle(ClientMessage::OpenSession {
        session: SessionId(1),
        app: None,
        protocol: PROTOCOL_VERSION,
    });
    router
}

fn preedit(router: &Router) -> String {
    router
        .current_frame()
        .preedit
        .iter()
        .map(|segment| segment.text.as_str())
        .collect()
}

#[test]
fn space_commits_the_phrase_and_enter_the_digits() {
    let normal = KeyModifiers::default();
    let mut router = router();
    let (outcome, commit, frame) = key(&mut router, 0x31, Some('1'), normal);
    assert_eq!((outcome, commit), (KeyOutcome::Consumed, None));
    assert_eq!(frame.candidates.items[0].text, PHONE);
    compose(&mut router, "981", normal);
    assert_eq!(preedit(&router), "1981");
    let (_, commit, frame) = key(&mut router, 0x20, Some(' '), normal);
    assert_eq!(commit.as_deref(), Some(PHONE));
    assert!(frame.is_empty());

    // 前一个字是号码本身（不是数字）时又能接着用；回车上屏敲的数字
    router.engine_mut().note_passthrough(' ');
    compose(&mut router, "19", normal);
    let (_, commit, _) = key(&mut router, 0x0D, None, normal);
    assert_eq!(commit.as_deref(), Some("19"));
}

#[test]
fn other_numbers_come_out_as_typed() {
    let normal = KeyModifiers::default();
    let mut router = router();
    compose(&mut router, "19", normal);
    // 对不上任何短语：缓冲区连同这一位原样上屏
    let (_, commit, frame) = key(&mut router, 0x35, Some('5'), normal);
    assert_eq!(commit.as_deref(), Some("195"));
    assert!(frame.is_empty());
    // 紧跟在数字后面的 1 不是号码的开头，交给应用
    let (outcome, commit, _) = key(&mut router, 0x31, Some('1'), normal);
    assert_eq!((outcome, commit), (KeyOutcome::Passthrough, None));
    // 没有短语以 5 开头
    router.engine_mut().note_passthrough(' ');
    let (outcome, _, _) = key(&mut router, 0x35, Some('5'), normal);
    assert_eq!(outcome, KeyOutcome::Passthrough);
}

#[test]
fn letters_after_the_digits_flush_them_first() {
    let normal = KeyModifiers::default();
    let mut router = router();
    compose(&mut router, "19", normal);
    let (_, commit, _) = key(&mut router, 0x4E, Some('n'), normal);
    assert_eq!(commit.as_deref(), Some("19"));
    assert_eq!(preedit(&router), "n");
}
