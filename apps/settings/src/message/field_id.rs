//! 输入框的编号：编辑状态按它存在 `Settings::fields` 里。

/// 输入框：云服务页的三个，短语编辑卡片里的三个，生词本页的搜索框。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FieldId {
    BaseUrl,
    Model,
    ApiKey,
    Proxy,
    YoudaoAppKey,
    YoudaoSecret,
    RelayUrl,
    RelayToken,
    PhraseCode,
    PhraseWeight,
    PhraseText,
    /// 生词本页的搜索框，不存进配置。
    VocabularySearch,
}

impl FieldId {
    /// 存进配置的那几个（短语的几个随卡片一起存）。
    pub const ALL: [Self; 8] = [
        Self::BaseUrl,
        Self::Model,
        Self::ApiKey,
        Self::Proxy,
        Self::YoudaoAppKey,
        Self::YoudaoSecret,
        Self::RelayUrl,
        Self::RelayToken,
    ];

    /// 配置文件 `[predict]` 里的键。
    pub fn key(self) -> &'static str {
        match self {
            Self::BaseUrl => "base_url",
            Self::Model => "model",
            Self::ApiKey => "api_key",
            Self::Proxy => "proxy",
            Self::YoudaoAppKey => "app_key",
            Self::YoudaoSecret => "secret",
            Self::RelayUrl => "relay_url",
            Self::RelayToken => "relay_token",
            Self::PhraseCode => "code",
            Self::PhraseWeight => "weight",
            Self::PhraseText => "text",
            Self::VocabularySearch => "vocabulary-search",
        }
    }
}
