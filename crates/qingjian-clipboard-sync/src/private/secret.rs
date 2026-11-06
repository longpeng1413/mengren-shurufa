//! 私密文字的受限所有权；丢弃时覆盖可控制的明文内存。

use zeroize::Zeroizing;

pub struct Secret(Zeroizing<String>);

impl Secret {
    pub fn new(text: String) -> Option<Self> {
        let text = Zeroizing::new(text);
        if text.is_empty()
            || text.len() > super::MAX_SECRET_BYTES
            || text.chars().any(char::is_control)
        {
            return None;
        }
        Some(Self(text))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret([REDACTED])")
    }
}
