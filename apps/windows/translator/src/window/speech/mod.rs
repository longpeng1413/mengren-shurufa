//! 朗读：本机声音优先（经典 SAPI 与系统新语音都认），没有这门语言的声音时按「朗读来源」联网用百度翻译的发音。
//!
//! 本机有哪些语言的声音在窗口每次切到前台时重查（窗口收起后进程会留十分钟，用户也可能开着窗口去装语音）；没有声音又不能联网读的语言不朗读（用默认中文声音硬读出来是错的），
//! 由窗口底部的小字说明。原文、译文的倍速档位与朗读来源各存一个小文件。
mod online;
mod repeat;
mod side;
mod source;
mod voices;

pub use online::{MM_MCINOTIFY, Outcome, VOICE_IN};
pub use repeat::{COUNTS as LOOP_COUNTS, Repeat};
pub use side::Side;
pub use source::Source;

use super::translation::LANGS;
use online::Online;
use qingjian_platform::dirs;
use windows::{
    Win32::{
        Foundation::HWND,
        Media::Speech::{ISpObjectToken, ISpVoice, SPRS_IS_SPEAKING, SPVOICESTATUS, SpVoice},
        System::Com::{CLSCTX_ALL, CoCreateInstance},
    },
    core::{HSTRING, PCWSTR, Result},
};

/// 朗读倍速档位、对应的 SAPI 语速与 MCI 倍速（千分之一）。
/// SAPI 语速 10 约为 3 倍、-10 约为 1/3，按对数换算取整。
pub const SPEEDS: [(&str, i32, u32); 6] = [
    ("0.5×", -6, 500),
    ("0.75×", -3, 750),
    ("1.0×", 0, 1000),
    ("1.25×", 2, 1250),
    ("1.5×", 4, 1500),
    ("2.0×", 6, 2000),
];

/// 默认档位（1.0×）。
const NORMAL: usize = 2;

/// SAPI `Speak` 标志：异步、先清掉在读的、纯文本。
const SPEAK_FLAGS: u32 = 1 | 2 | 16;

pub struct Speech {
    voice: Option<ISpVoice>,

    /// 正在朗读的是哪一边，再点同一个按钮就停。
    last: Option<Side>,

    /// 本机有声音的语言（`zh`、`en`…）及选用的声音。
    voices: Vec<(&'static str, ISpObjectToken)>,

    online: Online,

    /// 原文的倍速在 [`SPEEDS`] 里的下标。
    source_speed: usize,

    /// 译文的倍速在 [`SPEEDS`] 里的下标。
    result_speed: usize,

    pub source: Source,

    /// 循环播放的开关、次数与还没读完的几遍。
    pub repeat: Repeat,
}

impl Speech {
    /// 查本机声音、读上次的倍速与朗读来源。调用前线程已初始化 COM。
    pub fn new(hwnd: HWND) -> Self {
        let voices = scan();
        let saved = |side: Side| {
            read_file(side.speed_file())
                .and_then(|text| SPEEDS.iter().position(|s| s.0 == text.trim()))
        };
        let result_speed = saved(Side::Result).unwrap_or(NORMAL);
        // 原文倍速是后加的：没存过就跟译文一样
        let source_speed = saved(Side::Source).unwrap_or(result_speed);
        let source = read_file("translator-speech-source")
            .map(|text| Source::parse(&text))
            .unwrap_or_default();
        Self {
            voice: None,
            last: None,
            voices,
            online: Online::new(hwnd),
            source_speed,
            result_speed,
            source,
            repeat: Repeat::load(),
        }
    }

    /// 重新查本机声音：窗口收起后进程还留着，期间用户可能在系统设置里新装了语音。
    pub fn refresh(&mut self) {
        self.voices = scan();
    }

    /// 本机有没有这种语言的声音。
    pub fn local(&self, lang: &str) -> bool {
        self.voices.iter().any(|(l, _)| *l == lang)
    }

    /// 能不能朗读：本机有声音，或「自动」时百度能读。
    pub fn readable(&self, lang: &str) -> bool {
        self.local(lang) || self.online_reads(lang)
    }

    fn online_reads(&self, lang: &str) -> bool {
        self.source == Source::Auto && online::code(lang).is_some()
    }

    /// 本机能朗读的语言（中文在前）。
    pub fn local_languages(&self) -> Vec<&'static str> {
        self.voices.iter().map(|(lang, _)| *lang).collect()
    }

    /// 本机没有、联网（百度）能读的语言；「只用本机」时为空。
    pub fn online_languages(&self) -> Vec<&'static str> {
        std::iter::once("zh")
            .chain(LANGS.iter().map(|l| l.0))
            .filter(|lang| !self.local(lang) && self.online_reads(lang))
            .collect()
    }

    /// 这一边的倍速在 [`SPEEDS`] 里的下标。
    pub fn speed(&self, side: Side) -> usize {
        match side {
            Side::Source => self.source_speed,
            Side::Result => self.result_speed,
        }
    }

    /// 换这一边的倍速并记住；正在读的正是这一边时也跟着变。
    pub fn set_speed(&mut self, side: Side, index: usize) {
        let Some((label, rate, mci)) = SPEEDS.get(index) else {
            return;
        };
        match side {
            Side::Source => self.source_speed = index,
            Side::Result => self.result_speed = index,
        }
        if self.last == Some(side) {
            if let Some(voice) = &self.voice {
                let _ = unsafe { voice.SetRate(*rate) };
            }
            self.online.set_speed(*mci);
        }
        write_file(side.speed_file(), label);
    }

    /// 换朗读来源并记住。
    pub fn set_source(&mut self, source: Source) {
        self.source = source;
        write_file("translator-speech-source", source.key());
    }

    pub fn stop(&mut self) {
        if let Some(v) = &self.voice {
            let _ = unsafe { v.Speak(PCWSTR::null(), 3, None) };
        }
        self.online.stop();
        self.last = None;
        self.repeat.clear();
    }

    /// 定时器里调：开着循环且上一遍读完了，就接着读下一遍。
    pub fn tick(&mut self) {
        let speaking = self.speaking();
        if let Some((side, text, language)) = self.repeat.next(speaking, std::time::Instant::now())
        {
            tracing::info!(?side, "speech repeat round");
            if self.start(side, &text, &language).is_err() {
                self.repeat.clear();
            }
        }
    }

    /// 读 `text`；同一边正在读（或循环中等着读下一遍）时再点就停。开着循环时读够次数。
    pub fn read(&mut self, side: Side, text: &str, lang: &str) -> Result<()> {
        if self.last == Some(side) && (self.speaking() || self.repeat.side() == Some(side)) {
            self.stop();
            return Ok(());
        }
        self.stop();
        if text.trim().is_empty() {
            return Ok(());
        }
        if self.start(side, text, lang)? {
            self.repeat.arm(side, text, lang, std::time::Instant::now());
        }
        Ok(())
    }

    /// 开读一遍，不动循环状态；读不了（没有这门语言的声音）回 `false`。
    fn start(&mut self, side: Side, text: &str, lang: &str) -> Result<bool> {
        let token = self
            .voices
            .iter()
            .find(|(l, _)| *l == lang)
            .map(|(_, token)| token.clone());
        match token {
            Some(token) => {
                if self.voice.is_none() {
                    self.voice = Some(unsafe { CoCreateInstance(&SpVoice, None, CLSCTX_ALL) }?);
                }
                let voice = self.voice.as_ref().unwrap();
                unsafe {
                    voice.SetVoice(&token)?;
                    voice.SetRate(SPEEDS[self.speed(side)].1)?;
                    voice.Speak(&HSTRING::from(text), SPEAK_FLAGS, None)?;
                }
            }
            None if self.online_reads(lang) => {
                self.online.set_speed(SPEEDS[self.speed(side)].2);
                self.online.start(text, lang);
            }
            None => return Ok(false),
        }
        self.last = Some(side);
        Ok(true)
    }

    /// 窗口收到 [`VOICE_IN`]。
    pub fn online_ready(&mut self) -> Option<Outcome> {
        self.online.ready()
    }

    /// 窗口收到 [`MM_MCINOTIFY`]。
    pub fn online_notify(&mut self, flag: usize) {
        self.online.notify(flag);
    }

    fn speaking(&self) -> bool {
        if self.online.active() {
            return true;
        }
        let Some(voice) = &self.voice else {
            return false;
        };
        let mut state = SPVOICESTATUS::default();
        unsafe { voice.GetStatus(&mut state, std::ptr::null_mut()) }.is_ok()
            && state.dwRunningState == SPRS_IS_SPEAKING.0 as u32
    }
}

/// 本机有声音的语言（中文在前）及各自选用的声音。
fn scan() -> Vec<(&'static str, ISpObjectToken)> {
    std::iter::once(("zh", "804"))
        .chain(LANGS.iter().map(|l| (l.0, l.2)))
        .filter_map(|(lang, code)| voices::find(code).map(|token| (lang, token)))
        .collect()
}

fn read_file(name: &str) -> Option<String> {
    std::fs::read_to_string(dirs::user_dir()?.join(name)).ok()
}

fn write_file(name: &str, text: &str) {
    if let Some(path) = dirs::user_dir().map(|dir| dir.join(name))
        && let Err(error) = std::fs::write(path, text)
    {
        tracing::warn!(%error, name, "translator speech setting could not be saved");
    }
}

#[cfg(test)]
mod tests {
    use super::{NORMAL, SPEEDS, Source};

    #[test]
    fn speeds_are_ordered_and_normal_is_one() {
        assert_eq!(SPEEDS[NORMAL], ("1.0×", 0, 1000));
        assert!(SPEEDS.windows(2).all(|pair| pair[0].1 < pair[1].1));
        assert!(SPEEDS.windows(2).all(|pair| pair[0].2 < pair[1].2));
        assert!(SPEEDS.iter().all(|s| (-10..=10).contains(&s.1)));
    }

    #[test]
    fn source_round_trips_and_defaults_to_auto() {
        assert_eq!(Source::parse(Source::Local.key()), Source::Local);
        assert_eq!(Source::parse(Source::Auto.key()), Source::Auto);
        assert_eq!(Source::parse("garbage"), Source::Auto);
    }
}
