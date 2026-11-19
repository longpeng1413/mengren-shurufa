//! 语音识别配置（`[voice]` 分节，各平台共用的一份）。
//!
//! 与 `[predict]`（云联想、翻译）分开：语音识别是不同的服务、不同的密钥，用户可能只开一样。
//! 数据源按优先级：offline 本机离线优先，volcengine 豆包云端备用，none 不启用。
//! 密钥不派生进日志：诊断输出走 `logs::secrets::MaskingWriter`，与云联想密钥同一套遮蔽。

use serde::{Deserialize, Serialize};

use super::KeyCombo;

/// 没启用语音识别。
pub const PROVIDER_NONE: &str = "none";
/// 本机离线识别（sherpa-onnx + SenseVoice），不联网、不收费。
pub const PROVIDER_OFFLINE: &str = "offline";
/// 豆包流式语音识别（火山引擎），走云端。
pub const PROVIDER_VOLCENGINE: &str = "volcengine";

/// 豆包按时长计费的资源 ID（缺省）。
pub const DEFAULT_RESOURCE_ID: &str = "volc.seedasr.sauc.duration";

/// `[voice]`：语音识别的服务、密钥、触发方式。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct VoiceConfig {
    /// 识别服务：`none` 关闭 / `offline` 本机离线 / `volcengine` 豆包云端；写错按 `none`。
    pub provider: String,

    /// 豆包的资源 ID（计费方式）：小时版 `volc.seedasr.sauc.duration` / 并发版 `.concurrent` / 文件版 `volc.seedasr.auc`。
    pub resource_id: String,

    /// 豆包 API Key。可留空并设置 `api_key_env` 指定的环境变量；设置页填的写进配置同目录 `.env`。
    pub api_key: Option<String>,

    /// 存放密钥的环境变量名。
    pub api_key_env: String,

    /// 长按 Win 键触发语音的时长（毫秒）；未设自定义快捷键时生效。缺省 400。
    pub hold_ms: u64,

    /// 自定义语音快捷键（修饰键 + 字母/数字/句号）。设了就用它替代长按 Win：按下开始、松开结束。
    /// 留空表示用长按 Win。AppContainer 里读不到配置时为 `None`（那时只走长按 Win，由 Server 下发）。
    pub shortcut: Option<KeyCombo>,

    /// 离线模型的根目录（相对用户数据目录）；空串用默认的 `<用户目录>/voice-model`。
    /// 一般不用手填，留给了测试与自定义布局。
    pub model_root: String,
}

impl Default for VoiceConfig {
    fn default() -> Self {
        Self {
            provider: PROVIDER_NONE.to_owned(),
            resource_id: DEFAULT_RESOURCE_ID.to_owned(),
            api_key: None,
            api_key_env: "QINGJIAN_VOICE_API_KEY".to_owned(),
            hold_ms: 400,
            shortcut: None,
            model_root: String::new(),
        }
    }
}

impl VoiceConfig {
    /// 服务开没开：`offline` / `volcengine` 之外的都当关。
    pub fn enabled(&self) -> bool {
        matches!(
            self.provider.as_str(),
            PROVIDER_OFFLINE | PROVIDER_VOLCENGINE
        )
    }

    /// 是不是本机离线。
    pub fn is_offline(&self) -> bool {
        self.provider == PROVIDER_OFFLINE
    }

    /// 是不是豆包云端。
    pub fn is_volcengine(&self) -> bool {
        self.provider == PROVIDER_VOLCENGINE
    }

    /// 密钥：配置里的优先，其次环境变量；都没配返回 `None`。
    pub fn resolve_api_key(&self) -> Option<String> {
        self.api_key
            .as_deref()
            .map(str::trim)
            .filter(|k| !k.is_empty())
            .map(str::to_owned)
            .or_else(|| std::env::var(&self.api_key_env).ok())
            .filter(|k| !k.trim().is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::{PROVIDER_NONE, PROVIDER_OFFLINE, PROVIDER_VOLCENGINE, VoiceConfig};

    #[test]
    fn default_is_off_and_parses_all_providers() {
        let config = VoiceConfig::default();
        assert_eq!(config.provider, PROVIDER_NONE);
        assert!(!config.enabled());
        assert_eq!(config.hold_ms, 400);
        assert_eq!(
            serde_json::from_str::<VoiceConfig>(&serde_json::to_string(&config).unwrap()).unwrap(),
            config
        );
        for provider in [PROVIDER_OFFLINE, PROVIDER_VOLCENGINE] {
            let json = format!(r#"{{"provider":"{provider}"}}"#);
            let parsed: VoiceConfig = serde_json::from_str(&json).unwrap();
            assert!(parsed.enabled());
        }
        let unknown: VoiceConfig =
            serde_json::from_str(r#"{"provider":"something-else"}"#).unwrap();
        assert!(!unknown.enabled());
    }

    #[test]
    fn api_key_prefers_config_then_env() {
        // 用一个进程里独有的变量名，避免与别处并发测试互相踩踏；读不到也不影响断言配置优先。
        let config = VoiceConfig {
            api_key: None,
            api_key_env: "MENGREN_TEST_VOICE_KEY_NEVER_SET".to_owned(),
            ..VoiceConfig::default()
        };
        assert_eq!(config.resolve_api_key(), None);
        let config = VoiceConfig {
            api_key: Some("  in-config  ".to_owned()),
            ..config
        };
        assert_eq!(config.resolve_api_key().as_deref(), Some("in-config"));
        let blank = VoiceConfig {
            api_key: Some("   ".to_owned()),
            api_key_env: "MENGREN_TEST_VOICE_KEY_NEVER_SET".to_owned(),
            ..VoiceConfig::default()
        };
        assert_eq!(blank.resolve_api_key(), None);
    }
}
