//! 词典卡片表：`assets/wordcard/wordcard-en.tsv` 读进内存，按词查卡片。

use std::collections::HashMap;
use std::path::Path;

use super::card::WordCard;
use super::single_word;
use crate::GlossaryError;

/// 表里每行的列数：词、原形、音标、音标来源、考试标签、释义。
const COLUMNS: usize = 6;

/// 英文词典卡片表，见 `assets/wordcard/README.md`。
#[derive(Debug, Default)]
pub struct WordCards {
    entries: HashMap<String, WordCard>,
}

impl WordCards {
    /// 解析 TSV；任一行列数不对整表失败。
    pub fn parse(source: &str) -> Result<Self, GlossaryError> {
        let mut entries = HashMap::new();
        for (index, line) in source.lines().enumerate() {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let fields: Vec<&str> = line.split('\t').collect();
            let [word, lemma, phonetic, source, tags, definitions] = fields[..] else {
                return Err(GlossaryError::Line {
                    line: index + 1,
                    reason: format!("expected {COLUMNS} columns, got {}", fields.len()),
                });
            };
            let text = |value: &str| (!value.is_empty()).then(|| value.to_owned());
            entries.insert(
                word.to_owned(),
                WordCard {
                    word: word.to_owned(),
                    lemma: text(lemma),
                    phonetic: text(phonetic),
                    american: source == "us",
                    tags: tags.split_whitespace().map(str::to_owned).collect(),
                    definitions: definitions
                        .split("\\n")
                        .filter(|line| !line.is_empty())
                        .map(str::to_owned)
                        .collect(),
                },
            );
        }
        Ok(Self { entries })
    }

    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, GlossaryError> {
        Self::parse(&std::fs::read_to_string(path)?)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 选中的文字是单个英文词且表里有就给卡片；变形词没有自己的释义时用原形的释义与标签。
    pub fn lookup(&self, text: &str) -> Option<WordCard> {
        let card = self.find(single_word(text)?)?;
        if !card.definitions.is_empty() {
            return Some(card.clone());
        }
        let base = self.find(card.lemma.as_deref()?)?;
        Some(WordCard {
            phonetic: card.phonetic.clone().or_else(|| base.phonetic.clone()),
            american: if card.phonetic.is_some() {
                card.american
            } else {
                base.american
            },
            tags: base.tags.clone(),
            definitions: base.definitions.clone(),
            ..card.clone()
        })
    }

    /// 先按原样找，再试全小写与首字母大写（句首的 `Running`、专名 `China`）。
    fn find(&self, word: &str) -> Option<&WordCard> {
        let lower = word.to_lowercase();
        let mut chars = lower.chars();
        let capital = chars
            .next()
            .map(|first| first.to_uppercase().chain(chars).collect::<String>());
        self.entries
            .get(word)
            .or_else(|| self.entries.get(&lower))
            .or_else(|| capital.and_then(|capital| self.entries.get(&capital)))
    }
}
