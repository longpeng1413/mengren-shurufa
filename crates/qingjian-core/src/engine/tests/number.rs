use super::*;
use crate::CustomPhrase;

const PHONE: &str = "19813451919";

fn with_phrases(phrases: Vec<CustomPhrase>) -> Engine {
    let mut e = engine();
    e.set_custom_phrases(phrases).unwrap();
    e
}

/// 按壳的规则敲一串数字：接得住的进缓冲区，返回有没有全进去。
fn type_digits(e: &mut Engine, digits: &str) -> bool {
    for c in digits.chars() {
        if !e.number_input(c) {
            return false;
        }
        e.push(c);
    }
    true
}

#[test]
fn digits_that_start_a_number_code_stay_in_the_buffer() {
    let mut e = with_phrases(vec![CustomPhrase::new("1981", PHONE)]);
    assert!(type_digits(&mut e, "1981"));
    assert!(e.number_mode());
    let items = e.query().unwrap().candidates.items;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].text, PHONE);
    assert_eq!(items[0].kind, CandidateKind::Custom(1));
    assert_eq!(e.commit(&items[0]), PHONE);
    assert!(e.composition().is_empty());
}

#[test]
fn a_prefix_of_the_code_already_offers_the_phrase() {
    let mut e = with_phrases(vec![
        CustomPhrase::new(PHONE, PHONE),
        CustomPhrase {
            weight: 50,
            ..CustomPhrase::new("1981234", "另一个号码")
        },
        CustomPhrase::new("19", "正好是 19"),
        CustomPhrase::new("1981", PHONE),
    ]);
    assert!(type_digits(&mut e, "19"));
    let texts: Vec<_> = e
        .query()
        .unwrap()
        .candidates
        .items
        .into_iter()
        .map(|c| c.text)
        .collect();
    // 码正好是敲的在前，其余按权重；同一个号码只出一次
    assert_eq!(texts, ["正好是 19", "另一个号码", PHONE]);
}

#[test]
fn a_digit_that_matches_nothing_is_left_to_the_shell() {
    let mut e = with_phrases(vec![CustomPhrase::new("1981", PHONE)]);
    assert!(type_digits(&mut e, "19"));
    assert!(!e.number_input('5'));
    assert!(e.number_mode());
    e.push('5');
    assert_eq!(e.take_raw(), "195");
    // 没有短语以 5 开头：空缓冲区里的 5 照常交给应用
    assert!(!e.number_input('5'));
}

#[test]
fn letter_codes_and_disabled_phrases_do_not_count() {
    let e = with_phrases(vec![
        CustomPhrase {
            enabled: false,
            ..CustomPhrase::new("1981", PHONE)
        },
        CustomPhrase::new("sjh", PHONE),
    ]);
    assert!(!e.number_input('1'));
    assert!(!e.number_input('a'));
}

#[test]
fn digits_in_the_middle_of_a_number_do_not_start_a_phrase() {
    let mut e = with_phrases(vec![CustomPhrase::new("1981", PHONE)]);
    e.note_passthrough('5');
    assert!(!e.number_input('1'));
    e.note_passthrough(' ');
    assert!(e.number_input('1'));
}

#[test]
fn zhuyin_and_nine_key_keep_their_digit_keys() {
    let mut e = with_phrases(vec![CustomPhrase::new("1981", PHONE)]);
    e.set_nine_key(true);
    assert!(!e.number_input('1'));
    e.set_nine_key(false);
    e.set_zhuyin_mode(true);
    assert!(!e.number_input('1'));
}

#[test]
fn number_phrases_offer_no_pin() {
    let mut e = with_phrases(vec![CustomPhrase::new("1981", PHONE)]);
    assert!(type_digits(&mut e, "1981"));
    let item = e.query().unwrap().candidates.items.remove(0);
    assert_eq!(e.pin_target(&item), None);
}

#[test]
fn completions_replace_the_digits_before_the_cursor() {
    let e = with_phrases(vec![
        CustomPhrase::new("1981", PHONE),
        CustomPhrase::new(PHONE, PHONE),
    ]);
    assert_eq!(
        e.number_completions("电话：1981"),
        vec![(4, PHONE.to_owned())]
    );
    assert_eq!(e.number_completions("1"), vec![(1, PHONE.to_owned())]);
    // 型号、小数里的数字不补；已经是整串号码不再补；后面不是数字不补
    assert!(e.number_completions("abc1981").is_empty());
    assert!(e.number_completions("3.1981").is_empty());
    assert!(e.number_completions(PHONE).is_empty());
    assert!(e.number_completions("1981 ").is_empty());
    assert!(e.number_completions("").is_empty());
}
