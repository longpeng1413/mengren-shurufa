//! 输入防抖、翻译方向和异步响应的代次。
use qingjian_core::{PredictionKind, PredictionRequest, Predictor};
use qingjian_platform::{Config, windows_text::tr};
use qingjian_predict::CloudPredictor;
use std::time::{Duration, Instant};

pub const LANGS: [(&str, &str, &str); 8] = [
    ("en", "English", "409"),
    ("ja", "日本語", "411"),
    ("ko", "한국어", "412"),
    ("fr", "Français", "40C"),
    ("de", "Deutsch", "407"),
    ("es", "Español", "C0A"),
    ("ru", "Русский", "419"),
    ("vi", "Tiếng Việt", "42A"),
];
pub fn target(text: &str, language: &str) -> String {
    let mut han = 0;
    let mut other = 0;
    for c in text.chars() {
        match c as u32 {
            0x3040..=0x30ff | 0xff66..=0xff9f => return "zh".into(),
            0x3400..=0x9fff | 0xf900..=0xfaff | 0x20000..=0x323af => han += 1,
            _ if c.is_alphabetic() => other += 1,
            _ => {}
        }
    }
    if han > other {
        language.into()
    } else {
        "zh".into()
    }
}
pub struct Translation {
    pub source: String,
    pub language: usize,
    pub output: String,
    pub status: &'static str,
    sequence: u64,
    pending: Option<Instant>,
    worker: Result<CloudPredictor, &'static str>,
    done: &'static str,
}
impl Translation {
    pub fn new(config: Config) -> Self {
        let path = qingjian_platform::dirs::user_dir().map(|dir| dir.join("translator-language"));
        let saved = path.and_then(|p| std::fs::read_to_string(p).ok());
        let lang = saved
            .as_deref()
            .unwrap_or(&config.general.learning_language);
        let language = LANGS.iter().position(|l| l.0 == lang).unwrap_or(0);
        let mut cfg = config.predict;
        let done = if cfg.youdao.enabled { "youdao" } else { "ai" };
        cfg.debounce_ms = 0;
        cfg.min_interval_ms = 0;
        let worker = if cfg.youdao.enabled || cfg.enabled {
            CloudPredictor::new(&cfg).map_err(|_| "failed")
        } else {
            Err("disabled")
        };
        Self {
            source: String::new(),
            language,
            output: String::new(),
            status: "",
            sequence: 0,
            pending: None,
            worker,
            done,
        }
    }
    pub fn change(&mut self, text: String, language: usize, now: Instant) {
        if self.source == text && self.language == language {
            return;
        }
        if self.language != language
            && let Some(dir) = qingjian_platform::dirs::user_dir()
        {
            let _ = std::fs::write(dir.join("translator-language"), LANGS[language].0);
        }
        self.source = text;
        self.language = language;
        self.sequence += 1;
        self.output.clear();
        self.status = "";
        if let Ok(worker) = &mut self.worker {
            worker.cancel();
        }
        self.pending = (!self.source.trim().is_empty()).then_some(now + Duration::from_millis(800));
    }
    pub fn send(&mut self) {
        if self.pending.take().is_none() {
            return;
        }
        let worker = match &mut self.worker {
            Ok(w) => w,
            Err(key) => {
                self.status = tr(key);
                return;
            }
        };
        self.status = tr("busy");
        worker.submit(PredictionRequest {
            sequence: self.sequence,
            kind: PredictionKind::Translate,
            manual: true,
            text: self.source.clone(),
            target_language: target(&self.source, LANGS[self.language].0),
            before: String::new(),
            after: String::new(),
            pinyin: String::new(),
            letters: String::new(),
            syllables: 0,
            candidates: vec![],
            guess: String::new(),
            max_items: 1,
            want_sentence: true,
        });
    }
    pub fn tick(&mut self, now: Instant) -> bool {
        let mut changed = false;
        if self.pending.is_some_and(|due| now >= due) {
            self.send();
            changed = true;
        }
        if let Ok(worker) = &mut self.worker {
            while let Some(result) = worker.poll() {
                if result.sequence == self.sequence {
                    self.output = result.sentence.unwrap_or_default();
                    self.status = tr(if self.output.is_empty() {
                        "failed"
                    } else {
                        self.done
                    });
                    changed = true;
                }
            }
        }
        changed
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scripts() {
        for text in ["hello 世界", "お茶", "русский", "한국어"] {
            assert_eq!(target(text, "de"), "zh");
        }
        assert_eq!(target("你好！", "vi"), "vi");
    }
}
