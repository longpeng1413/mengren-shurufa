//! 有道智云文本翻译 API v3，仅用于显式翻译请求。

use serde_json::Value;
use sha2::{Digest, Sha256};

static SECRET: std::sync::OnceLock<fn() -> Option<String>> = std::sync::OnceLock::new();

pub fn install_secret_loader(loader: fn() -> Option<String>) {
    let _ = SECRET.set(loader);
}

pub fn load_secret() -> Option<String> {
    SECRET.get().and_then(|load| load())
}

/// 按 Unicode 字符截取，不能按 UTF-8 字节或 UTF-16 单元计数。
pub fn signing_input(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= 20 {
        return text.to_owned();
    }
    format!(
        "{}{}{}",
        chars[..10].iter().collect::<String>(),
        chars.len(),
        chars[chars.len() - 10..].iter().collect::<String>()
    )
}

pub fn signature(app_key: &str, secret: &str, text: &str, salt: &str, time: &str) -> String {
    let input = format!("{app_key}{}{salt}{time}{secret}", signing_input(text));
    Sha256::digest(input.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// 与猛人桌面助手一致，有道默认直连，避免国内服务绕行海外代理。
pub fn client() -> Result<reqwest::Client, reqwest::Error> {
    crate::chat_client::install_crypto_provider();
    crate::chat_client::client_builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .build()
}

/// 固定 HTTPS 地址，错误只保留错误码，不包含原文、签名或密钥。
pub async fn translate(
    client: &reqwest::Client,
    app_key: &str,
    secret: &str,
    text: &str,
    target: &str,
) -> Result<String, String> {
    if app_key.trim().is_empty() || secret.trim().is_empty() {
        return Err("Youdao credentials are missing".to_owned());
    }
    let salt = uuid::Uuid::new_v4().to_string();
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "Invalid system clock")?
        .as_secs()
        .to_string();
    let sign = signature(app_key, secret, text, &salt, &time);
    let mut form =
        reqwest::Url::parse("https://example.invalid/").map_err(|_| "Cannot encode translation")?;
    let target = if target == "zh" { "zh-CHS" } else { target };
    form.query_pairs_mut()
        .append_pair("q", text)
        .append_pair("from", "auto")
        .append_pair("to", target)
        .append_pair("appKey", app_key)
        .append_pair("salt", &salt)
        .append_pair("curtime", &time)
        .append_pair("signType", "v3")
        .append_pair("sign", &sign)
        .append_pair("strict", "true");
    let result: Value = client
        .post("https://openapi.youdao.com/api")
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .body(form.query().unwrap_or_default().to_owned())
        .send()
        .await
        .map_err(|_| "Youdao connection failed")?
        .error_for_status()
        .map_err(|_| "Youdao HTTP request failed")?
        .json()
        .await
        .map_err(|_| "Invalid Youdao response")?;
    let code = result["errorCode"]
        .as_str()
        .ok_or("Missing Youdao error code")?;
    if code != "0" {
        return Err(format!("Youdao error {code}"));
    }
    let translations = result["translation"]
        .as_array()
        .ok_or("Missing Youdao translation")?;
    let translated = translations
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join("\n");
    if translated.is_empty() {
        return Err("Empty Youdao translation".to_owned());
    }
    Ok(translated)
}

#[cfg(test)]
mod tests {
    #[test]
    fn standalone_client_needs_no_ai_account() {
        assert!(super::client().is_ok());
        let config = crate::PredictConfig {
            api_key: Some(String::new()),
            api_key_env: "MENGREN_TEST_UNUSED_KEY".to_owned(),
            youdao: crate::YoudaoConfig {
                enabled: true,
                app_key: "test-id".to_owned(),
                secret: Some("test-secret".to_owned()),
            },
            ..crate::PredictConfig::default()
        };
        assert!(!config.subscription);
        assert!(crate::CloudPredictor::new(&config).is_ok());
    }
    #[test]
    fn signs_unicode_as_characters_and_truncates_only_long_text() {
        assert_eq!(super::signing_input("你好🙂"), "你好🙂");
        assert_eq!(
            super::signing_input("1234567890abcdefghijk"),
            "123456789021bcdefghijk"
        );
        let text = "🙂".repeat(21);
        assert_eq!(
            super::signing_input(&text),
            format!("{}21{}", "🙂".repeat(10), "🙂".repeat(10))
        );
        assert_eq!(
            super::signature("", "", "abc", "", ""),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
