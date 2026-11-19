//! Windows 麦克风采集（WASAPI 共享模式）：让系统自动把设备格式重采样成 16 kHz / 单声道 / 16 位，
//! 后台线程轮询取包、算音量，经 [`Capture`] 的通道交给 Router 的 tick 喂给识别方。
//!
//! 每段语音开一个线程，停时置 `stop` 标志，线程收尾（Stop + ReleaseBuffer + CoUninitialize）后退出。
//! 用轮询而不是事件回调：省去 CreateEvent / SetEvent 的句柄管理，20 ms 一拍对语音足够，且停得快。

mod gain;

use std::ptr;
use std::slice;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::Duration;

use windows::Win32::Media::Audio::{
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, IAudioCaptureClient, IAudioClient,
    IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator, WAVE_FORMAT_PCM, WAVEFORMATEX, eCapture,
    eCommunications,
};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};

use self::gain::VoiceGain;
use crate::dispatch::{Capture, CaptureFactory, SAMPLE_RATE};

/// 取包间隔：20 ms 约 320 个 16 kHz 采样，识别实时性足够。
const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// 建录音源工厂，交给 [`crate::dispatch::Router::set_voice_capture`]。
/// 每次调用开一段新录音；失败返回给用户看的原因。
pub fn capture_factory() -> CaptureFactory {
    Arc::new(|| {
        let (pcm_tx, pcm_rx) = mpsc::channel::<Vec<i16>>();
        let (level_tx, level_rx) = mpsc::channel::<f32>();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        thread::Builder::new()
            .name("qingjian-voice-capture".to_owned())
            .spawn(move || {
                if let Err(error) = run(pcm_tx, level_tx, worker_stop) {
                    tracing::warn!(%error, "语音录音线程退出");
                }
            })
            .map_err(|error| error.to_string())?;
        Ok(Capture {
            pcm: pcm_rx,
            level: Some(level_rx),
            stop,
        })
    })
}

/// 录音线程主体：初始化 COM 与音频客户端，循环取包直到 `stop`，出错向上报。
fn run(pcm: Sender<Vec<i16>>, level: Sender<f32>, stop: Arc<AtomicBool>) -> Result<(), String> {
    // WASAPI 是 COM 接口，线程各自初始化；STA/MTA 都行，用 MTA 免得消息泵。
    let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if initialized.is_err() {
        return Err(format!("CoInitializeEx: {initialized:?}"));
    }
    let result = capture_loop(&pcm, &level, &stop);
    unsafe { CoUninitialize() };
    result
}

fn capture_loop(
    pcm: &Sender<Vec<i16>>,
    level: &Sender<f32>,
    stop: &AtomicBool,
) -> Result<(), String> {
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|e| format!("枚举音频设备失败: {e}"))?;
        let device: IMMDevice = enumerator
            .GetDefaultAudioEndpoint(eCapture, eCommunications)
            .map_err(|e| format!("没有默认麦克风: {e}"))?;
        let client: IAudioClient = device
            .Activate(CLSCTX_ALL, None)
            .map_err(|e| format!("打开音频客户端失败: {e}"))?;

        // 想要的格式：16 kHz 单声道 PCM 16 位。共享模式下系统负责把设备格式重采样过来。
        let format = WAVEFORMATEX {
            wFormatTag: WAVE_FORMAT_PCM as u16,
            nChannels: 1,
            nSamplesPerSec: SAMPLE_RATE,
            nAvgBytesPerSec: SAMPLE_RATE * 2,
            nBlockAlign: 2,
            wBitsPerSample: 16,
            cbSize: 0,
        };
        // 缓冲 1 秒（100 ns 单位）；共享模式周期传 0。
        let hns_buffer = 10_000_000i64;
        client
            .Initialize(AUDCLNT_SHAREMODE_SHARED, 0, hns_buffer, 0, &format, None)
            .map_err(|e| format!("初始化录音失败（设备可能不支持 16 kHz）: {e}"))?;
        let capture: IAudioCaptureClient = client
            .GetService()
            .map_err(|e| format!("取录音客户端失败: {e}"))?;
        client.Start().map_err(|e| format!("开始录音失败: {e}"))?;

        let mut gain = VoiceGain::default();
        loop {
            let mut max_level = 0.0f32;
            let mut packet = || -> Result<(), String> {
                let mut available = capture
                    .GetNextPacketSize()
                    .map_err(|e| format!("取包大小失败: {e}"))?;
                while available > 0 {
                    let mut data = ptr::null_mut();
                    let mut frames = 0u32;
                    let mut flags = 0u32;
                    capture
                        .GetBuffer(&mut data, &mut frames, &mut flags, None, None)
                        .map_err(|e| format!("读取音频失败: {e}"))?;
                    let mut samples =
                        if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 || data.is_null() {
                            vec![0; frames as usize]
                        } else {
                            slice::from_raw_parts(data.cast::<i16>(), frames as usize).to_vec()
                        };
                    let _ = capture.ReleaseBuffer(frames);
                    if !samples.is_empty() {
                        max_level = max_level.max(gain.process(&mut samples));
                        let _ = pcm.send(samples);
                    }
                    available = capture
                        .GetNextPacketSize()
                        .map_err(|e| format!("取下一包失败: {e}"))?;
                }
                Ok(())
            };
            if let Err(error) = packet() {
                let _ = client.Stop();
                return Err(error);
            }
            if max_level > 0.0 {
                let _ = level.send(max_level);
            }
            if stop.load(Ordering::Relaxed) {
                break;
            }
            thread::sleep(POLL_INTERVAL);
        }
        let _ = client.Stop();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::WAVE_FORMAT_PCM;
    use crate::dispatch::SAMPLE_RATE;

    #[test]
    fn pcm_format_is_16k_mono_16bit() {
        // 给识别方的格式常量：字节率 = 采样率 × 声道 × 位深/8 = 16000 × 1 × 2。
        assert_eq!(WAVE_FORMAT_PCM as i32, 1);
        assert_eq!(SAMPLE_RATE * 2, 32_000);
    }
}
