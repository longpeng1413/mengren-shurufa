//! 朗读的是原文还是译文：两边各有自己的倍速。

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Source,

    Result,
}

impl Side {
    /// 记住倍速的文件名；译文沿用加原文倍速之前的旧文件名，老用户的设置不丢。
    pub(super) fn speed_file(self) -> &'static str {
        match self {
            Side::Source => "translator-speech-speed-source",
            Side::Result => "translator-speech-speed",
        }
    }
}
