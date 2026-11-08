//! 从用户已复制的短数字或带验证码语义的短信中提取唯一数字码，不读取短信或存储正文。

#[cfg(test)]
mod tests;

/// 只接受 4–8 位数字；整段文字须有验证码语义，多个不同的数字码不猜测。
pub fn extract<'a>(text: &'a str, label: &str, classified: bool) -> Option<&'a str> {
    if text.len() > 4096 || label.len() > 1024 {
        return None;
    }
    let text = text.trim();
    let valid = |s: &str| (4..=8).contains(&s.len()) && s.bytes().all(|c| c.is_ascii_digit());
    if valid(text) {
        return Some(text);
    }
    if !classified && !has_context(text) && !has_context(label) {
        return None;
    }
    let mut result = None;
    // ASCII 字母粘连的片段不是纯数字码，中文提示语可以紧邻数字。
    for token in text.split(|c: char| !c.is_ascii_alphanumeric()) {
        if valid(token) {
            if result.is_some_and(|old| old != token) {
                return None;
            }
            result = Some(token);
        }
    }
    result
}

fn has_context(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    [
        "验证码",
        "驗證碼",
        "校验码",
        "校驗碼",
        "动态码",
        "短信码",
        "verification code",
        "security code",
        "one-time code",
        "one time code",
    ]
    .iter()
    .any(|word| text.contains(word))
        || text
            .split(|c: char| !c.is_ascii_alphanumeric())
            .any(|word| word == "otp")
}
