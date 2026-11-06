//! 读取所选服务可用的模型；会员目录只使用官方固定地址，不发送输入内容。

use crate::{PredictConfig, subscription};

pub async fn list(config: &PredictConfig) -> Result<Vec<String>, String> {
    let client = subscription::client_with_proxy(&config.proxy).map_err(|_| "invalid proxy")?;
    let token = if config.subscription {
        match config.member_token.as_ref() {
            Some(token) if !token.expose().is_empty() => token.expose().to_owned(),
            _ => subscription::access_token(&client)
                .await
                .map_err(|_| "member sign-in required")?,
        }
    } else {
        config
            .resolve_api_key()
            .filter(|key| !key.is_empty())
            .ok_or("missing service key")?
    };
    let base = if config.subscription {
        "https://api.openai.com/v1"
    } else {
        config.base_url.trim_end_matches('/')
    };
    let mut reply = client
        .get(format!("{base}/models"))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|_| "model request failed")?
        .error_for_status()
        .map_err(|_| "model request rejected")?;
    let mut bytes = Vec::new();
    while let Some(chunk) = reply.chunk().await.map_err(|_| "model response failed")? {
        if bytes.len().saturating_add(chunk.len()) > 2 * 1024 * 1024 {
            return Err("model response too large".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let body = serde_json::from_slice(&bytes).map_err(|_| "invalid model response")?;
    parse(&body, config.subscription)
}

fn parse(body: &serde_json::Value, member: bool) -> Result<Vec<String>, String> {
    let mut models: Vec<String> = if member {
        body["models"]
            .as_array()
            .ok_or("missing member model list")?
            .iter()
            .filter(|model| model["visibility"] == "list")
            .filter_map(|model| model["slug"].as_str())
            .take(2000)
            .map(str::to_owned)
            .collect()
    } else {
        body["data"]
            .as_array()
            .ok_or("model listing is unsupported")?
            .iter()
            .filter_map(|model| model["id"].as_str())
            .take(2000)
            .map(str::to_owned)
            .collect()
    };
    models.retain(|model| !model.trim().is_empty());
    models.dedup();
    Ok(models)
}

#[cfg(test)]
mod tests {
    #[test]
    fn member_catalog_excludes_hidden_models_and_api_ids() {
        let body = serde_json::json!({"models":[{"slug":"available","visibility":"list"},{"slug":"hidden","visibility":"hidden"}],"data":[{"id":"paid-api"}]});
        assert_eq!(super::parse(&body, true).unwrap(), ["available"]);
        assert_eq!(super::parse(&body, false).unwrap(), ["paid-api"]);
        assert!(super::parse(&serde_json::json!({"data":[]}), true).is_err());
    }
}
