//! 划词翻译的英文词典卡片：选中单个英文词时给美式音标、中文释义、考试标签与原形，不联网。

mod card;
mod table;

pub use self::card::WordCard;
pub use self::table::WordCards;

/// 单词最多多少个字母，再长就当句子走翻译。
const MAX_WORD: usize = 40;

/// 选中的文字去掉首尾空白与标点后是不是一个英文词（可带 `'`、`-` 连接，如 `don't`、`well-known`）；是就返回它。
pub fn single_word(text: &str) -> Option<&str> {
    let word = text.trim_matches(|c: char| !c.is_ascii_alphanumeric());
    let valid = !word.is_empty()
        && word.len() <= MAX_WORD
        && word.starts_with(|c: char| c.is_ascii_alphabetic())
        && word
            .split(['\'', '-', '’'])
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_alphabetic()));
    valid.then_some(word)
}

/// 短语里值得查词典的词：只看纯英文的文字（夹着汉字等就不算），按空白切开，
/// 跳过 [`FUNCTION_WORDS`] 与重复的词；实词超过 `max` 个就是句子，回 `None`。
pub fn phrase_words(text: &str, max: usize) -> Option<Vec<&str>> {
    if text.chars().any(|c| c.is_alphabetic() && !c.is_ascii()) {
        return None;
    }
    let mut words: Vec<&str> = Vec::new();
    for part in text.split_whitespace() {
        let Some(word) = single_word(part) else {
            continue;
        };
        let lower = word.to_ascii_lowercase();
        if FUNCTION_WORDS.contains(&lower.as_str())
            || words.iter().any(|seen| seen.eq_ignore_ascii_case(word))
        {
            continue;
        }
        words.push(word);
        if words.len() > max {
            return None;
        }
    }
    (!words.is_empty()).then_some(words)
}

/// 短语里不单独列词典的虚词：人人都认得，列出来只占地方。
const FUNCTION_WORDS: &[&str] = &[
    "a", "an", "the", "and", "or", "but", "of", "to", "in", "on", "at", "by", "for", "with",
    "from", "as", "is", "are", "am", "was", "were", "be", "been", "it", "its", "this", "that", "i",
    "you", "he", "she", "we", "they", "my", "your", "our", "their", "his", "her", "do", "does",
    "did", "not", "no", "so", "if",
];

#[cfg(test)]
mod tests;
