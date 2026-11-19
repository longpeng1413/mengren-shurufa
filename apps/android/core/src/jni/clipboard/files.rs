//! 文件描述符只在本机复制一次所有权，权限、目标与协议由同步服务检查。
use super::sync::running;
use jni::{
    JNIEnv,
    objects::{JObject, JString},
    sys::{JNI_FALSE, JNI_TRUE, jboolean, jint, jlong},
};
use std::os::fd::BorrowedFd;

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_qingjian_inputmethod_core_Native_lanFileEnabled(
    _env: JNIEnv,
    _this: JObject,
) -> jboolean {
    running()
        .as_ref()
        .is_some_and(|r| r.service.files_enabled())
        .into()
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_app_qingjian_inputmethod_core_Native_lanFileConfigure(
    mut env: JNIEnv,
    _this: JObject,
    id: JString,
    value: jboolean,
) -> jboolean {
    let Ok(id) = env.get_string(&id).map(String::from) else {
        return JNI_FALSE;
    };
    let guard = running();
    let Some(r) = guard.as_ref() else {
        return JNI_FALSE;
    };
    let ok = if id.is_empty() {
        r.service.set_files_enabled(value != JNI_FALSE)
    } else {
        r.service.select_file_peer(&id, value != JNI_FALSE)
    };
    ok.is_ok().into()
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_app_qingjian_inputmethod_core_Native_lanFileNetwork(
    mut env: JNIEnv,
    _this: JObject,
    json: JString,
) {
    let json = env.get_string(&json).map(String::from).unwrap_or_default();
    qingjian_clipboard_sync::service::files::update_android_network(&json);
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_app_qingjian_inputmethod_core_Native_lanFileSend(
    mut env: JNIEnv,
    _this: JObject,
    fd: jint,
    name: JString,
    size: jlong,
) -> jboolean {
    let Ok(name) = env.get_string(&name).map(String::from) else {
        return JNI_FALSE;
    };
    if fd < 0 {
        return JNI_FALSE;
    }
    let Ok(owned) = (unsafe { BorrowedFd::borrow_raw(fd) }).try_clone_to_owned() else {
        return JNI_FALSE;
    };
    let guard = running();
    let Some(r) = guard.as_ref().filter(|r| r.service.files_enabled()) else {
        return JNI_FALSE;
    };
    r.service
        .send_file(owned.into(), name, u64::try_from(size).ok());
    JNI_TRUE
}
