//! 版式模板：从原文拆出要翻译的正文，译完按原位置填回。

use super::part::Part;

/// 行首的列表记号，后面须跟空白才算（免得把「-5」「*注」当成列表）。
const BULLETS: &[char] = &[
    '•', '·', '▪', '◦', '‣', '●', '○', '■', '□', '◆', '★', '-', '*', '+', '>', '–', '—',
];

/// 标记 `<…>` 的最大字节数，超过就当正文里的尖括号。
const MAX_TAG: usize = 64;

/// 原文拆成的模板：保留部分与正文交替排列，换行统一成 `\n`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    parts: Vec<Part>,
}

impl Template {
    /// 拆原文：逐行留出缩进、行首列表记号与 `<tag>`，其余有字母的片段算正文。
    pub fn parse(text: &str) -> Self {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        let mut parts = Vec::new();
        for (index, line) in text.split('\n').enumerate() {
            if index > 0 {
                push_keep(&mut parts, "\n");
            }
            parse_line(line, &mut parts);
        }
        Self { parts }
    }

    /// 要翻译的正文，按出现顺序；每段都不含换行。
    pub fn texts(&self) -> Vec<&str> {
        self.parts
            .iter()
            .filter_map(|part| match part {
                Part::Text(text) => Some(text.as_str()),
                Part::Keep(_) => None,
            })
            .collect()
    }

    /// 按顺序把译文填回正文的位置；条数与正文段数不一致回 `None`。
    pub fn fill<S: AsRef<str>>(&self, translations: &[S]) -> Option<String> {
        if translations.len() != self.texts().len() {
            return None;
        }
        let mut next = translations.iter();
        let mut output = String::new();
        for part in &self.parts {
            match part {
                Part::Keep(kept) => output.push_str(kept),
                Part::Text(_) => output.push_str(next.next()?.as_ref().trim()),
            }
        }
        Some(output)
    }
}

fn parse_line(line: &str, parts: &mut Vec<Part>) {
    let body = line.trim_start();
    push_keep(parts, &line[..line.len() - body.len()]);
    let marker = list_marker(body);
    push_keep(parts, &body[..marker]);
    let mut rest = &body[marker..];
    while let Some((start, end)) = find_tag(rest) {
        push_text(parts, &rest[..start]);
        push_keep(parts, &rest[start..end]);
        rest = &rest[end..];
    }
    push_text(parts, rest);
}

/// 正文片段：首尾空白留在模板里；没有字母的（纯数字、符号）不送去翻译。
fn push_text(parts: &mut Vec<Part>, segment: &str) {
    let body = segment.trim();
    if !body.chars().any(char::is_alphabetic) {
        push_keep(parts, segment);
        return;
    }
    let lead = segment.len() - segment.trim_start().len();
    push_keep(parts, &segment[..lead]);
    parts.push(Part::Text(body.to_owned()));
    push_keep(parts, &segment[lead + body.len()..]);
}

/// 保留部分与前一段保留部分合并，模板里不出现相邻的两段 `Keep`。
fn push_keep(parts: &mut Vec<Part>, kept: &str) {
    if kept.is_empty() {
        return;
    }
    if let Some(Part::Keep(last)) = parts.last_mut() {
        last.push_str(kept);
    } else {
        parts.push(Part::Keep(kept.to_owned()));
    }
}

/// 行首列表记号（`•`、`-`、`1.`、`2)`、`3、`）连同其后空白的字节长度；不是列表行回 0。
fn list_marker(line: &str) -> usize {
    let Some(first) = line.chars().next() else {
        return 0;
    };
    let end = if BULLETS.contains(&first) {
        first.len_utf8()
    } else if first.is_ascii_digit() {
        let digits = line.bytes().take_while(u8::is_ascii_digit).count();
        match line[digits..].chars().next() {
            Some(mark @ ('.' | ')' | '、' | '．')) if digits <= 3 => digits + mark.len_utf8(),
            _ => return 0,
        }
    } else {
        return 0;
    };
    let rest = &line[end..];
    let spaces = rest.len() - rest.trim_start().len();
    // 中文的「1、」后面习惯不加空格；其余记号不跟空白就是正文（「3.14」「-5」）
    if spaces == 0 && !line[..end].ends_with('、') {
        return 0;
    }
    end + spaces
}

/// 下一个 `<tag>`、`</tag>`、`<tag attr="…">` 的字节范围；`a < b` 这类比较不算。
fn find_tag(text: &str) -> Option<(usize, usize)> {
    let mut from = 0;
    while let Some(offset) = text[from..].find('<') {
        let start = from + offset;
        let inner = &text[start + 1..];
        let name = inner.strip_prefix('/').unwrap_or(inner);
        if name.starts_with(|c: char| c.is_ascii_alphabetic())
            && let Some(close) = inner.find(['>', '<'])
            && close <= MAX_TAG
            && inner[close..].starts_with('>')
        {
            return Some((start, start + close + 2));
        }
        from = start + 1;
    }
    None
}
