//! 云端大模型（OpenAI 兼容接口）：安卓拿它做快捷翻译、释义兜底，开了「云联想」时也做组句联想；没开时平时敲的字不往外发，只发用户放进翻译框的那段与兜底时那个词。
//! 密钥由 Kotlin 从 Android Keystore 解出来交过来，Rust 侧只放在内存里（`PredictConfig` 里），不落盘、不进日志。

mod settings;

pub use settings::CloudSettings;

use qingjian_predict::{ConnectionTest, PredictError};

/// 键盘设置里「测试连接」：用这份配置发一条最小请求，阻塞到有结果（请求自带超时）。Kotlin 在后台线程调。
/// 成功回 `Ok(提示)`，失败回 `Err(给用户看的原因)`。
pub fn test_connection(settings: &CloudSettings) -> Result<String, String> {
    let config = settings.predict_config();
    let report = ConnectionTest::start(&config)
        .and_then(ConnectionTest::wait)
        .map_err(|error| describe(&error))?;
    Ok(format!(
        "连上了：{}，用时 {} 毫秒",
        report.model,
        report.elapsed.as_millis()
    ))
}

/// 错误翻成给用户看的话（`PredictError` 的文案是英文，给日志用）。
fn describe(error: &PredictError) -> String {
    match error {
        PredictError::InvalidProxy => "代理地址无效，请检查格式".to_owned(),
        PredictError::Subscription(
            qingjian_predict::subscription::SubscriptionError::NoAccount,
        ) => "ChatGPT 尚未登录或登录已失效，请重新登录".to_owned(),
        PredictError::Subscription(_) => {
            "ChatGPT 会员请求失败，请检查会员用量、网络和代理".to_owned()
        }
        PredictError::Translation(reason) => {
            format!("有道翻译失败，请检查应用 ID、密钥、服务开通情况和余额。原因：{reason}")
        }
        PredictError::MissingApiKey(_) => "还没填密钥".to_owned(),
        PredictError::Timeout(ms) => format!("{ms} 毫秒内没有回应，检查接口地址和网络"),
        _ if error.is_account_failure() => "密钥无效、余额不足或没有权限".to_owned(),
        PredictError::EmptyReply | PredictError::BudgetExhausted => {
            "连上了，但模型没给出内容，换个模型试试".to_owned()
        }
        _ => format!("连不上：{error}"),
    }
}
