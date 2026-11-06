//! 输入框：编辑状态与配置之间的换入、存回。

use bamboo_slip::TextEdit;

use super::Settings;
use crate::i18n::tr;
use crate::message::FieldId;

impl Settings {
    pub(super) fn with_fields(mut self) -> Self {
        for id in FieldId::ALL {
            self.reset_field(id);
        }
        self
    }

    /// 输入框换回配置里的值（密钥框总是空的，不显示已存的密钥）。
    pub(super) fn reset_field(&mut self, id: FieldId) {
        let predict = &self.config.predict;
        let value = match id {
            FieldId::BaseUrl => predict.base_url.clone(),
            FieldId::Model => predict.selected_model().to_owned(),
            FieldId::ApiKey => String::new(),
            FieldId::Proxy => predict.proxy.clone(),
            FieldId::YoudaoAppKey => predict.youdao.app_key.clone(),
            FieldId::YoudaoSecret => String::new(),
            FieldId::VocabularySearch => String::new(),
            FieldId::PhraseCode | FieldId::PhraseWeight | FieldId::PhraseText => return,
        };
        self.fields.insert(id, TextEdit::new(value));
    }

    /// 存一个输入框：没改、清空了都不存（清空等于不改）。
    pub(super) fn commit(&mut self, id: FieldId) {
        let Some(text) = self
            .fields
            .get(&id)
            .map(|edit| edit.text().trim().to_owned())
        else {
            return;
        };
        match id {
            FieldId::YoudaoAppKey => self.write("predict.youdao", "app_key", text),
            FieldId::YoudaoSecret =>
            {
                #[cfg(windows)]
                if !text.is_empty() {
                    self.notice = Some((
                        "cloud",
                        match qingjian_platform::member::save_youdao_secret(&text) {
                            Ok(()) => tr("cloud.key_saved").to_owned(),
                            Err(_) => tr("cloud.youdao_save_failed").to_owned(),
                        },
                    ));
                }
            }
            FieldId::Proxy => {
                self.write("predict", "proxy", text);
            }
            FieldId::BaseUrl | FieldId::Model => {
                let current = match id {
                    FieldId::BaseUrl => self.config.predict.base_url.as_str(),
                    _ => self.config.predict.selected_model(),
                };
                if !text.is_empty() && text != current {
                    let key = if id == FieldId::Model && self.config.predict.subscription {
                        "member_model"
                    } else {
                        id.key()
                    };
                    self.write("predict", key, text);
                }
            }
            FieldId::PhraseCode
            | FieldId::PhraseWeight
            | FieldId::PhraseText
            | FieldId::VocabularySearch => return,
            FieldId::ApiKey if text.is_empty() => {}
            // 密码框看不见内容，粘贴多了（带上了终端提示符、命令）用户发现不了
            FieldId::ApiKey if text.chars().any(|c| !c.is_ascii_graphic()) => {
                self.notice = Some(("cloud", tr("cloud.bad_key").to_owned()));
            }
            FieldId::ApiKey => {
                qingjian_platform::logs::secrets::register(&text);
                self.write("predict", id.key(), text);
                self.notice = Some(("cloud", tr("cloud.key_saved").to_owned()));
            }
        }
        self.reset_field(id);
    }
}
