//! 循环播放：读完一遍停一会儿再读，读够次数为止。开关与次数存一个小文件，翻译窗口与划词卡片共用。

use std::time::{Duration, Instant};

use super::{Side, read_file, write_file};

/// 可选的循环次数（连第一遍在内）。
pub const COUNTS: [u32; 4] = [2, 3, 5, 10];

/// 默认循环次数。
const DEFAULT_COUNT: u32 = 3;

/// 两遍之间停多久。
const GAP: Duration = Duration::from_millis(400);

/// 开读后多久还没听到声音就不再等（联网朗读要先下载）。
const START_TIMEOUT: Duration = Duration::from_secs(6);

/// 记住开关与次数的文件（`on 3` / `off 3`）。
const FILE: &str = "translator-speech-loop";

/// 还要再读的那一段。
#[derive(Debug)]
struct Pending {
    side: Side,

    text: String,

    language: String,

    /// 还剩几遍。
    left: u32,

    /// 这一遍开读的时刻。
    started: Instant,

    /// 这一遍真的出过声（系统语音刚开读的一瞬间、联网朗读下载时都还不算在读）。
    heard: bool,

    /// 读完（不出声了）的时刻，隔 [`GAP`] 再读下一遍。
    idle_since: Option<Instant>,
}

#[derive(Debug)]
pub struct Repeat {
    pub on: bool,

    /// 连第一遍在内一共读几遍。
    pub count: u32,

    pending: Option<Pending>,
}

impl Repeat {
    /// 读上次的开关与次数；没存过是关、3 次。
    pub fn load() -> Self {
        let (on, count) = read_file(FILE).map_or((false, DEFAULT_COUNT), |text| parse(&text));
        Self {
            on,
            count,
            pending: None,
        }
    }

    /// 改开关与次数并记住；关掉时连正在等的下一遍一起取消。
    pub fn set(&mut self, on: bool, count: u32) {
        self.on = on;
        self.count = count;
        if !on {
            self.pending = None;
        }
        write_file(FILE, &format!("{} {count}", if on { "on" } else { "off" }));
    }

    /// 刚开读一段：开着循环就记下还要再读几遍。
    pub fn arm(&mut self, side: Side, text: &str, language: &str, now: Instant) {
        self.pending = (self.on && self.count > 1).then(|| Pending {
            side,
            text: text.to_owned(),
            language: language.to_owned(),
            left: self.count - 1,
            started: now,
            heard: false,
            idle_since: None,
        });
    }

    pub fn clear(&mut self) {
        self.pending = None;
    }

    /// 还在循环（正在读或等着读下一遍）的是哪一边。
    pub fn side(&self) -> Option<Side> {
        self.pending.as_ref().map(|pending| pending.side)
    }

    /// 定时器里调：上一遍读完、隔够了，回下一遍要读的那一边、文字、语言。
    pub fn next(&mut self, speaking: bool, now: Instant) -> Option<(Side, String, String)> {
        let pending = self.pending.as_mut()?;
        if speaking {
            pending.heard = true;
            pending.idle_since = None;
            return None;
        }
        if !pending.heard {
            if now.duration_since(pending.started) > START_TIMEOUT {
                self.pending = None;
            }
            return None;
        }
        let since = *pending.idle_since.get_or_insert(now);
        if now.duration_since(since) < GAP {
            return None;
        }
        let item = (pending.side, pending.text.clone(), pending.language.clone());
        pending.left -= 1;
        pending.started = now;
        pending.heard = false;
        pending.idle_since = None;
        if pending.left == 0 {
            self.pending = None;
        }
        Some(item)
    }
}

/// `on 3` / `off 5`：开关与次数；读不懂的部分用默认值，次数只认 [`COUNTS`] 里的。
fn parse(text: &str) -> (bool, u32) {
    let mut parts = text.split_whitespace();
    let on = parts.next() == Some("on");
    let count = parts
        .next()
        .and_then(|count| count.parse().ok())
        .filter(|count| COUNTS.contains(count))
        .unwrap_or(DEFAULT_COUNT);
    (on, count)
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::{Repeat, parse};
    use crate::window::speech::Side;

    #[test]
    fn saved_settings_parse_with_defaults() {
        assert_eq!(parse("on 5"), (true, 5));
        assert_eq!(parse("off 10"), (false, 10));
        assert_eq!(parse("on 7"), (true, 3), "only listed counts");
        assert_eq!(parse(""), (false, 3));
    }

    #[test]
    fn plays_the_remaining_rounds_after_each_one_finishes() {
        let mut repeat = Repeat {
            on: true,
            count: 3,
            pending: None,
        };
        let t = Instant::now();
        let at = |ms: u64| t + Duration::from_millis(ms);
        repeat.arm(Side::Source, "hello", "en", t);
        assert_eq!(repeat.next(false, at(100)), None, "not started yet");
        assert_eq!(repeat.next(true, at(200)), None);
        assert_eq!(repeat.next(false, at(900)), None, "just finished");
        let second = repeat.next(false, at(1400));
        assert_eq!(second.map(|item| item.1), Some("hello".to_owned()));
        assert_eq!(repeat.side(), Some(Side::Source));
        assert_eq!(repeat.next(true, at(1500)), None);
        assert_eq!(repeat.next(false, at(2000)), None);
        assert!(
            repeat.next(false, at(2500)).is_some(),
            "third and last round"
        );
        assert_eq!(repeat.side(), None);
    }

    #[test]
    fn off_or_silent_never_repeats() {
        let t = Instant::now();
        let mut repeat = Repeat {
            on: false,
            count: 3,
            pending: None,
        };
        repeat.arm(Side::Result, "x", "en", t);
        assert_eq!(repeat.side(), None);
        repeat.on = true;
        repeat.arm(Side::Result, "x", "en", t);
        assert_eq!(repeat.next(false, t + Duration::from_secs(7)), None);
        assert_eq!(repeat.side(), None, "gave up: never heard");
    }
}
