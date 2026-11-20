//! SAPI 异步朗读、语言声音选择与朗读倍速。
//!
//! 只认经典 SAPI 声音（中文系统通常只有中文 Huihui、英文 Zira）。本机有哪些语言的声音在打开窗口时查一次，
//! 没有声音的语言不朗读（用默认中文声音硬读出来是错的），由窗口底部的小字说明。
use super::translation::LANGS;
use qingjian_platform::dirs;
use windows::{
    Win32::{
        Media::Speech::{
            ISpObjectTokenCategory, ISpVoice, SPCAT_VOICES, SPRS_IS_SPEAKING, SPVOICESTATUS,
            SpObjectTokenCategory, SpVoice,
        },
        System::Com::{CLSCTX_ALL, CoCreateInstance},
    },
    core::{HSTRING, PCWSTR, Result},
};

/// 朗读倍速档位及对应的 SAPI 语速：SAPI 语速 10 约为 3 倍、-10 约为 1/3，按对数换算取整。
pub const SPEEDS: [(&str, i32); 6] = [
    ("0.5×", -6),
    ("0.75×", -3),
    ("1.0×", 0),
    ("1.25×", 2),
    ("1.5×", 4),
    ("2.0×", 6),
];

/// 默认档位（1.0×）。
const NORMAL: usize = 2;

pub struct Speech {
    voice: Option<ISpVoice>,

    /// 正在朗读的按钮，再点同一个按钮就停。
    last: Option<usize>,

    /// 本机有声音的语言代码（`zh`、`en`…）。
    voices: Vec<&'static str>,

    /// 当前倍速在 [`SPEEDS`] 里的下标。
    pub speed: usize,
}

impl Speech {
    /// 查本机声音、读上次选的倍速。调用前线程已初始化 COM。
    pub fn new() -> Self {
        let voices = std::iter::once("zh")
            .chain(LANGS.iter().map(|l| l.0))
            .filter(|lang| installed(code(lang)))
            .collect();
        let speed = speed_file()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| SPEEDS.iter().position(|s| s.0 == text.trim()))
            .unwrap_or(NORMAL);
        Self {
            voice: None,
            last: None,
            voices,
            speed,
        }
    }

    /// 这种语言本机能不能朗读。
    pub fn readable(&self, lang: &str) -> bool {
        self.voices.contains(&lang)
    }

    /// 本机能朗读的语言代码。
    pub fn voices(&self) -> &[&'static str] {
        &self.voices
    }

    /// 换倍速并记住；正在读的这段也跟着变。
    pub fn set_speed(&mut self, index: usize) {
        let Some((label, rate)) = SPEEDS.get(index) else {
            return;
        };
        self.speed = index;
        if let Some(voice) = &self.voice {
            let _ = unsafe { voice.SetRate(*rate) };
        }
        if let Some(path) = speed_file()
            && let Err(error) = std::fs::write(path, label)
        {
            tracing::warn!(%error, "translator speech speed could not be saved");
        }
    }

    pub fn stop(&mut self) {
        if let Some(v) = &self.voice {
            let _ = unsafe { v.Speak(PCWSTR::null(), 3, None) };
        }
        self.last = None;
    }

    pub fn read(&mut self, id: usize, text: &str, lang: &str) -> Result<()> {
        if text.is_empty() || !self.readable(lang) {
            self.stop();
            return Ok(());
        }
        if self.voice.is_none() {
            self.voice = Some(unsafe { CoCreateInstance(&SpVoice, None, CLSCTX_ALL) }?);
        }
        let voice = self.voice.as_ref().unwrap();
        let mut state = SPVOICESTATUS::default();
        if self.last == Some(id)
            && unsafe { voice.GetStatus(&mut state, std::ptr::null_mut()) }.is_ok()
            && state.dwRunningState == SPRS_IS_SPEAKING.0 as u32
        {
            self.stop();
            return Ok(());
        }
        unsafe { voice.SetRate(SPEEDS[self.speed].1) }?;
        let safe = text
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
        let xml = format!(
            "<voice required=\u{22}Language={}\u{22}>{safe}</voice>",
            code(lang)
        );
        if unsafe { voice.Speak(&HSTRING::from(xml), 11, None) }.is_err() {
            unsafe { voice.Speak(&HSTRING::from(text), 3 | 16, None) }?;
        }
        self.last = Some(id);
        Ok(())
    }
}

/// 语言代码对应的 SAPI 语言号（十六进制 LCID）；中文及未知语言用 804。
fn code(lang: &str) -> &'static str {
    LANGS.iter().find(|l| l.0 == lang).map_or("804", |l| l.2)
}

/// 本机是否装了这种语言的 SAPI 声音。
fn installed(code: &str) -> bool {
    (|| -> Result<bool> {
        unsafe {
            let category: ISpObjectTokenCategory =
                CoCreateInstance(&SpObjectTokenCategory, None, CLSCTX_ALL)?;
            category.SetId(SPCAT_VOICES, false)?;
            let tokens =
                category.EnumTokens(&HSTRING::from(format!("Language={code}")), PCWSTR::null())?;
            let mut n = 0;
            tokens.GetCount(&mut n)?;
            Ok(n > 0)
        }
    })()
    .unwrap_or(false)
}

fn speed_file() -> Option<std::path::PathBuf> {
    Some(dirs::user_dir()?.join("translator-speech-speed"))
}

#[cfg(test)]
mod tests {
    use super::{NORMAL, SPEEDS};

    #[test]
    fn speeds_are_ordered_and_normal_is_one() {
        assert_eq!(SPEEDS[NORMAL], ("1.0×", 0));
        assert!(SPEEDS.windows(2).all(|pair| pair[0].1 < pair[1].1));
        assert!(SPEEDS.iter().all(|s| (-10..=10).contains(&s.1)));
    }
}
