//! ChatGPT 会员请求的 Responses API 消息与事件解析。
//!
//! 此模块只负责请求格式和事件解析；登录、令牌存储及客户端接入由平台层提供。

use serde_json::{Value, json};
use thiserror::Error;

mod account;
pub mod auth;
pub use account::Account;

/// 平台负责加密存取账户，网络层只临时取得明文。
pub trait CredentialStore: Send + Sync {
    fn lock(&self) -> Result<Box<dyn Send>, String> {
        Ok(Box::new(()))
    }
    fn load(&self) -> Result<Account, String>;
    fn save(&self, account: &Account) -> Result<(), String>;
}

static STORE: std::sync::OnceLock<std::sync::Arc<dyn CredentialStore>> = std::sync::OnceLock::new();
static REFRESH: std::sync::LazyLock<std::sync::Mutex<()>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(()));

pub fn install_store(store: std::sync::Arc<dyn CredentialStore>) {
    let _ = STORE.set(store);
}

/// 串行刷新令牌并保存轮换结果，避免同一进程同时使用旧 refresh token。
pub async fn access_token(client: &reqwest::Client) -> Result<String, SubscriptionError> {
    let store = STORE.get().ok_or(SubscriptionError::NoAccount)?;
    let store = store.clone();
    let client = client.clone();
    tokio::task::spawn_blocking(move || {
        let _guard = REFRESH.lock().map_err(|_| SubscriptionError::NoAccount)?;
        let _platform_guard = store.lock().map_err(|_| SubscriptionError::NoAccount)?;
        let mut account = store.load().map_err(|_| SubscriptionError::NoAccount)?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| SubscriptionError::NoAccount)?
            .as_secs();
        if account.expires_at <= now.saturating_add(180) {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|_| SubscriptionError::NoAccount)?;
            runtime
                .block_on(account.refresh(&client))
                .map_err(|_| SubscriptionError::NoAccount)?;
            store
                .save(&account)
                .map_err(|_| SubscriptionError::NoAccount)?;
        }
        Ok(account.access_token)
    })
    .await
    .map_err(|_| SubscriptionError::NoAccount)?
}

const MAX_STREAM_BYTES: usize = 2 * 1024 * 1024;

/// 登录和会员请求共用的 HTTPS 客户端；安卓先安装 rustls 加密实现。
pub fn client() -> Result<reqwest::Client, reqwest::Error> {
    client_with_proxy("")
}

pub fn client_with_proxy(proxy: &str) -> Result<reqwest::Client, reqwest::Error> {
    crate::chat_client::install_crypto_provider();
    let mut builder = crate::chat_client::client_builder()
        .timeout(std::time::Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::none());
    if !proxy.trim().is_empty() {
        builder = builder.proxy(reqwest::Proxy::all(proxy.trim())?);
    }
    builder.build()
}

/// 会员请求失败，错误不包含令牌和请求内容。
#[derive(Debug, Error)]
pub enum SubscriptionError {
    #[error("ChatGPT account is missing or needs sign-in")]
    NoAccount,
    #[error("subscription request failed: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("subscription request returned HTTP {0}")]
    Http(reqwest::StatusCode),
    #[error("invalid subscription event")]
    InvalidEvent,
    #[error("subscription response failed: {0}")]
    ResponseFailed(String),
    #[error("subscription stream ended without completion")]
    Incomplete,
    #[error("subscription stream is too large")]
    TooLarge,
}

/// 使用调用方提供的 HTTP 客户端发送会员请求，只有收到完成事件才返回结果。
pub async fn complete(
    client: &reqwest::Client,
    endpoint: &str,
    token: &str,
    model: &str,
    instructions: &str,
    input: &str,
) -> Result<String, SubscriptionError> {
    let mut response = client
        .post(endpoint)
        .bearer_auth(token)
        .json(&request(model, instructions, input))
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(SubscriptionError::Http(response.status()));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if bytes.len().saturating_add(chunk.len()) > MAX_STREAM_BYTES {
            return Err(SubscriptionError::TooLarge);
        }
        bytes.extend_from_slice(&chunk);
    }
    parse_stream(&bytes)
}

fn parse_stream(bytes: &[u8]) -> Result<String, SubscriptionError> {
    let text = std::str::from_utf8(bytes).map_err(|_| SubscriptionError::InvalidEvent)?;
    let mut output = String::new();
    let mut completed = false;
    let mut data = String::new();
    for line in text.lines().chain(std::iter::once("")) {
        if line.is_empty() {
            if data.is_empty() {
                continue;
            }
            let event: Value =
                serde_json::from_str(&data).map_err(|_| SubscriptionError::InvalidEvent)?;
            match event["type"].as_str() {
                Some("response.output_text.delta") => {
                    output.push_str(
                        event["delta"]
                            .as_str()
                            .ok_or(SubscriptionError::InvalidEvent)?,
                    );
                }
                Some("response.completed") => completed = true,
                Some("response.failed" | "response.incomplete" | "error") => {
                    let code = event
                        .pointer("/response/error/code")
                        .or_else(|| event.pointer("/error/code"))
                        .and_then(Value::as_str)
                        .unwrap_or("unknown_error");
                    return Err(SubscriptionError::ResponseFailed(code.to_owned()));
                }
                _ => {}
            }
            data.clear();
        } else if let Some(value) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(value.strip_prefix(' ').unwrap_or(value));
        }
    }
    if completed {
        Ok(output)
    } else {
        Err(SubscriptionError::Incomplete)
    }
}

/// 构造会员接口要求的无状态流式请求。
pub fn request(model: &str, instructions: &str, input: &str) -> Value {
    json!({
        "model": model,
        "instructions": instructions,
        "input": [{"type": "message", "role": "user", "content": input}],
        "store": false,
        "stream": true
    })
}

/// 从一条 Responses SSE `data` 内容中提取增量文字。
///
/// 返回 `None` 表示事件不含文本；无效 JSON 返回解析错误。
pub fn text_delta(data: &str) -> Result<Option<String>, serde_json::Error> {
    let event: Value = serde_json::from_str(data)?;
    Ok((event["type"] == "response.output_text.delta")
        .then(|| event["delta"].as_str().map(str::to_owned))
        .flatten())
}

#[cfg(test)]
mod tests {
    use super::{SubscriptionError, parse_stream, request, text_delta};

    #[test]
    fn subscription_request_is_streaming_and_stateless() {
        let body = request("model", "translate", "你好");
        assert_eq!(body["stream"], true);
        assert_eq!(body["store"], false);
        assert_eq!(body["instructions"], "translate");
        assert_eq!(body["input"][0]["role"], "user");
        assert_eq!(body["input"][0]["content"], "你好");
        for field in ["temperature", "max_output_tokens", "background"] {
            assert!(body.get(field).is_none());
        }
    }

    #[test]
    fn extracts_only_text_deltas() {
        assert_eq!(
            text_delta(r#"{"type":"response.output_text.delta","delta":"你好"}"#).unwrap(),
            Some("你好".to_owned())
        );
        assert_eq!(
            text_delta(r#"{"type":"response.completed"}"#).unwrap(),
            None
        );
        assert!(text_delta("not json").is_err());
    }

    #[test]
    fn requires_completion_and_preserves_failure_code() {
        let delta = "data: {\"type\":\"response.output_text.delta\",\"delta\":\"你好\"}\r\n\r\n";
        assert!(matches!(
            parse_stream(delta.as_bytes()),
            Err(SubscriptionError::Incomplete)
        ));
        let success = format!("{delta}data: {{\"type\":\"response.completed\"}}\n\n");
        assert_eq!(parse_stream(success.as_bytes()).unwrap(), "你好");
        let fail = format!(
            "{delta}data: {{\"type\":\"response.failed\",\"response\":{{\"error\":{{\"code\":\"subscription_sharing_usage_limit_exceeded\"}}}}}}\n\n"
        );
        assert!(
            matches!(parse_stream(fail.as_bytes()), Err(SubscriptionError::ResponseFailed(code)) if code == "subscription_sharing_usage_limit_exceeded")
        );
    }
}
