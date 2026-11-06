//! Windows 会员登录：系统浏览器授权后用 DPAPI 保存凭据。

use qingjian_predict::subscription::{Account, auth::LoginListener};
use std::path::Path;
use std::time::Duration;

pub fn login(config: Option<&Path>) -> Result<(), Box<dyn std::error::Error>> {
    let path = qingjian_platform::member::path().ok_or("No local data directory")?;
    let host_path = path.with_file_name("host-id");
    if let Some(parent) = host_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let host_id = match std::fs::read_to_string(&host_path) {
        Ok(id) => id,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            // 每次安装保持同一个设备标识，和账号令牌分开。
            let id = format!("urn:uuid:{}", uuid::Uuid::new_v4());
            std::fs::write(&host_path, &id)?;
            id
        }
        Err(error) => return Err(error.into()),
    };
    let listener = LoginListener::bind(host_id)?;
    crate::system::action::open(listener.authorization.url().as_str())?;
    let callback = listener.wait(Duration::from_secs(600))?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let proxy = config
            .and_then(|path| qingjian_platform::Config::load(path).ok())
            .map(|config| config.predict.proxy)
            .unwrap_or_default();
        let client = qingjian_predict::subscription::client_with_proxy(&proxy)?;
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
            .ok_or("No eligible member model")?;
        qingjian_platform::member::save(&path, &account).map_err(|error| error.to_string())?;
        if let Some(config) = config {
            qingjian_platform::Config::set_value(config, "predict", "member_model", model)?;
            qingjian_platform::Config::set_bool(config, "predict", "subscription", true)?;
        }
        Ok::<(), Box<dyn std::error::Error>>(())
    })
}
