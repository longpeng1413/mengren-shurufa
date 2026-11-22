use super::{WordCards, phrase_words, single_word};

const TABLE: &str = "# 注释\n\
China\t\tˈtʃaɪnə\tus\t\tn. 中国\n\
ran\trun\træn\tus\t\t\n\
run\t\trʌn\tus\tzk gk\tn. 跑, 赛跑\\nvi. 跑\n\
running\trun\tˈrʌnɪŋ\tus\tgk\tn. 赛跑\n\
vt\t\t\t\t\t\n";

#[test]
fn picks_one_english_word_out_of_the_selection() {
    assert_eq!(single_word("  running, "), Some("running"));
    assert_eq!(single_word("“don't”"), Some("don't"));
    assert_eq!(single_word("well-known."), Some("well-known"));
    assert_eq!(single_word("two words"), None);
    assert_eq!(single_word("苹果"), None);
    assert_eq!(single_word("2026"), None);
    assert_eq!(single_word("a--b"), None);
}

#[test]
fn own_entry_keeps_its_definitions_and_points_to_the_lemma() {
    let cards = WordCards::parse(TABLE).unwrap();
    let card = cards.lookup("Running").unwrap();
    assert_eq!(card.word, "running");
    assert_eq!(card.lemma.as_deref(), Some("run"));
    assert_eq!(card.phonetic.as_deref(), Some("ˈrʌnɪŋ"));
    assert!(card.american);
    assert_eq!(card.definitions, ["n. 赛跑"]);
}

#[test]
fn a_form_without_definitions_borrows_the_lemma() {
    let cards = WordCards::parse(TABLE).unwrap();
    let card = cards.lookup("ran").unwrap();
    assert_eq!(card.lemma.as_deref(), Some("run"));
    assert_eq!(card.phonetic.as_deref(), Some("ræn"));
    assert_eq!(card.tags, ["zk", "gk"]);
    assert_eq!(card.definitions, ["n. 跑, 赛跑", "vi. 跑"]);
}

#[test]
fn case_falls_back_and_unknown_words_are_none() {
    let cards = WordCards::parse(TABLE).unwrap();
    assert_eq!(cards.lookup("china").unwrap().word, "China");
    assert_eq!(cards.lookup("RUN").unwrap().word, "run");
    assert!(cards.lookup("vt").is_none(), "no definitions and no lemma");
    assert!(cards.lookup("zebra").is_none());
    assert!(cards.lookup("run fast").is_none());
}

#[test]
fn a_short_row_fails_the_whole_table() {
    assert!(WordCards::parse("run\t\trʌn\n").is_err());
}

#[test]
fn bundled_table_parses() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/wordcard/wordcard-en.tsv"
    );
    let cards = WordCards::from_path(path).unwrap();
    assert!(cards.len() > 50_000);
    let card = cards.lookup("presentation").unwrap();
    assert_eq!(card.phonetic.as_deref(), Some("ˌprezənˈteɪʃən"));
    assert!(card.american);
}

#[test]
fn phrases_keep_content_words_and_skip_long_sentences() {
    assert_eq!(
        phrase_words("Be clear and specific.", 6),
        Some(vec!["clear", "specific"])
    );
    assert_eq!(
        phrase_words("Good morning", 6),
        Some(vec!["Good", "morning"])
    );
    assert_eq!(phrase_words("the THE the", 6), None, "only function words");
    assert_eq!(phrase_words("run, Run, RUN", 6), Some(vec!["run"]));
    assert_eq!(phrase_words("等待 waiting", 6), None, "mixed with Chinese");
    assert_eq!(
        phrase_words("one two three four five six seven", 6),
        None,
        "a sentence"
    );
}
