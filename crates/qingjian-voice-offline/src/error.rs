//! 离线识别与模型下载出错。

use thiserror::Error;

#[derive(Debug, Error)]
pub enum OfflineError {
    #[error("recognizer library unavailable: {0}")]
    Library(String),

    #[error("speech model is not downloaded")]
    ModelMissing,

    #[error("cannot create recognizer from the model")]
    CreateFailed,

    #[error("decode failed: {0}")]
    Decode(String),

    #[error("download already running")]
    Busy,

    #[error("download cancelled")]
    Cancelled,

    #[error("download failed: HTTP {0}")]
    Http(u16),

    #[error("network: {0}")]
    Network(String),

    #[error("download stalled")]
    Stalled,

    #[error("downloaded file does not match the published hash")]
    Integrity,

    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

impl OfflineError {
    /// 给用户看的一句话（`#[error]` 的文案给日志用）。
    pub fn describe(&self) -> String {
        match self {
            Self::Library(_) => "这个安装包里没有离线识别引擎，或引擎版本不对".to_owned(),
            Self::ModelMissing => "还没下载离线语音模型".to_owned(),
            Self::CreateFailed => "离线语音模型加载失败，可以删除模型后重新下载".to_owned(),
            Self::Decode(_) => "离线识别出错了".to_owned(),
            Self::Busy => "模型正在下载".to_owned(),
            Self::Cancelled => "已取消下载".to_owned(),
            Self::Http(status) => format!("下载失败（HTTP {status}）"),
            Self::Network(_) => {
                "连不上 GitHub，国内网络请先打开代理；再下载会从断点继续".to_owned()
            }
            Self::Stalled => "下载 30 秒没有进展，检查网络或代理；再下载会从断点继续".to_owned(),
            Self::Integrity => "模型文件校验不通过，已删除，请重新下载".to_owned(),
            Self::Io(_) => "读写模型文件出错，检查手机存储空间".to_owned(),
        }
    }
}

impl From<libloading::Error> for OfflineError {
    fn from(error: libloading::Error) -> Self {
        Self::Library(error.to_string())
    }
}

impl From<reqwest::Error> for OfflineError {
    fn from(error: reqwest::Error) -> Self {
        if error.is_timeout() {
            Self::Stalled
        } else if let Some(status) = error.status() {
            Self::Http(status.as_u16())
        } else {
            Self::Network(error.to_string())
        }
    }
}
