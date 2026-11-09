//! 加载好的 SenseVoice 识别器：一次把一整段录音转成文字（非流式）。

use std::ffi::{CStr, CString, c_void};
use std::path::Path;
use std::ptr::NonNull;
use std::sync::Mutex;

use crate::OfflineError;
use crate::library::SherpaApi;
use crate::library::config::PaddedConfig;

pub struct Recognizer {
    api: &'static SherpaApi,

    raw: NonNull<c_void>,

    /// 同一个识别器不并发解码。
    lock: Mutex<()>,
}

// SAFETY: sherpa-onnx 的识别器对象不绑定线程；并发解码由 `lock` 串行化
unsafe impl Send for Recognizer {}
// SAFETY: 同上，所有经 `&self` 的调用都先拿 `lock`
unsafe impl Sync for Recognizer {}

impl Recognizer {
    /// 按模型文件建识别器；模型约 240 MB，手机上要一两秒，在后台线程调。
    pub fn new(
        api: &'static SherpaApi,
        model: &Path,
        tokens: &Path,
        threads: i32,
    ) -> Result<Self, OfflineError> {
        let model = path_string(model)?;
        let tokens = path_string(tokens)?;
        let mut padded = PaddedConfig::zeroed();
        let config = &mut padded.config;
        config.feat.sample_rate = SAMPLE_RATE as i32;
        config.feat.feature_dim = 80;
        config.model.sense_voice.model = model.as_ptr();
        config.model.sense_voice.use_itn = 1;
        config.model.tokens = tokens.as_ptr();
        config.model.num_threads = threads.max(1);
        config.model.provider = c"cpu".as_ptr();
        config.decoding_method = c"greedy_search".as_ptr();
        // SAFETY: 配置里的字符串在调用期间活着（库会自己拷一份）；返回空指针表示失败
        let raw = unsafe { (api.create_recognizer)(&padded.config) };
        NonNull::new(raw)
            .map(|raw| Self {
                api,
                raw,
                lock: Mutex::new(()),
            })
            .ok_or(OfflineError::CreateFailed)
    }

    /// 识别一段（-1..1 的浮点采样）。
    pub fn decode(&self, sample_rate: u32, samples: &[f32]) -> Result<String, OfflineError> {
        if samples.is_empty() {
            return Ok(String::new());
        }
        let length = i32::try_from(samples.len())
            .map_err(|_| OfflineError::Decode("audio too long".to_owned()))?;
        let _guard = self
            .lock
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        // SAFETY: 流由这个识别器建、在本函数里销毁；采样切片在调用期间有效
        unsafe {
            let stream = (self.api.create_stream)(self.raw.as_ptr());
            if stream.is_null() {
                return Err(OfflineError::Decode("cannot create stream".to_owned()));
            }
            (self.api.accept_waveform)(stream, sample_rate as i32, samples.as_ptr(), length);
            (self.api.decode)(self.raw.as_ptr(), stream);
            let json = (self.api.result_json)(stream);
            let text = if json.is_null() {
                Err(OfflineError::Decode("no result".to_owned()))
            } else {
                let text = text_of(CStr::from_ptr(json).to_bytes());
                (self.api.destroy_result_json)(json);
                text
            };
            (self.api.destroy_stream)(stream);
            text
        }
    }
}

impl Drop for Recognizer {
    fn drop(&mut self) {
        // SAFETY: raw 由 create_recognizer 返回，只在这里销毁一次
        unsafe { (self.api.destroy_recognizer)(self.raw.as_ptr()) }
    }
}

/// 结果 JSON 里的 `text`（SenseVoice 的语种、情绪标签已由库去掉）。
fn text_of(json: &[u8]) -> Result<String, OfflineError> {
    let value: serde_json::Value =
        serde_json::from_slice(json).map_err(|error| OfflineError::Decode(error.to_string()))?;
    Ok(value
        .get("text")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_owned())
}

fn path_string(path: &Path) -> Result<CString, OfflineError> {
    let text = path.to_str().ok_or(OfflineError::ModelMissing)?;
    CString::new(text).map_err(|_| OfflineError::ModelMissing)
}

/// SenseVoice 的特征按 16 kHz 算；别的采样率的录音由库内部重采样。
pub const SAMPLE_RATE: u32 = 16_000;

#[cfg(test)]
mod tests {
    use super::text_of;

    #[test]
    fn reads_text_from_result_json() {
        let json = r#"{"lang":"<|zh|>","emotion":"<|NEUTRAL|>","text":" 你好。 ","tokens":[]}"#;
        assert_eq!(text_of(json.as_bytes()).unwrap(), "你好。");
        assert_eq!(text_of(b"{}").unwrap(), "");
        assert!(text_of(b"not json").is_err());
    }
}
