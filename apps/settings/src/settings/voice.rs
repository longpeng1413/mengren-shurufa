//! 语音设置页的状态维护：豆包密钥的 .env 读写、模型根目录、消息处理里的重活。
//! 密钥写进配置同目录的 `.env`（见 `VoiceConfig::api_key_env`），不进 config.toml、不进配置导出、不进日志。

use std::path::{Path, PathBuf};

use super::Settings;
use crate::i18n::{tr, tr_args};

impl Settings {
    /// 语音设置页「测试」：按当前服务测豆包连接或离线模型。
    pub(super) fn voice_test(&mut self) {
        if self.voice_test.running() {
            return;
        }
        let config = self.config.voice.clone();
        let outcome = if config.is_volcengine() {
            self.voice_test
                .start_volcengine(&config, self.waker.clone())
        } else if config.is_offline() {
            let root = self.voice_model_root();
            self.voice_test.start_offline(&root, self.waker.clone())
        } else {
            Err(tr("voice.not_enabled").to_owned())
        };
        let text = match outcome {
            Ok(()) => tr("voice.testing").to_owned(),
            Err(error) => error,
        };
        self.notice = Some(("voice", text));
    }

    /// 模型根目录：与 Server 的默认一致（`<用户目录>/voice-model`）；配置里写了就用写的。
    pub(crate) fn voice_model_root(&self) -> PathBuf {
        let configured = &self.config.voice.model_root;
        if !configured.trim().is_empty() {
            return PathBuf::from(configured.trim());
        }
        crate::system::paths::user_dir()
            .map(|dir| dir.join("voice-model"))
            .unwrap_or_else(|| PathBuf::from("voice-model"))
    }

    /// 豆包密钥是否已设置：配置里或 .env 环境变量里有。
    pub(crate) fn voice_key_set(&self) -> bool {
        self.config.voice.resolve_api_key().is_some()
    }

    /// 写豆包密钥到 .env（原子替换），并登记脱敏；清空则删除这一行。
    pub(super) fn save_voice_key(&mut self, key: &str) -> Result<(), String> {
        let env_file = self.voice_env_path().ok_or_else(no_env_dir)?;
        let name = self.config.voice.api_key_env.clone();
        let old = std::fs::read_to_string(&env_file).unwrap_or_default();
        let next = set_env_value(&old, &name, key);
        if let Some(parent) = env_file.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        atomic_write(&env_file, &next)?;
        if !key.is_empty() {
            qingjian_platform::logs::secrets::register(key);
        }
        // 让本进程立刻读到新值，测试连接才用得上
        // SAFETY: 单线程的设置程序，写环境变量不与其它线程的数据竞争。
        unsafe {
            if key.is_empty() {
                std::env::remove_var(&name);
            } else {
                std::env::set_var(&name, key);
            }
        }
        Ok(())
    }

    /// 清掉豆包密钥。
    pub(super) fn clear_voice_key(&mut self) -> Result<(), String> {
        self.save_voice_key("")
    }

    /// 配置文件同目录的 `.env`。
    fn voice_env_path(&self) -> Option<PathBuf> {
        self.path.parent().map(|dir| dir.join(".env"))
    }
}

fn no_env_dir() -> String {
    tr("voice.env_no_dir").to_owned()
}

/// 在 .env 文本里设一个键；`value` 空串则删掉这一行。
fn set_env_value(text: &str, name: &str, value: &str) -> String {
    let mut kept: Vec<String> = text
        .lines()
        .filter(|line| {
            line.trim()
                .split_once('=')
                .is_none_or(|(key, _)| key.trim() != name)
        })
        .map(str::to_owned)
        .collect();
    if !value.is_empty() {
        let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
        kept.push(format!("{name}=\"{escaped}\""));
    }
    let mut out = kept.join("\n");
    out.push('\n');
    out
}

/// 写临时文件再改名，避免写一半断电留下半截 .env。
fn atomic_write(path: &Path, text: &str) -> Result<(), String> {
    let temporary = path.with_extension("env.tmp");
    std::fs::write(&temporary, text).map_err(|e| tr_args("voice.env_failed", &[("error", &e)]))?;
    std::fs::rename(&temporary, path).map_err(|e| tr_args("voice.env_failed", &[("error", &e)]))
}

#[cfg(test)]
mod tests {
    use super::set_env_value;

    #[test]
    fn set_and_clear_env_value() {
        let updated = set_env_value(
            "A=1\nQINGJIAN_VOICE_API_KEY=\"old\"\n",
            "QINGJIAN_VOICE_API_KEY",
            "new",
        );
        assert!(updated.contains("QINGJIAN_VOICE_API_KEY=\"new\""));
        assert!(updated.contains("A=1"));
        assert_eq!(updated.matches("QINGJIAN_VOICE_API_KEY").count(), 1);
        let cleared = set_env_value(&updated, "QINGJIAN_VOICE_API_KEY", "");
        assert!(!cleared.contains("QINGJIAN_VOICE_API_KEY"));
        assert!(cleared.contains("A=1"));
    }
}
