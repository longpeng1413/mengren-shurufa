//! 数字短语：输入码全是数字（手机号、证件号、卡号）的自定义短语，按已敲数字的前缀出候选。
//!
//! 桌面端走组句：没在打字时敲的数字是某条数字短语的开头，就留在缓冲区里出候选（[`Engine::number_input`]），
//! 空格上屏短语、回车原样上屏数字；接着敲的数字对不上任何短语时壳把整串数字原样上屏。
//! 手机上数字直接上屏，候选条按光标前那串数字给补全（[`Engine::number_completions`]），点了才换掉。

use crate::custom_phrase::{CustomPhrase, MAX_CODE_LEN, expand_variables, is_number_code};
use crate::engine::Engine;

/// 补全最多给几条：候选条一屏之内。
const MAX_COMPLETIONS: usize = 9;

impl Engine {
    /// 缓冲区是不是一段数字短语的输入（全是数字）。此时数字键是内容不是选词，壳把数字交给 [`Self::push`]
    /// 前先问 [`Self::number_input`]；注音（数字是音节键）与九宫格（数字是字母键）不参与。
    pub fn number_mode(&self) -> bool {
        let text = self.composition.text();
        !self.zhuyin && !self.nine_key && is_number_code(text)
    }

    /// 这个数字键该不该进缓冲区：敲进去之后仍是某条启用的数字短语输入码的开头。
    ///
    /// 没在组句时，上一个上屏的字是字母、数字或小数点（正在打一串数字或型号的中间）就不接，免得半路弹候选。
    /// 返回 `false` 而 [`Self::number_mode`] 为真时，壳把缓冲区连同这个数字原样上屏。
    pub fn number_input(&self, c: char) -> bool {
        if !c.is_ascii_digit() || self.zhuyin || self.nine_key {
            return false;
        }
        let text = self.composition.text();
        if text.is_empty() {
            if self
                .previous_char()
                .is_some_and(|prev| prev.is_ascii_alphanumeric() || prev == '.')
            {
                return false;
            }
        } else if !self.number_mode() {
            return false;
        }
        let mut typed = text.to_owned();
        typed.insert(self.composition.cursor().min(typed.len()), c);
        !self.number_phrases(&typed).is_empty()
    }

    /// 光标前的文字 `before` 以一串数字结尾、且是某条数字短语输入码的开头时，给出这些短语：
    /// 每条是（上屏前删掉光标前几个字符，上屏的文字）。数字前紧挨着字母或小数点（型号、小数）时不给，
    /// 已经打成短语本身的不再给。
    pub fn number_completions(&self, before: &str) -> Vec<(usize, String)> {
        if self.zhuyin {
            return Vec::new();
        }
        let digits = before.bytes().rev().take_while(u8::is_ascii_digit).count();
        if digits == 0 || digits > MAX_CODE_LEN {
            return Vec::new();
        }
        let (head, run) = before.split_at(before.len() - digits);
        if head
            .chars()
            .last()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '.')
        {
            return Vec::new();
        }
        let now = jiff::Zoned::now();
        self.number_phrases(run)
            .into_iter()
            .map(|phrase| expand_variables(&phrase.text, &now).into_owned())
            .filter(|text| text != run)
            .take(MAX_COMPLETIONS)
            .map(|text| (digits, text))
            .collect()
    }

    /// 输入码以 `typed` 开头的启用数字短语：输入码正好是 `typed` 的在前，其余按权重从高到低，
    /// 相同的按列表顺序；同一段文字只留第一条（`1981` 与整串号码都配了同一个号码时不出两遍）。
    pub(super) fn number_phrases(&self, typed: &str) -> Vec<&CustomPhrase> {
        if !is_number_code(typed) {
            return Vec::new();
        }
        let mut phrases: Vec<&CustomPhrase> = self
            .custom_phrases
            .iter()
            .filter(|p| p.enabled && is_number_code(&p.code) && p.code.starts_with(typed))
            .collect();
        phrases.sort_by_key(|p| (p.code != typed, std::cmp::Reverse(p.weight)));
        let mut seen = std::collections::HashSet::new();
        phrases.retain(|p| seen.insert(p.text.as_str()));
        phrases
    }
}
