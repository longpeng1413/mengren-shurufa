//! Windows 麦克风采集（WASAPI 共享模式）：交给识别方 16 kHz / 单声道 / 16 位 PCM。
//!
//! 每段语音开一个线程：打开设备（[`device`]）、必要时自己转格式（[`format`]、[`resample`]）、
//! 放大偏轻的声音（[`gain`]），轮询取包经 [`Capture`] 的通道交给 Router 的 tick。
//! 工厂等线程真正开始录音（或失败）才返回，打不开的原因直接显示在语音面板上；
//! 录到一半出错经 `failure` 通道报给 Router。用轮询不用事件回调：20 ms 一拍对语音足够，且停得快。

mod device;
mod error;
mod format;
mod gain;
mod resample;

use std::slice;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use qingjian_platform::windows_text::tr;
use windows::Win32::Media::Audio::{AUDCLNT_BUFFERFLAGS_SILENT, IAudioCaptureClient};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};

use self::error::{CaptureError, CaptureErrorKind};
use self::format::StreamFormat;
use self::gain::VoiceGain;
use self::resample::Resampler;
use crate::dispatch::{Capture, CaptureFactory, SAMPLE_RATE};

/// 取包间隔：20 ms 约 320 个 16 kHz 采样，识别实时性足够。
const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// 等录音线程打开设备的上限。正常几十毫秒；驱动卡住时不让 Router 线程一直等。
const START_TIMEOUT: Duration = Duration::from_millis(1500);

/// 建录音源工厂，交给 [`crate::dispatch::Router::set_voice_capture`]。
/// 每次调用开一段新录音；失败返回给用户看的原因。
pub fn capture_factory() -> CaptureFactory {
    Arc::new(|| {
        let (pcm_tx, pcm_rx) = mpsc::channel::<Vec<i16>>();
        let (level_tx, level_rx) = mpsc::channel::<f32>();
        let (status_tx, status_rx) = mpsc::channel::<Result<(), String>>();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        thread::Builder::new()
            .name("qingjian-voice-capture".to_owned())
            .spawn(move || run(&pcm_tx, &level_tx, &status_tx, &worker_stop))
            .map_err(|error| {
                tracing::warn!(%error, "cannot spawn voice capture thread");
                tr("mic").to_owned()
            })?;
        match status_rx.recv_timeout(START_TIMEOUT) {
            Ok(Ok(())) => Ok(Capture {
                pcm: pcm_rx,
                level: Some(level_rx),
                stop,
                failure: Some(status_rx),
            }),
            Ok(Err(message)) => Err(message),
            Err(RecvTimeoutError::Timeout) => {
                stop.store(true, Ordering::Relaxed);
                tracing::warn!(
                    timeout_ms = START_TIMEOUT.as_millis() as u64,
                    "voice capture did not start in time"
                );
                Err(tr("mic_timeout").to_owned())
            }
            Err(RecvTimeoutError::Disconnected) => Err(tr("mic").to_owned()),
        }
    })
}

/// 录音线程主体：初始化 COM，录到 `stop`；开始前出错经 `status` 报打不开，开始后出错报中断。
fn run(
    pcm: &Sender<Vec<i16>>,
    level: &Sender<f32>,
    status: &Sender<Result<(), String>>,
    stop: &AtomicBool,
) {
    // WASAPI 是 COM 接口，线程各自初始化；用 MTA 免得消息泵。
    let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if let Err(error) = initialized.ok() {
        let error = CaptureError::from_windows("CoInitializeEx", &error, CaptureErrorKind::Failed);
        tracing::warn!(%error, "voice capture failed");
        let _ = status.send(Err(error.message()));
        return;
    }
    if let Err(error) = unsafe { capture(pcm, level, status, stop) } {
        tracing::warn!(%error, "voice capture failed");
        let _ = status.send(Err(error.message()));
    }
    unsafe { CoUninitialize() };
}

/// 一段录音的统计，结束时写一行 info 日志。
#[derive(Default)]
struct Stats {
    packets: u32,

    silent_packets: u32,

    frames: u64,

    /// 原始（放大前）峰值，0..1。
    peak: f32,
}

unsafe fn capture(
    pcm: &Sender<Vec<i16>>,
    level: &Sender<f32>,
    status: &Sender<Result<(), String>>,
    stop: &AtomicBool,
) -> Result<(), CaptureError> {
    let opened = unsafe { device::open() }?;
    let client = &opened.client;
    let capture: IAudioCaptureClient = unsafe { client.GetService() }.map_err(|error| {
        CaptureError::from_windows("GetService", &error, CaptureErrorKind::Failed)
    })?;
    unsafe { client.Start() }
        .map_err(|error| CaptureError::from_windows("Start", &error, CaptureErrorKind::Failed))?;
    let _ = status.send(Ok(()));
    let started = Instant::now();
    tracing::info!(device = %opened.name, format = %opened.format, "voice capture started");

    let block = usize::from(opened.format.block_align);
    let mut converter = Converter::new(opened.format);
    let mut gain = VoiceGain::default();
    let mut stats = Stats::default();
    let result = loop {
        let mut max_level = 0.0f32;
        let mut got_audio = false;
        let drained = unsafe {
            drain(&capture, block, &mut |data, frames, silent| {
                got_audio = true;
                stats.packets += 1;
                stats.frames += u64::from(frames);
                if silent {
                    stats.silent_packets += 1;
                }
                let mut samples = converter.convert(data, frames as usize, silent);
                if !samples.is_empty() {
                    let peak = gain.process(&mut samples);
                    max_level = max_level.max(peak);
                    let _ = pcm.send(samples);
                }
            })
        };
        if let Err(error) = drained {
            break Err(error);
        }
        stats.peak = stats.peak.max(max_level);
        // 收到包就报电平，0 也报：Router 据此区分「麦克风交出纯静音」和「没有电平通道」
        if got_audio {
            let _ = level.send(max_level);
        }
        if stop.load(Ordering::Relaxed) {
            break Ok(());
        }
        thread::sleep(POLL_INTERVAL);
    };
    let _ = unsafe { client.Stop() };
    tracing::info!(
        device = %opened.name,
        seconds = format!("{:.2}", started.elapsed().as_secs_f32()),
        packets = stats.packets,
        silent_packets = stats.silent_packets,
        frames = stats.frames,
        peak = format!("{:.4}", stats.peak),
        peak_dbfs = format!("{:.1}", 20.0 * stats.peak.max(1e-6).log10()),
        "voice capture stopped"
    );
    result
}

/// 取完当前所有包，逐包交给 `each(数据, 帧数, 是否静音包)`；`block` 是一帧的字节数。
unsafe fn drain(
    capture: &IAudioCaptureClient,
    block: usize,
    each: &mut dyn FnMut(&[u8], u32, bool),
) -> Result<(), CaptureError> {
    let lost = |step, error: windows::core::Error| {
        CaptureError::from_windows(step, &error, CaptureErrorKind::Lost)
    };
    let mut available =
        unsafe { capture.GetNextPacketSize() }.map_err(|error| lost("GetNextPacketSize", error))?;
    while available > 0 {
        let mut data = std::ptr::null_mut();
        let mut frames = 0u32;
        let mut flags = 0u32;
        unsafe { capture.GetBuffer(&mut data, &mut frames, &mut flags, None, None) }
            .map_err(|error| lost("GetBuffer", error))?;
        let silent = flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 || data.is_null();
        let bytes: &[u8] = if silent {
            &[]
        } else {
            // SAFETY: GetBuffer 保证有 frames × nBlockAlign 字节可读，到 ReleaseBuffer 前有效
            unsafe { slice::from_raw_parts(data, frames as usize * block) }
        };
        each(bytes, frames, silent);
        let _ = unsafe { capture.ReleaseBuffer(frames) };
        available = unsafe { capture.GetNextPacketSize() }
            .map_err(|error| lost("GetNextPacketSize", error))?;
    }
    Ok(())
}

/// 设备格式 → 16 kHz 单声道 i16。系统已替我们转好时直接拷贝。
struct Converter {
    format: StreamFormat,

    resampler: Resampler,

    mono: Vec<f32>,

    resampled: Vec<f32>,
}

impl Converter {
    fn new(format: StreamFormat) -> Self {
        Self {
            format,
            resampler: Resampler::new(format.rate, SAMPLE_RATE),
            mono: Vec::new(),
            resampled: Vec::new(),
        }
    }

    /// `data` 是 `frames` 帧的原始字节；静音包不读数据，按帧数补零。
    fn convert(&mut self, data: &[u8], frames: usize, silent: bool) -> Vec<i16> {
        self.mono.clear();
        if silent {
            self.mono.resize(frames, 0.0);
        } else {
            if self.format == StreamFormat::TARGET {
                return data
                    .chunks_exact(2)
                    .take(frames)
                    .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
                    .collect();
            }
            self.format.mix_to_mono(data, frames, &mut self.mono);
        }
        if self.format == StreamFormat::TARGET {
            return vec![0; frames];
        }
        self.resampled.clear();
        self.resampler.push(&self.mono, &mut self.resampled);
        self.resampled
            .iter()
            .map(|sample| (sample * 32_768.0).round().clamp(-32_768.0, 32_767.0) as i16)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::Converter;
    use super::format::{SampleKind, StreamFormat};

    #[test]
    fn converts_stereo_float_48k_to_mono_16k() {
        let format = StreamFormat {
            kind: SampleKind::Float32,
            channels: 2,
            rate: 48_000,
            block_align: 8,
        };
        let mut converter = Converter::new(format);
        let frame = [0.5_f32.to_le_bytes(), 0.5_f32.to_le_bytes()].concat();
        let data = frame.repeat(4_800);
        let mut total = Vec::new();
        for _ in 0..10 {
            total.extend(converter.convert(&data, 4_800, false));
        }
        // 1 秒 48 kHz → 约 16000 个采样，直流 0.5 → 约 16384
        assert!((15_900..=16_000).contains(&total.len()));
        assert!((i32::from(total[8_000]) - 16_384).abs() < 50);
        assert_eq!(converter.convert(&[], 480, true).len(), 160);
    }

    #[test]
    fn target_format_is_copied_and_silence_is_zero_filled() {
        let mut converter = Converter::new(StreamFormat::TARGET);
        let data = [1_i16, -2, 3].map(i16::to_le_bytes).concat();
        assert_eq!(converter.convert(&data, 3, false), [1, -2, 3]);
        assert_eq!(converter.convert(&[], 4, true), [0; 4]);
    }

    /// 需要真实麦克风：两条初始化路径各录 1 秒，都应交出约 16000 个采样。
    #[test]
    #[ignore = "requires a real microphone"]
    fn real_microphone_records_16k_on_both_paths() {
        use std::sync::atomic::Ordering;
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();
        for force in [false, true] {
            super::device::FORCE_MIX_FORMAT.store(force, Ordering::Relaxed);
            let capture = (super::capture_factory())().expect("microphone opens");
            std::thread::sleep(std::time::Duration::from_millis(1000));
            capture.stop.store(true, Ordering::Relaxed);
            std::thread::sleep(std::time::Duration::from_millis(100));
            let samples: usize = capture.pcm.try_iter().map(|chunk| chunk.len()).sum();
            println!("force_mix={force} samples={samples}");
            assert!((14_000..=18_500).contains(&samples), "{samples}");
        }
        super::device::FORCE_MIX_FORMAT.store(false, Ordering::Relaxed);
    }
}
