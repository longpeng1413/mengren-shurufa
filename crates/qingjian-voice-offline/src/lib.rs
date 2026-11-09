//! 本机离线语音识别：sherpa-onnx（Apache-2.0）跑 SenseVoice Small int8 模型，实现 Core 的 [`qingjian_core::Transcriber`]。
//! 不联网、不收费；引擎库运行时加载（`library` 模块），模型首次使用时下载到本机（[`SpeechModel`]）。
//! 与云端识别（`qingjian-voice`）分开成 crate：这里要调 C 接口，那边禁止 unsafe。设计见 `docs/design/voice-and-swipe.md`。

mod config;
mod error;
mod library;
mod model;
mod recognizer;
mod shared;
mod transcriber;

pub use config::OfflineConfig;
pub use error::OfflineError;
pub use model::{DownloadProgress, ModelFile, SpeechModel};
pub use transcriber::OfflineTranscriber;

use std::time::{Duration, Instant};

/// 删除模型、换模型前放掉进程里加载着的识别器。
pub fn release_shared() {
    shared::release();
}

/// 现在内存里有没有加载着模型。
pub fn model_loaded() -> bool {
    shared::loaded()
}

/// 设置页「测试识别」：加载模型（已加载就直接用）并识别半秒静音，阻塞到完成，回用时。
pub fn test_recognizer(config: &OfflineConfig) -> Result<Duration, OfflineError> {
    let started = Instant::now();
    let recognizer = shared::acquire(config)?;
    recognizer.decode(
        recognizer::SAMPLE_RATE,
        &[0.0; recognizer::SAMPLE_RATE as usize / 2],
    )?;
    drop(recognizer);
    shared::touch();
    Ok(started.elapsed())
}

/// 识别一整段录音（单声道 16 位 PCM），阻塞到完成；命令行验证与桌面端用。
pub fn transcribe(
    config: &OfflineConfig,
    sample_rate: u32,
    samples: &[i16],
) -> Result<String, OfflineError> {
    let recognizer = shared::acquire(config)?;
    let floats: Vec<f32> = samples
        .iter()
        .map(|&sample| f32::from(sample) / 32_768.0)
        .collect();
    let text = recognizer.decode(sample_rate, &floats);
    drop(recognizer);
    shared::touch();
    text
}

/// 引擎库报的版本（诊断用）。
pub fn library_version(config: &OfflineConfig) -> Result<String, OfflineError> {
    library::api(config.library_dir.as_deref()).map(|api| api.version.clone())
}
