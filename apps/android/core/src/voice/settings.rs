//! 壳交过来的语音识别配置（JSON，含解好的密钥），按它挑识别方。

use std::path::PathBuf;

use qingjian_core::{NoTranscriber, Transcriber};
use qingjian_voice::{VolcengineConfig, VolcengineTranscriber};
use qingjian_voice_offline::{OfflineConfig, OfflineTranscriber, SpeechModel};
use serde::Deserialize;

/// 设置页「语音识别」里填的（Kotlin 解出密钥后整份交过来）。不派生 `Debug`，免得密钥被打进日志。
#[derive(Deserialize)]
pub struct VoiceSettings {
    /// 识别服务：`volcengine`（豆包，云端）或 `offline`（本机离线）；别的值当没选。
    pub provider: String,

    #[serde(default)]
    pub api_key: String,

    /// 火山引擎的资源 ID（计费方式）。
    #[serde(default)]
    pub resource_id: String,

    /// 离线模型的根目录（App 私有目录下），模型在它下面按版本分文件夹。
    #[serde(default)]
    pub model_root: String,

    /// App 的原生库目录；离线引擎按名字找不到时按这个目录找。
    #[serde(default)]
    pub library_dir: String,
}

impl VoiceSettings {
    pub fn volcengine(&self) -> Option<VolcengineConfig> {
        if self.provider != PROVIDER_VOLCENGINE {
            return None;
        }
        let resource = if self.resource_id.trim().is_empty() {
            VolcengineConfig::HOURLY
        } else {
            self.resource_id.as_str()
        };
        Some(VolcengineConfig::new(self.api_key.as_str(), resource))
    }

    /// 选了本机离线且给了模型目录。
    pub fn offline(&self) -> Option<OfflineConfig> {
        if self.provider != PROVIDER_OFFLINE || self.model_root.trim().is_empty() {
            return None;
        }
        let library =
            Some(PathBuf::from(&self.library_dir)).filter(|_| !self.library_dir.is_empty());
        Some(OfflineConfig::new(
            SpeechModel::new(&self.model_root),
            library,
        ))
    }

    /// 这份配置对应的识别方；没选服务是 `NoTranscriber`（没填密钥、没下模型的 `available()` 为假，入口照样提示还不能用）。
    pub fn transcriber(&self) -> Box<dyn Transcriber> {
        if let Some(config) = self.offline() {
            return Box::new(OfflineTranscriber::new(config));
        }
        match self.volcengine() {
            Some(config) => Box::new(VolcengineTranscriber::new(config)),
            None => Box::new(NoTranscriber),
        }
    }
}

const PROVIDER_VOLCENGINE: &str = "volcengine";

const PROVIDER_OFFLINE: &str = "offline";

#[cfg(test)]
mod tests {
    use super::VoiceSettings;

    fn parse(json: &str) -> VoiceSettings {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn offline_needs_model_root_and_downloaded_model() {
        let offline = parse(
            r#"{"provider":"offline","model_root":"/nonexistent/voice-model","library_dir":""}"#,
        );
        let config = offline.offline().expect("offline selected");
        assert!(config.library_dir.is_none());
        assert!(offline.volcengine().is_none());
        assert!(!offline.transcriber().available(), "model not downloaded");
        assert!(parse(r#"{"provider":"offline"}"#).offline().is_none());
        let cloud = parse(r#"{"provider":"volcengine","api_key":"k"}"#);
        assert!(cloud.offline().is_none());
        assert!(cloud.volcengine().is_some());
    }
}
