//! 同步合并、重复、删除、本机并发编辑与坏文件保护。

use super::*;
use crate::PhraseBook;

fn directory(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("phrase-sync-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn merges_groups_codes_and_preserves_local_deletion() {
    let dir = directory("merge");
    let sync = SyncedPhrases::new(&dir);
    let coded = Phrase {
        text: "第一行\n第二行 {date}".into(),
        group: "工作".into(),
        code: "hello".into(),
        weight: 77,
        enabled: false,
    };
    let mut uncoded = coded.clone();
    uncoded.code.clear();
    uncoded.text = "点击填写".into();
    assert_eq!(sync.receive(&[coded.clone(), uncoded.clone()]).unwrap(), 2);
    assert_eq!(sync.receive(std::slice::from_ref(&coded)).unwrap(), 0);
    let mut book = PhraseBook::open(&dir);
    assert_eq!(book.phrases(), &[coded.clone(), uncoded]);
    assert_eq!(book.custom_phrases()[0].weight, 77);
    assert!(!book.custom_phrases()[0].enabled);
    book.remove(0).unwrap();
    sync.receive(std::slice::from_ref(&coded)).unwrap();
    assert_eq!(PhraseBook::open(&dir).phrases().len(), 1);
    book.add(&coded.text, &coded.group, &coded.code).unwrap();
    assert_eq!(PhraseBook::open(&dir).phrases().len(), 2);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn rejects_invalid_batch_without_partial_write_or_corrupt_overwrite() {
    let dir = directory("reject");
    let sync = SyncedPhrases::new(&dir);
    let good = Phrase::from_custom(&CustomPhrase::new("ok", "fixture"));
    let mut bad = good.clone();
    bad.code = "UPPER".into();
    assert!(sync.receive(&[good.clone(), bad]).is_err());
    assert!(!dir.join("phrase-sync.json").exists());
    std::fs::write(dir.join("phrase-sync.json"), "damaged fixture").unwrap();
    assert!(sync.receive(&[good]).is_err());
    assert_eq!(
        std::fs::read_to_string(dir.join("phrase-sync.json")).unwrap(),
        "damaged fixture"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn network_receive_does_not_overwrite_a_concurrent_local_edit() {
    let dir = directory("concurrent");
    let sync = SyncedPhrases::new(&dir);
    let mut book = PhraseBook::open(&dir);
    book.add("local", "", "aa").unwrap();
    sync.receive(&[Phrase::from_custom(&CustomPhrase::new(
        "aa",
        "remote conflict",
    ))])
    .unwrap();
    book.update(0, "edited locally", "", "aa").unwrap();
    let reopened = PhraseBook::open(&dir);
    assert_eq!(reopened.phrases().len(), 2);
    assert_eq!(reopened.phrases()[0].text, "edited locally");
    assert_eq!(reopened.phrases()[1].text, "remote conflict");
    std::fs::remove_dir_all(dir).unwrap();
}
