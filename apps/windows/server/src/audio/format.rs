//! 设备音频格式的描述，以及把一包交错多声道采样折成单声道 f32（-1..1）。
//!
//! 共享模式下设备混音格式多是 48 kHz 多声道 float32，也有 16 / 24 / 32 位整数；
//! 扩展格式头（WAVE_FORMAT_EXTENSIBLE）的真正类型在 SubFormat 里。

use std::fmt;

use windows::core::GUID;

use crate::dispatch::SAMPLE_RATE;

const WAVE_FORMAT_PCM: u16 = 1;
const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;

/// KSDATAFORMAT_SUBTYPE_PCM / _IEEE_FLOAT：只有第一段不同。
const SUBTYPE_PCM: GUID = GUID::from_u128(0x00000001_0000_0010_8000_00aa00389b71);
const SUBTYPE_FLOAT: GUID = GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleKind {
    Int16,

    /// 3 字节紧凑排列。
    Int24,

    /// 4 字节容器；有效位可能只有 24 位（值在高位，按 32 位换算即可）。
    Int32,

    Float32,
}

impl SampleKind {
    fn bytes(self) -> usize {
        match self {
            Self::Int16 => 2,
            Self::Int24 => 3,
            Self::Int32 | Self::Float32 => 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamFormat {
    pub kind: SampleKind,

    pub channels: u16,

    pub rate: u32,

    /// 一帧（所有声道各一个采样）的字节数。
    pub block_align: u16,
}

impl StreamFormat {
    /// 识别方要的格式：16 kHz 单声道 16 位。
    pub const TARGET: Self = Self {
        kind: SampleKind::Int16,
        channels: 1,
        rate: SAMPLE_RATE,
        block_align: 2,
    };

    /// 从 WAVEFORMATEX 的字段（及扩展头的 SubFormat）认格式；认不出回 `None`。
    pub fn from_parts(
        tag: u16,
        channels: u16,
        rate: u32,
        block_align: u16,
        bits: u16,
        sub_format: Option<GUID>,
    ) -> Option<Self> {
        let float = match tag {
            WAVE_FORMAT_PCM => false,
            WAVE_FORMAT_IEEE_FLOAT => true,
            WAVE_FORMAT_EXTENSIBLE => match sub_format? {
                SUBTYPE_PCM => false,
                SUBTYPE_FLOAT => true,
                _ => return None,
            },
            _ => return None,
        };
        let kind = match (float, bits) {
            (false, 16) => SampleKind::Int16,
            (false, 24) => SampleKind::Int24,
            (false, 32) => SampleKind::Int32,
            (true, 32) => SampleKind::Float32,
            _ => return None,
        };
        let format = Self {
            kind,
            channels,
            rate,
            block_align,
        };
        (channels > 0
            && rate > 0
            && usize::from(block_align) >= usize::from(channels) * kind.bytes())
        .then_some(format)
    }

    /// 把 `frames` 帧交错采样平均成单声道，追加到 `out`。`data` 不够长时按实际长度截。
    pub fn mix_to_mono(&self, data: &[u8], frames: usize, out: &mut Vec<f32>) {
        let block = usize::from(self.block_align);
        let width = self.kind.bytes();
        let channels = usize::from(self.channels);
        let frames = frames.min(data.len() / block.max(1));
        out.reserve(frames);
        for frame in data.chunks_exact(block).take(frames) {
            let sum: f32 = (0..channels)
                .map(|channel| sample(self.kind, &frame[channel * width..][..width]))
                .sum();
            out.push(sum / channels as f32);
        }
    }
}

fn sample(kind: SampleKind, bytes: &[u8]) -> f32 {
    match kind {
        SampleKind::Int16 => f32::from(i16::from_le_bytes([bytes[0], bytes[1]])) / 32_768.0,
        SampleKind::Int24 => {
            // 放进 i32 的高 24 位再右移，保留符号
            let value = i32::from_le_bytes([0, bytes[0], bytes[1], bytes[2]]) >> 8;
            value as f32 / 8_388_608.0
        }
        SampleKind::Int32 => {
            i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f32 / 2_147_483_648.0
        }
        SampleKind::Float32 => f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
    }
}

impl fmt::Display for StreamFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} Hz x {} ch {:?} (block {})",
            self.rate, self.channels, self.kind, self.block_align
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{
        SUBTYPE_FLOAT, SUBTYPE_PCM, SampleKind, StreamFormat, WAVE_FORMAT_EXTENSIBLE,
        WAVE_FORMAT_IEEE_FLOAT, WAVE_FORMAT_PCM,
    };

    #[test]
    fn recognizes_plain_float_and_extensible_headers() {
        let pcm = StreamFormat::from_parts(WAVE_FORMAT_PCM, 1, 16_000, 2, 16, None);
        assert_eq!(pcm, Some(StreamFormat::TARGET));
        let float = StreamFormat::from_parts(WAVE_FORMAT_IEEE_FLOAT, 2, 48_000, 8, 32, None);
        assert_eq!(float.map(|f| f.kind), Some(SampleKind::Float32));
        let ext = |sub, bits, block| {
            StreamFormat::from_parts(WAVE_FORMAT_EXTENSIBLE, 4, 48_000, block, bits, Some(sub))
                .map(|f| f.kind)
        };
        assert_eq!(ext(SUBTYPE_FLOAT, 32, 16), Some(SampleKind::Float32));
        assert_eq!(ext(SUBTYPE_PCM, 24, 12), Some(SampleKind::Int24));
        assert_eq!(ext(SUBTYPE_PCM, 32, 16), Some(SampleKind::Int32));
        // 未知子类型、8 位、块对齐小于声道数都拒绝
        assert_eq!(ext(windows::core::GUID::zeroed(), 32, 16), None);
        assert_eq!(
            StreamFormat::from_parts(WAVE_FORMAT_PCM, 1, 16_000, 1, 8, None),
            None
        );
        assert_eq!(ext(SUBTYPE_FLOAT, 32, 8), None);
    }

    #[test]
    fn averages_channels_for_every_sample_width() {
        let mono = |kind, block, data: &[u8]| {
            let format = StreamFormat {
                kind,
                channels: 2,
                rate: 48_000,
                block_align: block,
            };
            let mut out = Vec::new();
            format.mix_to_mono(data, 1, &mut out);
            out[0]
        };
        let i16s = [16_384_i16.to_le_bytes(), 0_i16.to_le_bytes()].concat();
        assert!((mono(SampleKind::Int16, 4, &i16s) - 0.25).abs() < 1e-6);
        // 24 位：-0.5 与 -0.5
        let half = (-4_194_304_i32).to_le_bytes();
        let i24s = [&half[..3], &half[..3]].concat();
        assert!((mono(SampleKind::Int24, 6, &i24s) + 0.5).abs() < 1e-6);
        let i32s = [(1_i32 << 30).to_le_bytes(), (1_i32 << 30).to_le_bytes()].concat();
        assert!((mono(SampleKind::Int32, 8, &i32s) - 0.5).abs() < 1e-6);
        let f32s = [0.5_f32.to_le_bytes(), (-0.25_f32).to_le_bytes()].concat();
        assert!((mono(SampleKind::Float32, 8, &f32s) - 0.125).abs() < 1e-6);
    }

    #[test]
    fn short_buffers_are_truncated_instead_of_panicking() {
        let format = StreamFormat {
            kind: SampleKind::Float32,
            channels: 2,
            rate: 48_000,
            block_align: 8,
        };
        let mut out = Vec::new();
        format.mix_to_mono(&[0; 12], 5, &mut out);
        assert_eq!(out.len(), 1);
    }
}
