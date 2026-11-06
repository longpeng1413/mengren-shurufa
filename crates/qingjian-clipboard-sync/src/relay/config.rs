//! 中转连接参数。令牌只在运行时由平台的加密存储注入。

use serde::{Deserialize, Serialize};

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RelayConfig {
    pub enabled: bool,

    pub url: String,

    #[serde(skip)]
    pub token: String,
}

impl std::fmt::Debug for RelayConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RelayConfig")
            .field("enabled", &self.enabled)
            .field("url", &self.url)
            .field("token", &"[redacted]")
            .finish()
    }
}

impl RelayConfig {
    pub fn validate(&self) -> Result<(), crate::SyncError> {
        let uri: tungstenite::http::Uri = self.url.parse().map_err(|_| super::invalid())?;
        let host = uri.host().ok_or_else(super::invalid)?;
        let local = matches!(host, "127.0.0.1" | "[::1]" | "::1");
        if !(uri.scheme_str() == Some("wss") || local && uri.scheme_str() == Some("ws"))
            || uri.query().is_some()
            || uri
                .authority()
                .is_some_and(|authority| authority.as_str().contains('@'))
            || uri.path() != "/v1/clipboard"
            || !(24..=256).contains(&self.token.len())
            || !self.token.bytes().all(|byte| byte.is_ascii_graphic())
        {
            return Err(super::invalid());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::RelayConfig;

    #[test]
    fn only_secure_public_endpoints_and_no_secret_serialization() {
        let mut config = RelayConfig {
            enabled: true,
            url: "wss://relay.example.com/v1/clipboard".into(),
            token: "example-test-token-123456789".into(),
        };
        assert!(config.validate().is_ok());
        assert!(
            !serde_json::to_string(&config)
                .unwrap()
                .contains(&config.token)
        );
        assert!(!format!("{config:?}").contains(&config.token));
        for url in [
            "ws://example.com/v1/clipboard",
            "wss://user:secret@example.com/v1/clipboard",
            "wss://example.com/v1/clipboard?token=x",
            "wss://example.com/wrong",
        ] {
            config.url = url.into();
            assert!(config.validate().is_err(), "{url}");
        }
        config.url = "ws://127.0.0.1:1234/v1/clipboard".into();
        assert!(config.validate().is_ok());
    }
}
