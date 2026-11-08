//! 只提取壳已读取的剪贴板文本，不记录文本、验证码或分类结果。

use ::jni::{
    JNIEnv,
    objects::{JObject, JString},
    sys::{JNI_TRUE, jboolean, jstring},
};

#[unsafe(no_mangle)]
pub extern "system" fn Java_app_qingjian_inputmethod_core_Native_clipboardVerificationCode<
    'local,
>(
    mut env: JNIEnv<'local>,
    _this: JObject<'local>,
    text: JString<'local>,
    label: JString<'local>,
    classified: jboolean,
) -> jstring {
    let (Ok(text), Ok(label)) = (
        env.get_string(&text).map(String::from),
        env.get_string(&label).map(String::from),
    ) else {
        return std::ptr::null_mut();
    };
    let Some(code) =
        qingjian_core::verification_code::extract(&text, &label, classified == JNI_TRUE)
    else {
        return std::ptr::null_mut();
    };
    env.new_string(code)
        .map(JString::into_raw)
        .unwrap_or(std::ptr::null_mut())
}
