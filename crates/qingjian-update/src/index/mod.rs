mod asset;
mod fetch;
mod release;
mod signature;

pub use asset::Asset;
#[cfg(feature = "desktop")]
pub(crate) use fetch::fetch_index;
pub(crate) use fetch::fetch_index_with_proxy;
pub use release::Release;
pub use signature::{PUBLIC_KEYS, verify};

use serde::Deserialize;

#[cfg(feature = "desktop")]
use qingjian_platform::UpdateChannel;

use crate::{Available, Target, UpdateError, Version};

/// 认得的索引格式版本；发版侧改了已有字段的含义才会加一，见到别的号就不读。
pub const SCHEMA_VERSION: u32 = 1;

/// 版本索引 `releases.json`，只取检查更新用得着的字段。
#[derive(Debug, Clone, Deserialize)]
pub struct Index {
    pub schema_version: u32,

    /// 从新到旧。
    pub releases: Vec<Release>,
}

impl Index {
    /// 解析已经验过签名的索引。
    pub fn parse(bytes: &[u8]) -> Result<Self, UpdateError> {
        let index: Self = serde_json::from_slice(bytes)?;
        if index.schema_version != SCHEMA_VERSION {
            return Err(UpdateError::UnsupportedSchema(index.schema_version));
        }
        Ok(index)
    }

    /// 比 `current` 新、渠道里看得到、有这台机器安装包的版本里最新的那个。
    #[cfg(feature = "desktop")]
    pub fn newest(
        &self,
        current: &Version,
        target: Target,
        channel: UpdateChannel,
    ) -> Option<Available> {
        self.for_target(current, target, channel == UpdateChannel::Beta)
    }

    /// 跨平台共同挑选规则，安卓无需依赖桌面配置库。
    pub fn for_target(&self, current: &Version, target: Target, beta: bool) -> Option<Available> {
        self.releases
            .iter()
            .filter(|release| beta || release.channel == "stable")
            .filter_map(|release| {
                let asset = release
                    .assets
                    .iter()
                    .find(|asset| asset.platform == target.platform && asset.cpu == target.cpu)?;
                let version = Version::parse(&release.version)?;
                (version > *current).then_some((version, release, asset))
            })
            .max_by(|left, right| left.0.cmp(&right.0))
            .map(|(_, release, asset)| Available {
                version: release.version.clone(),
                channel: release.channel.clone(),
                date: release.date.clone(),
                notes: release.notes.clone(),
                asset: Some(asset.clone()),
            })
    }
}

#[cfg(all(test, feature = "desktop"))]
mod tests {
    use super::*;

    const MAC: Target = Target {
        platform: "macos",
        cpu: "arm64",
    };
    const WINDOWS: Target = Target {
        platform: "windows",
        cpu: "x86_64",
    };

    fn index() -> Index {
        Index::parse(
            br#"{
              "schema_version": 1,
              "releases": [
                {"version": "0.1.5-beta.1", "channel": "beta", "assets": [{"platform": "macos", "cpu": "arm64"}]},
                {"version": "0.1.4", "channel": "stable", "date": "2026-10-01", "notes": ["a"],
                 "assets": [{"platform": "macos", "cpu": "arm64"}, {"platform": "macos", "cpu": "x86_64"}]},
                {"version": "0.1.4", "channel": "stable", "assets": [{"platform": "windows", "cpu": "x86_64"}]},
                {"version": "0.1.3", "channel": "stable", "assets": [{"platform": "macos", "cpu": "arm64"}]}
              ]
            }"#,
        )
        .unwrap()
    }

    fn newest(current: &str, target: Target, channel: UpdateChannel) -> Option<String> {
        index()
            .newest(&Version::parse(current).unwrap(), target, channel)
            .map(|available| available.version)
    }

    #[test]
    fn stable_channel_skips_prereleases() {
        assert_eq!(
            newest("0.1.3", MAC, UpdateChannel::Stable).as_deref(),
            Some("0.1.4")
        );
        assert_eq!(newest("0.1.4", MAC, UpdateChannel::Stable), None);
    }

    #[test]
    fn beta_channel_takes_the_newest_of_both() {
        assert_eq!(
            newest("0.1.3", MAC, UpdateChannel::Beta).as_deref(),
            Some("0.1.5-beta.1")
        );
        // 测试版用户在正式版出来后升到正式版
        assert_eq!(
            newest("0.1.4-beta.2", WINDOWS, UpdateChannel::Beta).as_deref(),
            Some("0.1.4")
        );
    }

    #[test]
    fn only_releases_with_a_package_for_this_machine_count() {
        // 0.1.5-beta.1 只发了 macOS
        assert_eq!(newest("0.1.4", WINDOWS, UpdateChannel::Beta), None);
    }

    #[test]
    fn unknown_schema_is_rejected() {
        let error = Index::parse(br#"{"schema_version": 2, "releases": []}"#).unwrap_err();
        assert!(matches!(error, UpdateError::UnsupportedSchema(2)));
    }
}
