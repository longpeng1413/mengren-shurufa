//! 朗读来源：本机没有这门语言的声音时要不要联网用百度翻译的发音。

/// 朗读来源，存在 `%APPDATA%\Qingjian\translator-speech-source`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Source {
    /// 本机有声音用本机，没有再联网用百度（缺省）。
    #[default]
    Auto,

    /// 只用本机声音，不联网、不把文字发给百度。
    Local,
}

impl Source {
    pub fn key(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Local => "local",
        }
    }

    pub fn parse(text: &str) -> Self {
        if text.trim() == "local" {
            Self::Local
        } else {
            Self::Auto
        }
    }
}
