//! 验证码提取边界：前导零、完整短信、歧义和普通标识符。

use super::extract;

#[test]
fn copied_digits_and_sms_keep_leading_zeroes() {
    assert_eq!(extract(" 001234 ", "", false), Some("001234"));
    assert_eq!(
        extract("【测试】验证码001234，5分钟有效。", "", false),
        Some("001234")
    );
    assert_eq!(
        extract("Your verification code is 654321.", "", false),
        Some("654321")
    );
    assert_eq!(extract("Code: 9876", "短信验证码", false), Some("9876"));
    assert_eq!(extract("Code: 9876", "", true), Some("9876"));
}

#[test]
fn rejects_phone_card_ambiguous_and_embedded_identifiers() {
    for text in [
        "13800138000",
        "6216616309000000",
        "123",
        "123456789",
        "id=123456",
        "code=AB1234",
        "notpossible 123456",
    ] {
        assert_eq!(extract(text, "", false), None, "{text}");
    }
    assert_eq!(extract("验证码123456，另一验证码654321", "", false), None);
    assert_eq!(
        extract("验证码123456，再次提示123456", "", false),
        Some("123456")
    );
    assert_eq!(extract(&"1".repeat(4097), "", true), None);
}
