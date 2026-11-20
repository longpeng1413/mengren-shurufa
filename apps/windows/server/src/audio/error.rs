//! 录音失败的分类：给用户看的一句话（按界面语言）与写日志的细节（哪一步、HRESULT）分开。

use std::fmt;

use qingjian_platform::windows_text::tr;
use windows::Win32::Media::Audio::{AUDCLNT_E_DEVICE_IN_USE, AUDCLNT_E_DEVICE_INVALIDATED};
use windows::core::HRESULT;

/// 拒绝访问：隐私设置里关了「允许桌面应用访问麦克风」时，Activate / Initialize 回这个。
const E_ACCESSDENIED: HRESULT = HRESULT(0x8007_0005_u32 as i32);

/// 没有可用的录音设备（GetDefaultAudioEndpoint 找不到）。
const E_NOTFOUND: HRESULT = HRESULT(0x8007_0490_u32 as i32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureErrorKind {
    /// 没有录音设备，或设备刚被拔掉。
    NoDevice,

    /// 系统隐私设置拒绝访问。
    Denied,

    /// 被别的程序独占。
    Busy,

    /// 设备格式读不懂（不是 PCM / 浮点）。
    Format,

    /// 录到一半设备断开或读包出错。
    Lost,

    /// 其他打不开的情况。
    Failed,
}

#[derive(Debug, Clone)]
pub struct CaptureError {
    pub kind: CaptureErrorKind,

    /// 出错的步骤，如 `Initialize`，只进日志。
    step: &'static str,

    code: Option<HRESULT>,
}

impl CaptureError {
    pub fn new(kind: CaptureErrorKind, step: &'static str) -> Self {
        Self {
            kind,
            step,
            code: None,
        }
    }

    /// 按 HRESULT 归类；`fallback` 是认不出时的类别（打开阶段用 `Failed`，录音中用 `Lost`）。
    pub fn from_windows(
        step: &'static str,
        error: &windows::core::Error,
        fallback: CaptureErrorKind,
    ) -> Self {
        let code = error.code();
        let kind = if code == E_ACCESSDENIED {
            CaptureErrorKind::Denied
        } else if code == AUDCLNT_E_DEVICE_IN_USE {
            CaptureErrorKind::Busy
        } else if code == E_NOTFOUND || code == AUDCLNT_E_DEVICE_INVALIDATED {
            if fallback == CaptureErrorKind::Lost {
                CaptureErrorKind::Lost
            } else {
                CaptureErrorKind::NoDevice
            }
        } else {
            fallback
        };
        Self {
            kind,
            step,
            code: Some(code),
        }
    }

    /// 面板上给用户看的一句话；认不出的情况带上错误码，方便用户报问题。
    pub fn message(&self) -> String {
        let text = tr(match self.kind {
            CaptureErrorKind::NoDevice => "mic_none",
            CaptureErrorKind::Denied => "mic_denied",
            CaptureErrorKind::Busy => "mic_busy",
            CaptureErrorKind::Format => "mic_format",
            CaptureErrorKind::Lost => "mic_lost",
            CaptureErrorKind::Failed => "mic",
        });
        match (self.kind, self.code) {
            (CaptureErrorKind::Failed | CaptureErrorKind::Format, Some(code)) => {
                format!("{text}（0x{:08X}）", code.0 as u32)
            }
            _ => text.to_owned(),
        }
    }
}

impl fmt::Display for CaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.code {
            Some(code) => write!(
                f,
                "{} failed: {:?} hr=0x{:08X}",
                self.step, self.kind, code.0 as u32
            ),
            None => write!(f, "{} failed: {:?}", self.step, self.kind),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CaptureError, CaptureErrorKind, E_ACCESSDENIED, E_NOTFOUND};
    use windows::Win32::Media::Audio::{AUDCLNT_E_DEVICE_IN_USE, AUDCLNT_E_UNSUPPORTED_FORMAT};

    #[test]
    fn classifies_privacy_busy_missing_and_unknown_codes() {
        let kind = |code, fallback| {
            CaptureError::from_windows("Initialize", &windows::core::Error::from(code), fallback)
                .kind
        };
        let open = CaptureErrorKind::Failed;
        assert_eq!(kind(E_ACCESSDENIED, open), CaptureErrorKind::Denied);
        assert_eq!(kind(AUDCLNT_E_DEVICE_IN_USE, open), CaptureErrorKind::Busy);
        assert_eq!(kind(E_NOTFOUND, open), CaptureErrorKind::NoDevice);
        assert_eq!(
            kind(E_NOTFOUND, CaptureErrorKind::Lost),
            CaptureErrorKind::Lost
        );
        assert_eq!(kind(AUDCLNT_E_UNSUPPORTED_FORMAT, open), open);
        let unknown = CaptureError::from_windows(
            "Initialize",
            &windows::core::Error::from(AUDCLNT_E_UNSUPPORTED_FORMAT),
            open,
        );
        assert!(unknown.message().contains("0x88890008"));
        assert!(unknown.to_string().contains("Initialize"));
    }
}
