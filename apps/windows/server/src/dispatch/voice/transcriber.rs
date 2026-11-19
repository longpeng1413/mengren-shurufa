//! 按 `[voice]` 配置装配识别方：本机离线优先，豆包云端备用，没配 / 没密钥就是 `NoTranscriber`。
//! 与安卓 `apps/android/core/src/voice/settings.rs` 同一套选择逻辑，只是配置来自 TOML 而不是 JNI JSON。

use std::path::{Path, PathBuf};

use qingjian_core::{NoTranscriber, Transcriber};
use qingjian_platform::VoiceConfig;
use qingjian_voice::{VolcengineConfig, VolcengineTranscriber};
use qingjian_voice_offline::{OfflineConfig, OfflineTranscriber, SpeechModel};

/// 离线引擎库（随 Server exe 同目录）；库名按平台由 qingjian-voice-offline 内部决定。
/// 传目录而不是文件，让它按 `sherpa-onnx-c-api.dll` / `.so` / `.dylib` 找。
pub fn build(
    config: &VoiceConfig,
    user_dir: Option<&Path>,
    exe_dir: Option<&Path>,
) -> Box<dyn Transcriber> {
    if config.is_offline() {
        let root = model_root(config, user_dir);
        let library = exe_dir.map(Path::to_path_buf);
        tracing::info!(root = %root.display(), "语音识别：本机离线（SenseVoice）");
        return Box::new(OfflineTranscriber::new(OfflineConfig::new(
            SpeechModel::new(root),
            library,
        )));
    }
    if config.is_volcengine() {
        if let Some(key) = config.resolve_api_key() {
            tracing::info!(resource = %config.resource_id, "语音识别：豆包云端");
            return Box::new(VolcengineTranscriber::new(VolcengineConfig::new(
                key,
                &config.resource_id,
            )));
        }
        tracing::warn!("语音识别选了豆包但没填 API Key，入口不可用");
    }
    Box::new(NoTranscriber)
}

/// 模型根目录：配置没写就用 `<用户目录>/voice-model`（与安卓 filesDir/voice-model 对齐）；
/// 没有用户目录（测试）退回当前目录下的相对路径。
fn model_root(config: &VoiceConfig, user_dir: Option<&Path>) -> PathBuf {
    if !config.model_root.trim().is_empty() {
        return PathBuf::from(config.model_root.trim());
    }
    user_dir
        .map(|dir| dir.join("voice-model"))
        .unwrap_or_else(|| PathBuf::from("voice-model"))
}

#[cfg(test)]
mod tests {
    use super::{build, model_root};
    use qingjian_platform::VoiceConfig;
    use std::path::Path;

    #[test]
    fn providers_pick_a_transcriber_and_availability() {
        let none = VoiceConfig::default();
        assert!(!build(&none, None, None).available());

        let volc_no_key = VoiceConfig {
            provider: qingjian_platform::PROVIDER_VOLCENGINE.to_owned(),
            api_key: None,
            api_key_env: "MENGREN_TEST_VOICE_KEY_UNSET".to_owned(),
            ..VoiceConfig::default()
        };
        assert!(!build(&volc_no_key, None, None).available());
    }

    #[test]
    fn model_root_defaults_to_user_dir() {
        let config = VoiceConfig::default();
        assert_eq!(
            model_root(&config, Some(Path::new("/data/Qingjian"))),
            Path::new("/data/Qingjian/voice-model")
        );
        let custom = VoiceConfig {
            model_root: "D:/models/asr".to_owned(),
            ..VoiceConfig::default()
        };
        assert_eq!(model_root(&custom, None), Path::new("D:/models/asr"));
    }
}
