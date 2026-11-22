//! 取词：划词卡片的弹出与按钮、翻译窗口顶部的「取词…」菜单（划词翻译、弹出卡片时自动朗读外文两个开关）。
//! 开关写进配置文件 `[lookup]`，Server 热加载后按新设置显示「译」图标。
//! 自动朗读读外文那一边：英文单词与外文句子读原文，中文等译文回来后读译文。

use qingjian_platform::{Config, HoverMode, dirs, windows_text::tr};

use super::card::{CLOSE, COPY, LOOP, OPEN, SPEAK_RESULT, SPEAK_SOURCE};
use super::speech::Side;
use super::{LOOKUP, STATUS, Window};

/// 词卡发音用的语言（美式英语，与卡片上的美式音标一致）。
const WORD_VOICE: &str = "en";

/// 「取词…」菜单里悬停三项的起始下标与对应方式。
const HOVER_ITEM: usize = 3;
const HOVERS: [HoverMode; 3] = [HoverMode::Off, HoverMode::Ctrl, HoverMode::Always];

impl Window {
    /// Server 请求在 `(x, y)` 弹卡片；`hover` 是悬停取词的卡片（不抢焦点，鼠标走开就收）。
    pub(super) fn show_card(&mut self, x: i32, y: i32, text: String, hover: bool) {
        self.speech.refresh();
        let label = self.loop_label();
        self.card.set_loop_label(&label);
        let word = self.card.show(x, y, text, self.model.language, hover);
        tracing::info!(
            word = word.is_some(),
            empty = self.card.text.is_empty(),
            auto_speak = self.lookup.auto_speak,
            "lookup card shown"
        );
        if !self.lookup.auto_speak || self.card.text.is_empty() {
            return;
        }
        let (source, _) = self.card.model.voices();
        match word {
            Some(word) => self.auto_read(Side::Source, &word, WORD_VOICE),
            None if source != "zh" => {
                let text = self.card.text.clone();
                self.auto_read(Side::Source, &text, source);
            }
            // 原文是中文：外文在译文里，等译文到了再读
            None => self.card.read_when_translated = true,
        }
    }

    /// 卡片的译文到了：原文是中文且开着自动朗读，就读译文。
    pub(super) fn card_translated(&mut self) {
        if self.card.read_when_translated && !self.card.model.output.is_empty() {
            self.card.read_when_translated = false;
            let (_, language) = self.card.model.voices();
            let text = self.card.model.output.clone();
            self.auto_read(Side::Result, &text, &language);
        }
    }

    fn auto_read(&mut self, side: Side, text: &str, language: &str) {
        if self.speech.readable(language) {
            let spoken = self.speech.read(side, text, language);
            tracing::info!(ok = spoken.is_ok(), language, "lookup card auto read");
        }
    }

    /// 卡片上的按钮。
    pub(super) fn card_command(&mut self, param: usize) {
        // 只认按钮的点击（BN_CLICKED 为 0），文本框的通知不管
        if param >> 16 != 0 {
            return;
        }
        self.card.engage();
        match param & 0xffff {
            CLOSE => self.card.hide(),
            SPEAK_SOURCE => {
                let (text, language) = match &self.card.word {
                    Some(word) => (word.word.clone(), WORD_VOICE),
                    None => (self.card.text.clone(), self.card.model.voices().0),
                };
                if self.speech.read(Side::Source, &text, language).is_err() {
                    tracing::warn!("card speech failed");
                }
            }
            SPEAK_RESULT => {
                let (_, language) = self.card.model.voices();
                let text = self.card.model.output.clone();
                if self.speech.read(Side::Result, &text, &language).is_err() {
                    tracing::warn!("card speech failed");
                }
            }
            COPY => self.card.copy(),
            LOOP => {
                let (owner, anchor) = (self.card.hwnd, self.card.loop_anchor());
                self.loop_menu(owner, anchor);
            }
            OPEN => {
                let text = self.card.text.clone();
                self.card.hide();
                self.receive(text);
            }
            _ => {}
        }
    }

    /// 「取词…」菜单：划词翻译、自动朗读两个开关（选中就反过来），分隔线下是悬停取词三选一。
    pub(super) fn lookup_menu(&mut self) {
        let labels = [
            tr("lookup_selection"),
            tr("lookup_auto_speak"),
            "",
            tr("lookup_hover_off"),
            tr("lookup_hover_ctrl"),
            tr("lookup_hover_always"),
        ];
        let mut checked: Vec<usize> = [self.lookup.selection, self.lookup.auto_speak]
            .iter()
            .enumerate()
            .filter_map(|(index, on)| on.then_some(index))
            .collect();
        checked.push(
            HOVER_ITEM
                + HOVERS
                    .iter()
                    .position(|m| *m == self.lookup.hover)
                    .unwrap_or(0),
        );
        match self.menu(LOOKUP, &labels, &checked) {
            Some(0) => self.set_lookup("selection", !self.lookup.selection),
            Some(1) => self.set_lookup("auto_speak", !self.lookup.auto_speak),
            Some(index) if index >= HOVER_ITEM => {
                if let Some(&mode) = HOVERS.get(index - HOVER_ITEM) {
                    self.set_hover(mode);
                }
            }
            _ => {}
        }
    }

    fn set_hover(&mut self, mode: HoverMode) {
        let saved = dirs::config_path()
            .ok_or_else(|| "no config directory".to_owned())
            .and_then(|path| {
                Config::set_value(&path, "lookup", "hover", mode.key())
                    .map_err(|error| error.to_string())
            });
        if let Err(error) = saved {
            tracing::warn!(%error, "hover lookup setting could not be saved");
            self.set(STATUS, tr("lookup_save_failed"));
            return;
        }
        self.lookup.hover = mode;
        if mode != HoverMode::Off {
            self.card.preload();
        }
    }

    fn set_lookup(&mut self, key: &str, value: bool) {
        let saved = dirs::config_path()
            .ok_or_else(|| "no config directory".to_owned())
            .and_then(|path| {
                Config::set_bool(&path, "lookup", key, value).map_err(|error| error.to_string())
            });
        if let Err(error) = saved {
            tracing::warn!(%error, key, "lookup setting could not be saved");
            self.set(STATUS, tr("lookup_save_failed"));
            return;
        }
        match key {
            "selection" => {
                self.lookup.selection = value;
                if value {
                    self.card.preload();
                }
            }
            _ => self.lookup.auto_speak = value,
        }
    }
}
