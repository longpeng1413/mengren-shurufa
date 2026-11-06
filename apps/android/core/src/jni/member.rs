//! 会员登录与续期的 JNI 出口；令牌只交给 Java 密钥库封装，不写日志。

use jni::{
    JNIEnv,
    objects::{JObject, JString},
    sys::jstring,
};
use qingjian_predict::subscription::{Account, auth::LoginListener};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, LazyLock, Mutex};

type PendingLogin = (Arc<LoginListener>, String);
static PENDING: LazyLock<Mutex<Option<PendingLogin>>> = LazyLock::new(|| Mutex::new(None));

fn output(env: &JNIEnv<'_>, value: Result<String, String>) -> jstring {
    let json = match value {
        Ok(value) => value,
        Err(error) => serde_json::json!({"error": error}).to_string(),
    };
    env.new_string(json)
        .map(JString::into_raw)
        .unwrap_or(std::ptr::null_mut())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_qingjian_inputmethod_core_Native_memberStart<'a>(
    mut env: JNIEnv<'a>,
    _: JObject<'a>,
    host: JString<'a>,
    proxy: JString<'a>,
) -> jstring {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let host = env
            .get_string(&host)
            .map(String::from)
            .map_err(|_| "Cannot read host ID")?;
        let listener =
            Arc::new(LoginListener::bind(host).map_err(|_| "Cannot open login callback")?);
        let url = listener.authorization.url().to_string();
        let proxy = env
            .get_string(&proxy)
            .map(String::from)
            .map_err(|_| "Cannot read proxy")?;
        *PENDING.lock().map_err(|_| "Cannot start login")? = Some((listener, proxy));
        Ok::<_, &str>(serde_json::json!({"url": url}).to_string())
    }))
    .unwrap_or(Err("Cannot start login"))
    .map_err(str::to_owned);
    output(&env, result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_qingjian_inputmethod_core_Native_memberFinish<'a>(
    env: JNIEnv<'a>,
    _: JObject<'a>,
) -> jstring {
    let result = catch_unwind(AssertUnwindSafe(|| -> Result<String, String> {
        let (listener, proxy) = PENDING
            .lock()
            .map_err(|_| "Cannot finish login")?
            .take()
            .ok_or("No login pending")?;
        let callback = listener
            .wait(std::time::Duration::from_secs(600))
            .map_err(|error| error.to_string())?;
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| error.to_string())?;
        rt.block_on(async {
            let client = qingjian_predict::subscription::client_with_proxy(&proxy)
                .map_err(|error| error.to_string())?;
            let account = Account::sign_in(&client, &listener.authorization, &callback)
                .await
                .map_err(|error| error.to_string())?;
            let catalog: serde_json::Value = client
                .get("https://api.openai.com/v1/models")
                .bearer_auth(&account.access_token)
                .send()
                .await
                .map_err(|_| "Cannot list member models")?
                .error_for_status()
                .map_err(|_| "Member model list rejected")?
                .json()
                .await
                .map_err(|_| "Invalid member model list")?;
            let model = catalog["models"]
                .as_array()
                .and_then(|models| models.iter().find(|model| model["visibility"] == "list"))
                .and_then(|model| model["slug"].as_str())
                .ok_or("No eligible member model")?;
            Ok(serde_json::json!({"account": account, "model": model}).to_string())
        })
    }))
    .unwrap_or_else(|_| Err("Cannot finish login".to_owned()));
    output(&env, result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_qingjian_inputmethod_core_Native_memberRefresh<'a>(
    mut env: JNIEnv<'a>,
    _: JObject<'a>,
    account: JString<'a>,
    proxy: JString<'a>,
) -> jstring {
    let result = catch_unwind(AssertUnwindSafe(|| -> Result<String, String> {
        let json = env
            .get_string(&account)
            .map(String::from)
            .map_err(|_| "Cannot read account")?;
        let mut account: Account = serde_json::from_str(&json).map_err(|_| "Invalid account")?;
        let proxy = env
            .get_string(&proxy)
            .map(String::from)
            .map_err(|_| "Cannot read proxy")?;
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| "Cannot start login runtime")?;
        rt.block_on(async {
            let client = qingjian_predict::subscription::client_with_proxy(&proxy)
                .map_err(|_| "Cannot create login client")?;
            account
                .refresh(&client)
                .await
                .map_err(|_| "ChatGPT sign-in required")?;
            serde_json::to_string(&account).map_err(|_| "Cannot encode account".to_owned())
        })
    }))
    .unwrap_or_else(|_| Err("Cannot refresh account".to_owned()));
    output(&env, result)
}
