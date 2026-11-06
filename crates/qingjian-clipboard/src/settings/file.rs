//! 配置文件里剪贴板用到的分节。每个值先按 `toml::Value` 读，写错的键才不会连累别的键。

use serde::Deserialize;

#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct File {
    pub(super) general: GeneralSection,

    pub(super) clipboard: ClipboardSection,

    pub(super) shortcut: ShortcutSection,
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct GeneralSection {
    pub(super) ui_language: Option<toml::Value>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct ClipboardSection {
    pub(super) enabled: Option<toml::Value>,

    pub(super) edge: Option<toml::Value>,

    pub(super) history_limit: Option<toml::Value>,

    pub(super) paused: Option<toml::Value>,

    pub(super) excluded_apps: Option<toml::Value>,

    pub(super) sync_enabled: Option<toml::Value>,

    pub(super) relay_enabled: Option<toml::Value>,

    pub(super) private_receive: Option<toml::Value>,

    pub(super) private_auto_fill: Option<toml::Value>,

    pub(super) private_send_mode: Option<toml::Value>,

    pub(super) relay_url: Option<toml::Value>,

    pub(super) relay_revision: Option<toml::Value>,

    pub(super) sync_neon: Option<toml::Value>,

    pub(super) device_name: Option<toml::Value>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct ShortcutSection {
    pub(super) private_input: Option<toml::Value>,

    pub(super) private_send: Option<toml::Value>,

    pub(super) clipboard: Option<toml::Value>,
}
