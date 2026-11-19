//! Windows 语音面板、状态条和翻译窗口共用文案。

bamboo_slip_i18n::catalog! {
    pub static CATALOG = {
        "zh-Hans" => "../locales/windows/zh-Hans.toml",
        "zh-Hant" => "../locales/windows/zh-Hant.toml",
        "en" => "../locales/windows/en.toml",
        "ja" => "../locales/windows/ja.toml",
        "es" => "../locales/windows/es.toml",
        "vi" => "../locales/windows/vi.toml",
        "de" => "../locales/windows/de.toml",
    };
}

pub fn tr(key: &'static str) -> &'static str {
    CATALOG.get(key)
}

#[cfg(test)]
mod tests {
    #[test]
    fn catalogs_agree() {
        assert!(super::CATALOG.problems().is_empty());
    }
}
