//! ChatGPT 会员登录的一次授权尝试，校验回调与 PKCE 参数。

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

mod identity;
mod listener;
pub use identity::{Identity, verify_identity};
pub use listener::LoginListener;

const AUTH_URL: &str = "https://auth.openai.com/api/accounts/authorize";
const RESOURCE: &str = "https://api.openai.com/v1";
const SCOPE: &str = "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";

/// 一次登录尝试的临时秘密，不应写入日志或保存。
pub struct Authorization {
    state: String,
    nonce: String,
    verifier: String,
    redirect: String,
    host_id: String,
}

/// 验证后可用于换取令牌的回调参数。
pub struct Callback {
    pub code: String,
    pub client_id: String,
}

/// 令牌交换响应；不得记录到日志。使用前还必须验证 ID token 和授权范围。
#[derive(serde::Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub id_token: String,
    pub token_type: String,
    pub expires_in: u64,
    pub scope: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AuthError {
    #[error("invalid loopback callback")]
    InvalidRedirect,
    #[error("invalid authorization callback")]
    InvalidCallback,
    #[error("authorization was denied")]
    Denied,
    #[error("token exchange failed")]
    Exchange,
    #[error("identity validation failed")]
    InvalidIdentity,
    #[error("authorization timed out")]
    Timeout,
}

impl Authorization {
    /// 只接受 127.0.0.1 的 HTTP 回调，监听器须在打开浏览器前启动。
    pub fn new(port: u16, host_id: String) -> Result<Self, AuthError> {
        if port == 0 || host_id.trim().is_empty() {
            return Err(AuthError::InvalidRedirect);
        }
        Ok(Self {
            state: Uuid::new_v4().to_string(),
            nonce: Uuid::new_v4().to_string(),
            verifier: format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple()),
            redirect: format!("http://127.0.0.1:{port}/auth/callback"),
            host_id,
        })
    }

    pub fn url(&self) -> reqwest::Url {
        let mut url = reqwest::Url::parse(AUTH_URL).expect("valid authorization URL");
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(self.verifier.as_bytes()));
        url.query_pairs_mut()
            .append_pair("client_id", "dynamic_agent_client")
            .append_pair("agent_name_hint", "猛人输入法")
            .append_pair("ext_agent_host_id", &self.host_id)
            .append_pair("response_type", "code")
            .append_pair("redirect_uri", &self.redirect)
            .append_pair("scope", SCOPE)
            .append_pair("resource", RESOURCE)
            .append_pair("state", &self.state)
            .append_pair("nonce", &self.nonce)
            .append_pair("code_challenge_method", "S256")
            .append_pair("code_challenge", &challenge);
        url
    }

    pub fn nonce(&self) -> &str {
        &self.nonce
    }

    pub fn verifier(&self) -> &str {
        &self.verifier
    }

    pub fn redirect(&self) -> &str {
        &self.redirect
    }

    /// 换取令牌。结果尚未通过 ID token 签名、nonce 和权限验证，不能直接使用。
    pub async fn exchange(
        &self,
        client: &reqwest::Client,
        callback: &Callback,
    ) -> Result<TokenResponse, AuthError> {
        let mut form = reqwest::Url::parse("https://example.invalid/").expect("valid URL");
        form.query_pairs_mut()
            .append_pair("grant_type", "authorization_code")
            .append_pair("client_id", &callback.client_id)
            .append_pair("code", &callback.code)
            .append_pair("code_verifier", &self.verifier)
            .append_pair("redirect_uri", &self.redirect)
            .append_pair("resource", RESOURCE);
        let response = client
            .post("https://auth.openai.com/api/accounts/oauth/token")
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .body(form.query().unwrap_or_default().to_owned())
            .send()
            .await
            .map_err(|_| AuthError::Exchange)?;
        if !response.status().is_success() {
            return Err(AuthError::Exchange);
        }
        response.json().await.map_err(|_| AuthError::Exchange)
    }

    /// 只接受同一次尝试的回调；拒绝重复参数及未发放的客户端 ID。
    pub fn callback(&self, value: &str) -> Result<Callback, AuthError> {
        let url = reqwest::Url::parse(value).map_err(|_| AuthError::InvalidCallback)?;
        let expected =
            reqwest::Url::parse(&self.redirect).map_err(|_| AuthError::InvalidRedirect)?;
        if url.scheme() != expected.scheme()
            || url.host_str() != expected.host_str()
            || url.port_or_known_default() != expected.port_or_known_default()
            || url.path() != expected.path()
            || url.fragment().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(AuthError::InvalidCallback);
        }
        let mut state = None;
        let mut code = None;
        let mut client_id = None;
        let mut error = None;
        for (key, value) in url.query_pairs() {
            let slot = match key.as_ref() {
                "state" => &mut state,
                "code" => &mut code,
                "client_id" => &mut client_id,
                "error" => &mut error,
                _ => continue,
            };
            if slot.replace(value.into_owned()).is_some() {
                return Err(AuthError::InvalidCallback);
            }
        }
        if state.as_deref() != Some(self.state.as_str()) {
            return Err(AuthError::InvalidCallback);
        }
        if error.is_some() {
            return Err(AuthError::Denied);
        }
        match (code, client_id) {
            (Some(code), Some(client_id))
                if !code.is_empty() && client_id.starts_with("oaiapp_") =>
            {
                Ok(Callback { code, client_id })
            }
            _ => Err(AuthError::InvalidCallback),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AuthError, Authorization};

    #[test]
    fn builds_pkce_request_and_rejects_forged_callbacks() {
        let auth = Authorization::new(1455, "urn:uuid:test".into()).unwrap();
        let url = auth.url();
        let pairs: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(pairs["client_id"], "dynamic_agent_client");
        assert_eq!(pairs["code_challenge_method"], "S256");
        assert!(pairs["scope"].contains("chatgpt.tokens.use.direct"));
        let valid = format!(
            "{}?state={}&code=abc&client_id=oaiapp_test",
            auth.redirect(),
            pairs["state"]
        );
        assert!(auth.callback(&valid).is_ok());
        assert!(matches!(
            auth.callback(&valid.replace(&pairs["state"], "wrong")),
            Err(AuthError::InvalidCallback)
        ));
        assert!(matches!(
            auth.callback(&format!("{valid}&code=other")),
            Err(AuthError::InvalidCallback)
        ));
        assert!(matches!(
            auth.callback(&valid.replace("127.0.0.1", "localhost")),
            Err(AuthError::InvalidCallback)
        ));
    }
}
