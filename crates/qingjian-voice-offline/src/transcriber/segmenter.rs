//! 攒录音并切段：SenseVoice 一次识别整段，太长的一段既慢又容易掉字，所以当前段超过 [`MAX_SEGMENT`] 秒时，
//! 在它后半截最安静的地方切下前面那块，先识别成文字存着，后面的接着攒。

/// 已经识别好的前几段文字 + 还在攒的当前段录音。
#[derive(Debug)]
pub struct Segmenter {
    sample_rate: u32,

    /// 当前段，-1..1 浮点。
    current: Vec<f32>,

    /// 前面已切走那几段的文字。
    done: String,
}

impl Segmenter {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            sample_rate: sample_rate.max(1),
            current: Vec::new(),
            done: String::new(),
        }
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn push(&mut self, samples: &[i16]) {
        self.current
            .extend(samples.iter().map(|&sample| f32::from(sample) / 32_768.0));
    }

    /// 当前段的录音。
    pub fn current(&self) -> &[f32] {
        &self.current
    }

    /// 当前段的时长（秒）。
    pub fn seconds(&self) -> f32 {
        self.current.len() as f32 / self.sample_rate as f32
    }

    /// 当前段超长时切下前面一块交出去识别；不超长回 `None`。
    pub fn split_off(&mut self) -> Option<Vec<f32>> {
        let rate = self.sample_rate as usize;
        if self.current.len() <= MAX_SEGMENT * rate {
            return None;
        }
        let cut = quietest(&self.current, MIN_CUT * rate, MAX_SEGMENT * rate, rate / 10);
        let tail = self.current.split_off(cut);
        Some(std::mem::replace(&mut self.current, tail))
    }

    /// 切走那块识别出的文字接到前面。
    pub fn append_done(&mut self, text: &str) {
        self.done = join(&self.done, text);
    }

    /// 前面几段的文字接上当前段的 `tail`。
    pub fn text_with(&self, tail: &str) -> String {
        join(&self.done, tail)
    }
}

/// `[from, to)` 里按 `frame` 一帧算能量，回最安静那帧的中点。
fn quietest(samples: &[f32], from: usize, to: usize, frame: usize) -> usize {
    let frame = frame.max(1);
    let to = to.min(samples.len());
    let mut best = (f32::INFINITY, to);
    let mut start = from;
    while start + frame <= to {
        let energy: f32 = samples[start..start + frame].iter().map(|s| s * s).sum();
        if energy < best.0 {
            best = (energy, start + frame / 2);
        }
        start += frame;
    }
    best.1
}

/// 两段文字接起来：中文直接接，英文单词之间补空格。
fn join(head: &str, tail: &str) -> String {
    let (head, tail) = (head.trim(), tail.trim());
    if head.is_empty() {
        return tail.to_owned();
    }
    if tail.is_empty() {
        return head.to_owned();
    }
    let ascii = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric());
    if ascii(head.chars().last()) && ascii(tail.chars().next()) {
        format!("{head} {tail}")
    } else {
        format!("{head}{tail}")
    }
}

/// 当前段超过这么多秒就切。
pub const MAX_SEGMENT: usize = 20;

/// 切点不早于第几秒（太早切出的段太短，上下文不够）。
const MIN_CUT: usize = 10;

#[cfg(test)]
mod tests {
    use super::{Segmenter, join, quietest};

    #[test]
    fn joins_chinese_directly_and_english_with_space() {
        assert_eq!(join("你好。", "今天"), "你好。今天");
        assert_eq!(join("hello", "world"), "hello world");
        assert_eq!(join("", " 好 "), "好");
        assert_eq!(join("好", ""), "好");
    }

    #[test]
    fn finds_quiet_frame() {
        let mut samples = vec![0.5f32; 100];
        samples[60..70].fill(0.0);
        assert_eq!(quietest(&samples, 0, 100, 10), 65);
    }

    #[test]
    fn splits_long_segment_at_quiet_point() {
        let rate = 100;
        let mut segmenter = Segmenter::new(rate);
        let mut loud = vec![10_000i16; 21 * rate as usize];
        // 第 15 秒处有 0.1 秒安静
        loud[1500..1510].fill(0);
        segmenter.push(&loud[..19 * rate as usize]);
        assert!(segmenter.split_off().is_none());
        segmenter.push(&loud[19 * rate as usize..]);
        let head = segmenter.split_off().expect("over 20 s");
        assert_eq!(head.len(), 1505);
        assert_eq!(segmenter.current().len(), 21 * rate as usize - 1505);
        segmenter.append_done("第一段。");
        assert_eq!(segmenter.text_with("第二段"), "第一段。第二段");
    }
}
