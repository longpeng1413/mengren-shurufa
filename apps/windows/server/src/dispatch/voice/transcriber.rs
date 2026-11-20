//! 按 `[voice]` 配置装配识别方：本机离线优先，豆包云端备用，没配 / 没密钥就是 `NoTranscriber`。
//! 与安卓 `apps/android/core/src/voice/settings.rs` 同一套选择逻辑，只是配置来自 TOML 而不是 JNI JSON。

use std::path::{Path, PathBuf};

use qingjian_core::{NoTranscriber, Transcriber};
use qingjian_platform::VoiceConfig;
use qingjian_platform::windows_text::tr;
use qingjian_voice::{VolcengineConfig, VolcengineTranscriber};
use qingjian_voice_offline::{OfflineConfig, OfflineError, OfflineTranscriber, SpeechModel};

/// 离线引擎库（随 Server exe 同目录）；库名按平台由 qingjian-voice-offline 内部决定。
/// 传目录而不是文件，让它按 `sherpa-onnx-c-api.dll` / `.so` / `.dylib` 找。
pub fn build(
    config: &VoiceConfig,
    user_dir: Option<&Path>,
    exe_dir: Option<&Path>,
) -> Box<dyn Transcriber> {
    if config.is_offline() {
        let offline = offline_config(config, user_dir, exe_dir);
        tracing::info!(root = %offline.model.root().display(), "语音识别：本机离线（SenseVoice）");
        return Box::new(OfflineTranscriber::new(offline));
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

/// 识别方用不了时给用户看的原因：选了服务却缺模型 / 引擎 / 密钥时说清缺什么，都没选才说「未开启」。
pub fn unavailable_reason(
    config: &VoiceConfig,
    user_dir: Option<&Path>,
    exe_dir: Option<&Path>,
) -> String {
    if config.is_offline() {
        match offline_config(config, user_dir, exe_dir).problem() {
            Some(OfflineError::ModelMissing) => return tr("voice_model").to_owned(),
            Some(error @ OfflineError::Library(_)) => {
                tracing::warn!(%error, "offline speech engine unavailable");
                return tr("voice_engine").to_owned();
            }
            Some(error) => {
                tracing::warn!(%error, "offline speech unavailable");
                return error.describe();
            }
            None => {}
        }
    }
    if config.is_volcengine() && config.resolve_api_key().is_none() {
        return tr("voice_key").to_owned();
    }
    tr("off").to_owned()
}

fn offline_config(
    config: &VoiceConfig,
    user_dir: Option<&Path>,
    exe_dir: Option<&Path>,
) -> OfflineConfig {
    OfflineConfig::new(
        SpeechModel::new(model_root(config, user_dir)),
        exe_dir.map(Path::to_path_buf),
    )
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
    use super::{build, model_root, unavailable_reason};
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
        assert_eq!(
            unavailable_reason(&volc_no_key, None, None),
            qingjian_platform::windows_text::tr("voice_key")
        );
        assert_eq!(
            unavailable_reason(&none, None, None),
            qingjian_platform::windows_text::tr("off")
        );
        let offline = VoiceConfig {
            provider: qingjian_platform::PROVIDER_OFFLINE.to_owned(),
            model_root: std::env::temp_dir()
                .join("mengren-test-missing-voice-model")
                .display()
                .to_string(),
            ..VoiceConfig::default()
        };
        assert_eq!(
            unavailable_reason(&offline, None, None),
            qingjian_platform::windows_text::tr("voice_model")
        );
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
