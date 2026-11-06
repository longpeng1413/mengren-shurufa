//! 交互式会员链路验收：浏览器授权、身份验签、模型发现和真实会员请求。
//! 不读取既有凭据，不落盘令牌，不调用 API Key 付费路径。

use qingjian_predict::subscription::{Account, auth::LoginListener, complete};
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if !std::env::args().any(|arg| arg == "--login") {
        eprintln!(
            "Run with --login to open ChatGPT authorization. Credentials are kept in memory only."
        );
        return Ok(());
    }
    let listener = LoginListener::bind(format!("urn:uuid:{}", uuid::Uuid::new_v4()))?;
    let url = listener.authorization.url().to_string();
    #[cfg(target_os = "windows")]
    std::process::Command::new("rundll32.exe")
        .args(["url.dll,FileProtocolHandler", &url])
        .spawn()?;
    #[cfg(not(target_os = "windows"))]
    println!("Open {url}");
    println!("Waiting for browser authorization (10 minutes)...");
    let callback = listener.wait(Duration::from_secs(600))?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let account = Account::sign_in(&client, &listener.authorization, &callback).await?;
        let catalog: serde_json::Value = client
            .get("https://api.openai.com/v1/models")
            .bearer_auth(&account.access_token)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let model = catalog["models"]
            .as_array()
            .and_then(|models| models.iter().find(|model| model["visibility"] == "list"))
            .and_then(|model| model["slug"].as_str())
            .ok_or("No eligible member model returned")?;
        println!("Member authorization verified. Testing model: {model}");
        let result = complete(
            &client,
            "https://api.openai.com/v1/responses",
            &account.access_token,
            model,
            "Reply with exactly OK.",
            "Connection test",
        )
        .await?;
        println!("Member response: {result}");
        Ok::<(), Box<dyn std::error::Error>>(())
    })
}
