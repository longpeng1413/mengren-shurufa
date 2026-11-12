//! 安卓更新出口：网络、签名与包校验复用更新 crate，壳只负责状态与系统安装。

use ::jni::{
    JNIEnv,
    objects::{JObject, JString},
    sys::{jlong, jstring},
};
use serde_json::{Value, json};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

static PERCENT: AtomicU64 = AtomicU64::new(0);
static CANCELLED: AtomicBool = AtomicBool::new(false);

fn reply(env: &mut JNIEnv<'_>, value: Value) -> jstring {
    env.new_string(value.to_string())
        .map(JString::into_raw)
        .unwrap_or(std::ptr::null_mut())
}
fn error(error: qingjian_update::UpdateError) -> Value {
    let code = match error {
        qingjian_update::UpdateError::Cancelled => "cancelled",
        qingjian_update::UpdateError::Integrity => "integrity",
        qingjian_update::UpdateError::InvalidPackage => "release_changed",
        qingjian_update::UpdateError::MalformedSignature
        | qingjian_update::UpdateError::UntrustedSignature => "signature",
        _ => "network",
    };
    json!({"error":code})
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_qingjian_inputmethod_core_Native_updateCheck(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    version: JString<'_>,
    proxy: JString<'_>,
) -> jstring {
    let value = catch_unwind(AssertUnwindSafe(|| {
        let version = env
            .get_string(&version)
            .map(String::from)
            .unwrap_or_default();
        let proxy = env.get_string(&proxy).map(String::from).unwrap_or_default();
        match qingjian_update::check_with_proxy(
            &version,
            qingjian_update::Target::current(),
            false,
            &proxy,
        ) {
            Ok(available) => json!({"available":available}),
            Err(err) => error(err),
        }
    }))
    .unwrap_or_else(|_| json!({"error":"network"}));
    reply(&mut env, value)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_qingjian_inputmethod_core_Native_updateDownload(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    current: JString<'_>,
    version: JString<'_>,
    proxy: JString<'_>,
    cache: JString<'_>,
) -> jstring {
    PERCENT.store(0, Ordering::Relaxed);
    CANCELLED.store(false, Ordering::Relaxed);
    let value = catch_unwind(AssertUnwindSafe(|| {
        let current = env
            .get_string(&current)
            .map(String::from)
            .unwrap_or_default();
        let version = env
            .get_string(&version)
            .map(String::from)
            .unwrap_or_default();
        let proxy = env.get_string(&proxy).map(String::from).unwrap_or_default();
        let cache = env.get_string(&cache).map(String::from).unwrap_or_default();
        match qingjian_update::download_release(
            &current,
            qingjian_update::Target::current(),
            false,
            &version,
            std::path::Path::new(&cache),
            &proxy,
            |done, total| {
                PERCENT.store(done.saturating_mul(100) / total.max(1), Ordering::Relaxed);
                !CANCELLED.load(Ordering::Relaxed)
            },
        ) {
            Ok(path) => json!({"path":path}),
            Err(err) => error(err),
        }
    }))
    .unwrap_or_else(|_| json!({"error":"network"}));
    reply(&mut env, value)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_qingjian_inputmethod_core_Native_updateProgress(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
) -> jlong {
    PERCENT.load(Ordering::Relaxed) as jlong
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_qingjian_inputmethod_core_Native_updateCancel(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
) {
    CANCELLED.store(true, Ordering::Relaxed);
}

/// 猛人 GitHub 最新正式版的发布页，与桌面端「下载」打开的是同一个地址；关于页显示并可用浏览器打开。
#[unsafe(no_mangle)]
pub extern "system" fn Java_app_qingjian_inputmethod_core_Native_updateDownloadPage<'local>(
    env: JNIEnv<'local>,
    _this: JObject<'local>,
) -> jstring {
    env.new_string(qingjian_update::DOWNLOAD_URL)
        .map(JString::into_raw)
        .unwrap_or(std::ptr::null_mut())
}
