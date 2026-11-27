//! 认悬停：鼠标停住够久（按住 Ctrl 方式要一直按着 Ctrl、期间没按别的键）就在停住的地方取词，
//! 同一个停留点只取一次，挪开再停才再取。按着鼠标键时不算（拖选、拖窗口）。

use std::time::{Duration, Instant};

use qingjian_platform::HoverMode;

/// 鼠标挪动多少像素以内还算停着（物理像素）。
const SLOP: i32 = 4;

/// 按住 Ctrl 时停多久取词：太短的话按 Ctrl+C 时 C 还没按下卡片就弹了（0.15 秒时用户常遇到）。
const CTRL_DELAY: Duration = Duration::from_millis(500);

/// 不按 Ctrl 时停多久取词：短了鼠标随手一停就弹。
const REST_DELAY: Duration = Duration::from_millis(500);

/// 这一刻的输入状态，由定时器采样。
#[derive(Debug, Clone, Copy)]
pub(super) struct Sample {
    pub(super) x: i32,

    pub(super) y: i32,

    /// Ctrl 按着，而且从按下到现在没按过别的键（不是 Ctrl+C 这类快捷键）。
    pub(super) ctrl: bool,

    /// 有鼠标键按着。
    pub(super) button: bool,
}

#[derive(Debug, Default)]
pub(super) struct Dwell {
    mode: HoverMode,

    /// 停留点与开始停留的时刻。
    rest: Option<(i32, i32, Instant)>,

    /// 这个停留点已经取过词。
    fired: bool,
}

impl Dwell {
    pub(super) fn set_mode(&mut self, mode: HoverMode) {
        self.mode = mode;
        self.rest = None;
        self.fired = false;
    }

    pub(super) fn mode(&self) -> HoverMode {
        self.mode
    }

    /// 定时器每次采样调一次；该取词时回停留点。
    pub(super) fn tick(&mut self, sample: Sample, now: Instant) -> Option<(i32, i32)> {
        let armed = match self.mode {
            HoverMode::Off => false,
            HoverMode::Ctrl => sample.ctrl,
            HoverMode::Always => true,
        };
        if !armed || sample.button {
            self.rest = None;
            self.fired = false;
            return None;
        }
        let moved = self
            .rest
            .is_none_or(|(x, y, _)| (sample.x - x).abs() > SLOP || (sample.y - y).abs() > SLOP);
        if moved {
            self.rest = Some((sample.x, sample.y, now));
            self.fired = false;
            return None;
        }
        let (x, y, since) = self.rest?;
        let delay = if self.mode == HoverMode::Ctrl {
            CTRL_DELAY
        } else {
            REST_DELAY
        };
        if self.fired || now.duration_since(since) < delay {
            return None;
        }
        self.fired = true;
        Some((x, y))
    }
}
