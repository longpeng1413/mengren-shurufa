//! 流式重采样到 16 kHz：加窗 sinc 插值，降采样时截止频率随比例下调，免得高频折叠成杂音。
//! 只在系统不肯替我们转格式（AUTOCONVERTPCM 被拒）时用；语音只要 8 kHz 以下，16 个旁瓣足够。

use std::f32::consts::PI;

/// 每侧取多少个输入采样（按输出相对的输入间隔放大）。
const HALF_TAPS: f32 = 16.0;

pub struct Resampler {
    /// 输入采样率 / 输出采样率。
    step: f64,

    /// 下一个输出点在 `history` 里的位置（可带小数）。
    position: f64,

    /// 截止频率（相对输入奈奎斯特频率，≤ 1）。
    cutoff: f32,

    /// 卷积半宽（输入采样数）。
    half: usize,

    history: Vec<f32>,
}

impl Resampler {
    pub fn new(input_rate: u32, output_rate: u32) -> Self {
        let step = f64::from(input_rate) / f64::from(output_rate);
        let cutoff = (1.0 / step as f32).min(1.0) * 0.95;
        Self {
            step,
            position: 0.0,
            cutoff,
            half: (HALF_TAPS / cutoff).ceil() as usize,
            history: Vec::new(),
        }
    }

    pub fn passthrough(&self) -> bool {
        self.step == 1.0
    }

    /// 喂一段输入，追加能算出的输出。开头不足半宽的部分按 0 补。
    pub fn push(&mut self, input: &[f32], out: &mut Vec<f32>) {
        if self.passthrough() {
            out.extend_from_slice(input);
            return;
        }
        if self.history.is_empty() {
            // 开头补零，第一个输出点就能取到左侧采样
            self.history.resize(self.half, 0.0);
            self.position = self.half as f64;
        }
        self.history.extend_from_slice(input);
        while (self.position as usize) + self.half < self.history.len() {
            out.push(self.interpolate(self.position));
            self.position += self.step;
        }
        // 丢掉以后用不到的旧采样
        let keep_from = (self.position as usize).saturating_sub(self.half);
        if keep_from > 0 {
            self.history.drain(..keep_from);
            self.position -= keep_from as f64;
        }
    }

    fn interpolate(&self, at: f64) -> f32 {
        let center = at.floor() as isize;
        let fraction = (at - at.floor()) as f32;
        let half = self.half as isize;
        let mut sum = 0.0;
        let mut weights = 0.0;
        for offset in (1 - half)..=half {
            let Some(&value) = usize::try_from(center + offset)
                .ok()
                .and_then(|i| self.history.get(i))
            else {
                continue;
            };
            let distance = offset as f32 - fraction;
            let weight = self.kernel(distance);
            sum += value * weight;
            weights += weight;
        }
        // 归一化：直流增益保持 1，截断窗口带来的微小偏差也一并抵消
        if weights.abs() > f32::EPSILON {
            sum / weights
        } else {
            0.0
        }
    }

    fn kernel(&self, distance: f32) -> f32 {
        let x = distance * self.cutoff;
        let sinc = if x.abs() < 1e-6 {
            1.0
        } else {
            (PI * x).sin() / (PI * x)
        };
        // Hann 窗
        let ratio = (distance / (self.half as f32 + 1.0)).clamp(-1.0, 1.0);
        let window = 0.5 + 0.5 * (PI * ratio).cos();
        sinc * window
    }
}

#[cfg(test)]
mod tests {
    use super::Resampler;
    use std::f32::consts::PI;

    fn tone(rate: u32, hz: f32, seconds: f32) -> Vec<f32> {
        (0..(rate as f32 * seconds) as usize)
            .map(|i| (2.0 * PI * hz * i as f32 / rate as f32).sin() * 0.5)
            .collect()
    }

    fn rms(samples: &[f32]) -> f32 {
        (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
    }

    #[test]
    fn keeps_speech_band_and_length_across_odd_chunks() {
        for rate in [48_000, 44_100, 32_000, 96_000, 8_000] {
            let input = tone(rate, 440.0, 1.0);
            let mut resampler = Resampler::new(rate, 16_000);
            let mut out = Vec::new();
            for chunk in input.chunks(rate as usize / 100 + 7) {
                resampler.push(chunk, &mut out);
            }
            assert!(
                (15_900..=16_000).contains(&out.len()),
                "rate {rate}: {} samples",
                out.len()
            );
            // 去掉开头补零的过渡段后，440 Hz 的能量应保持（0.5 振幅正弦 RMS ≈ 0.354）
            let level = rms(&out[200..out.len() - 200]);
            assert!((level - 0.3536).abs() < 0.01, "rate {rate}: rms {level}");
        }
    }

    #[test]
    fn suppresses_tones_above_the_new_nyquist() {
        let input = tone(48_000, 12_000.0, 0.5);
        let mut resampler = Resampler::new(48_000, 16_000);
        let mut out = Vec::new();
        resampler.push(&input, &mut out);
        assert!(rms(&out[200..out.len() - 200]) < 0.03);
    }

    #[test]
    fn same_rate_passes_samples_through() {
        let mut resampler = Resampler::new(16_000, 16_000);
        let mut out = Vec::new();
        resampler.push(&[0.1, -0.2], &mut out);
        assert_eq!(out, [0.1, -0.2]);
        assert!(resampler.passthrough());
    }
}
