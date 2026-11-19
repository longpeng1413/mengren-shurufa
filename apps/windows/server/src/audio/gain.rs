//! 逐包放大偏轻的麦克风信号，让流式识别不必等整段录音结束。

use crate::dispatch::SAMPLE_RATE;

const FULL_SCALE: f32 = 32_768.0;
const TARGET_RMS: f32 = FULL_SCALE * 0.1; // -20 dBFS
const MAX_PEAK: f32 = 29_204.0; // -1 dBFS，向下取整
const MAX_GAIN: f32 = 15.848_932; // +24 dB
const MIN_GAIN: f32 = 1.122_018_5; // +1 dB
const NOISE_FLOOR: f32 = FULL_SCALE * 0.001; // -60 dBFS
const RISE_SAMPLES: f32 = SAMPLE_RATE as f32 * 0.040;

pub(super) struct VoiceGain {
    gain: f32,
}

impl Default for VoiceGain {
    fn default() -> Self {
        Self { gain: 1.0 }
    }
}

impl VoiceGain {
    /// 修改 PCM 后返回原始峰值，供波形面板显示实际麦克风电平。
    pub(super) fn process(&mut self, samples: &mut [i16]) -> f32 {
        if samples.is_empty() {
            return 0.0;
        }

        let mut peak = 0.0_f32;
        let mut energy = 0.0_f32;
        for &sample in samples.iter() {
            let sample = f32::from(sample);
            peak = peak.max(sample.abs());
            energy += sample * sample;
        }
        let raw_level = peak / FULL_SCALE;
        let rms = (energy / samples.len() as f32).sqrt();
        if rms < NOISE_FLOOR {
            self.gain = 1.0;
            return raw_level;
        }

        let target = (TARGET_RMS / rms).min(MAX_PEAK / peak).min(MAX_GAIN);
        if target < MIN_GAIN {
            self.gain = 1.0;
            return raw_level;
        }

        // 大声来时立即限制增益，音量回落后用约 40 ms 的时间常数平滑恢复。
        let step = 1.0 - (-1.0 / RISE_SAMPLES).exp();
        let mut gain = self.gain.min(target);
        for sample in samples {
            gain += (target - gain) * step;
            *sample = (f32::from(*sample) * gain).round() as i16;
        }
        self.gain = gain;
        raw_level
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quiet_pcm_reaches_the_target_without_changing_the_waveform_meter() {
        let mut gain = VoiceGain::default();
        let mut samples = [650, -650].repeat(160);
        assert_eq!(gain.process(&mut samples), 650.0 / FULL_SCALE);
        assert!(samples[318] > 1_500);
        for _ in 0..10 {
            samples = [650, -650].repeat(160);
            gain.process(&mut samples);
        }
        assert!((f32::from(samples[318]) - TARGET_RMS).abs() < 15.0);
        assert_eq!(samples[318], -samples[319]);
    }

    #[test]
    fn caps_gain_and_protects_a_loud_transient_after_quiet_speech() {
        let mut gain = VoiceGain::default();
        let mut faint = [80; 3_200];
        gain.process(&mut faint);
        assert!((1_250..=1_268).contains(&faint[3_199]));
        let mut transient = [100; 320];
        transient[0] = -12_000;
        gain.process(&mut transient);
        assert_eq!(transient[0], -(MAX_PEAK as i16));

        let mut already_loud = [i16::MIN, i16::MAX];
        gain.process(&mut already_loud);
        assert_eq!(already_loud, [i16::MIN, i16::MAX]);
    }

    #[test]
    fn does_not_raise_the_noise_floor_and_resets_gain_between_sessions() {
        let mut gain = VoiceGain::default();
        let mut noise = [20, -20];
        assert_eq!(gain.process(&mut []), 0.0);
        gain.process(&mut [0; 320]);
        gain.process(&mut [100; 3_200]);
        gain.process(&mut noise);
        assert_eq!(noise, [20, -20]);
        assert_eq!(gain.gain, 1.0);
    }
}
