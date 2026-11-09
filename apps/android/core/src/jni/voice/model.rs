//! 离线语音模型的出口：状态、下载（阻塞，Kotlin 在后台线程调，进度另外轮询状态）、取消、删除与启动时清理。
//! `root` 是 App 私有目录下的模型根目录，Kotlin 每次都传同一个。

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::SystemTime;

use ::jni::JNIEnv;
use ::jni::objects::{JObject, JString};
use ::jni::sys::jstring;
use qingjian_voice_offline::{DownloadProgress, SpeechModel};
use serde_json::{Value, json};

static PROGRESS: DownloadProgress = DownloadProgress::new();

fn text(env: &mut JNIEnv<'_>, value: &JString<'_>) -> String {
    env.get_string(value).map(String::from).unwrap_or_default()
}

fn reply(env: &mut JNIEnv<'_>, value: &Value) -> jstring {
    env.new_string(value.to_string())
        .map(JString::into_raw)
        .unwrap_or(std::ptr::null_mut())
}

/// `{installed, bytes, total, downloading, received, loaded}`：装好没有、占用字节、要下载的总字节、是否在下载与已下字节、内存里是否加载着。
#[unsafe(no_mangle)]
pub extern "system" fn Java_app_qingjian_inputmethod_core_Native_voiceModelStatus<'local>(
    mut env: JNIEnv<'local>,
    _this: JObject<'local>,
    root: JString<'local>,
) -> jstring {
    let root = text(&mut env, &root);
    let value = catch_unwind(|| {
        let model = SpeechModel::new(&root);
        json!({
            "installed": model.installed(),
            "bytes": model.size(),
            "total": model.total_bytes(),
            "downloading": SpeechModel::downloading(),
            "received": PROGRESS.received(),
            "loaded": qingjian_voice_offline::model_loaded(),
        })
    })
    .unwrap_or_else(|_| json!({}));
    reply(&mut env, &value)
}

/// 下载模型，阻塞到下完、出错或取消；回 `{ok: true}` 或 `{error: 给用户看的话}`。
#[unsafe(no_mangle)]
pub extern "system" fn Java_app_qingjian_inputmethod_core_Native_voiceModelDownload<'local>(
    mut env: JNIEnv<'local>,
    _this: JObject<'local>,
    root: JString<'local>,
    proxy: JString<'local>,
) -> jstring {
    super::super::logging::init(None);
    let root = text(&mut env, &root);
    let proxy = text(&mut env, &proxy);
    let value = catch_unwind(AssertUnwindSafe(|| {
        match SpeechModel::new(&root).download(&proxy, &PROGRESS) {
            Ok(()) => json!({ "ok": true }),
            Err(error) => {
                tracing::warn!(%error, "speech model download failed");
                json!({ "error": error.describe() })
            }
        }
    }))
    .unwrap_or_else(|_| json!({ "error": "下载时出错了" }));
    reply(&mut env, &value)
}

/// 让正在进行的下载停下（半截文件留着，下次续传）。
#[unsafe(no_mangle)]
pub extern "system" fn Java_app_qingjian_inputmethod_core_Native_voiceModelCancel<'local>(
    _env: JNIEnv<'local>,
    _this: JObject<'local>,
) {
    PROGRESS.cancel();
}

/// 删除模型（先放掉内存里那份）；下载中不删。回 `{ok: true}` 或 `{error}`。
#[unsafe(no_mangle)]
pub extern "system" fn Java_app_qingjian_inputmethod_core_Native_voiceModelDelete<'local>(
    mut env: JNIEnv<'local>,
    _this: JObject<'local>,
    root: JString<'local>,
) -> jstring {
    let root = text(&mut env, &root);
    let value = catch_unwind(|| {
        if SpeechModel::downloading() {
            return json!({ "error": "模型正在下载，请先取消下载" });
        }
        qingjian_voice_offline::release_shared();
        match SpeechModel::new(&root).delete() {
            Ok(()) => json!({ "ok": true }),
            Err(error) => {
                tracing::warn!(%error, "cannot delete speech model");
                json!({ "error": "删除失败，稍后再试" })
            }
        }
    })
    .unwrap_or_else(|_| json!({ "error": "删除时出错了" }));
    reply(&mut env, &value)
}

/// 启动时清理：别的版本的模型、超过 7 天没续上的半截下载。
#[unsafe(no_mangle)]
pub extern "system" fn Java_app_qingjian_inputmethod_core_Native_voiceModelCleanup<'local>(
    mut env: JNIEnv<'local>,
    _this: JObject<'local>,
    root: JString<'local>,
) {
    let root = text(&mut env, &root);
    let _ = catch_unwind(|| SpeechModel::new(&root).cleanup(SystemTime::now()));
}
