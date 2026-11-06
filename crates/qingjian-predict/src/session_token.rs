//! 仅运行时使用的会员令牌，调试输出隐藏内容，不进入配置序列化。

#[derive(Clone, PartialEq, Eq)]
pub struct SessionToken(String);

impl SessionToken {
    pub fn new(value: String) -> Self {
        Self(value)
    }
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for SessionToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SessionToken([redacted])")
    }
}
