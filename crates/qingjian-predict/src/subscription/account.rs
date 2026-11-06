//! 验证后的会员账户；序列化结果必须由壳加密保存，禁止写日志。

use serde::{Deserialize, Serialize};

use super::auth::{AuthError, Authorization, Callback, verify_identity};

#[derive(Serialize, Deserialize)]
pub struct Account {
    pub client_id: String,
    pub subject: String,
    pub email: Option<String>,
    pub access_token: String,
    pub refresh_token: String,
    pub id_token: String,
    pub expires_at: u64,
    pub scope: String,
}

impl Account {
    /// 提前续期，调用方必须原子保存成功响应里的整套令牌。
    pub async fn refresh(&mut self, client: &reqwest::Client) -> Result<(), AuthError> {
        let mut form = reqwest::Url::parse("https://example.invalid/").expect("valid URL");
        form.query_pairs_mut()
            .append_pair("grant_type", "refresh_token")
            .append_pair("client_id", &self.client_id)
            .append_pair("refresh_token", &self.refresh_token)
            .append_pair("resource", "https://api.openai.com/v1");
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
        let tokens: super::auth::TokenResponse =
            response.json().await.map_err(|_| AuthError::Exchange)?;
        if !tokens.token_type.eq_ignore_ascii_case("bearer")
            || !tokens
                .scope
                .split_whitespace()
                .any(|scope| scope == "chatgpt.tokens.use.direct")
            || tokens.access_token.is_empty()
            || tokens.refresh_token.is_empty()
        {
            return Err(AuthError::InvalidIdentity);
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| AuthError::InvalidIdentity)?
            .as_secs();
        self.access_token = tokens.access_token;
        self.refresh_token = tokens.refresh_token;
        self.id_token = tokens.id_token;
        self.scope = tokens.scope;
        self.expires_at = now.saturating_add(tokens.expires_in);
        Ok(())
    }

    /// 只有身份签名及会员授权检查通过，才构造可保存的账户。
    pub async fn sign_in(
        client: &reqwest::Client,
        authorization: &Authorization,
        callback: &Callback,
    ) -> Result<Self, AuthError> {
        let tokens = authorization.exchange(client, callback).await?;
        let identity =
            verify_identity(client, &tokens, &callback.client_id, authorization.nonce()).await?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| AuthError::InvalidIdentity)?
            .as_secs();
        Ok(Self {
            client_id: callback.client_id.clone(),
            subject: identity.subject,
            email: identity.email,
            access_token: tokens.access_token,
            refresh_token: tokens.refresh_token,
            id_token: tokens.id_token,
            expires_at: now.saturating_add(tokens.expires_in),
            scope: tokens.scope,
        })
    }
}
