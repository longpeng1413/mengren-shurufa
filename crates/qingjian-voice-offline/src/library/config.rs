//! sherpa-onnx C 接口 `SherpaOnnxOfflineRecognizerConfig` 的内存布局，逐字段对齐随包的 2.0.1 版（`c-api.h`）。
//! 库是按指针整份读这个结构体的，少一个字段后面全错位，所以用不上的模型也要占位；换库版本要对着新头文件重核，
//! 并改 [`super::EXPECTED_VERSION`]。全零是合法的缺省值（空指针 = 不用，0 = 用库里的缺省）。

use std::ffi::c_char;

type Text = *const c_char;

#[repr(C)]
pub struct FeatureConfig {
    pub sample_rate: i32,

    pub feature_dim: i32,
}

#[repr(C)]
pub struct TransducerConfig {
    pub encoder: Text,

    pub decoder: Text,

    pub joiner: Text,
}

/// 只有一个模型文件的那几种（paraformer、nemo ctc、tdnn、dolphin、zipformer ctc 等）。
#[repr(C)]
pub struct SingleModelConfig {
    pub model: Text,
}

#[repr(C)]
pub struct WhisperConfig {
    pub encoder: Text,

    pub decoder: Text,

    pub language: Text,

    pub task: Text,

    pub tail_paddings: i32,

    pub enable_token_timestamps: i32,

    pub enable_segment_timestamps: i32,
}

#[repr(C)]
pub struct SenseVoiceConfig {
    pub model: Text,

    /// 空指针 / 空串是自动判断语种。
    pub language: Text,

    /// 1：数字写成阿拉伯数字、加标点（逆文本规整）。
    pub use_itn: i32,
}

#[repr(C)]
pub struct MoonshineConfig {
    pub preprocessor: Text,

    pub encoder: Text,

    pub uncached_decoder: Text,

    pub cached_decoder: Text,

    pub merged_decoder: Text,
}

#[repr(C)]
pub struct EncoderDecoderConfig {
    pub encoder: Text,

    pub decoder: Text,
}

#[repr(C)]
pub struct CanaryConfig {
    pub encoder: Text,

    pub decoder: Text,

    pub src_lang: Text,

    pub tgt_lang: Text,

    pub use_pnc: i32,
}

#[repr(C)]
pub struct FunAsrNanoConfig {
    pub encoder_adaptor: Text,

    pub llm: Text,

    pub embedding: Text,

    pub tokenizer: Text,

    pub system_prompt: Text,

    pub user_prompt: Text,

    pub max_new_tokens: i32,

    pub temperature: f32,

    pub top_p: f32,

    pub seed: i32,

    pub language: Text,

    pub itn: i32,

    pub hotwords: Text,
}

#[repr(C)]
pub struct Qwen3AsrConfig {
    pub conv_frontend: Text,

    pub encoder: Text,

    pub decoder: Text,

    pub tokenizer: Text,

    pub max_total_len: i32,

    pub max_new_tokens: i32,

    pub temperature: f32,

    pub top_p: f32,

    pub seed: i32,

    pub hotwords: Text,
}

#[repr(C)]
pub struct CohereTranscribeConfig {
    pub encoder: Text,

    pub decoder: Text,

    pub language: Text,

    pub use_punct: i32,

    pub use_itn: i32,
}

#[repr(C)]
pub struct ModelConfig {
    pub transducer: TransducerConfig,

    pub paraformer: SingleModelConfig,

    pub nemo_ctc: SingleModelConfig,

    pub whisper: WhisperConfig,

    pub tdnn: SingleModelConfig,

    pub tokens: Text,

    pub num_threads: i32,

    pub debug: i32,

    pub provider: Text,

    pub model_type: Text,

    pub modeling_unit: Text,

    pub bpe_vocab: Text,

    pub telespeech_ctc: Text,

    pub sense_voice: SenseVoiceConfig,

    pub moonshine: MoonshineConfig,

    pub fire_red_asr: EncoderDecoderConfig,

    pub dolphin: SingleModelConfig,

    pub zipformer_ctc: SingleModelConfig,

    pub canary: CanaryConfig,

    pub wenet_ctc: SingleModelConfig,

    pub omnilingual: SingleModelConfig,

    pub medasr: SingleModelConfig,

    pub funasr_nano: FunAsrNanoConfig,

    pub fire_red_asr_ctc: SingleModelConfig,

    pub qwen3_asr: Qwen3AsrConfig,

    pub cohere_transcribe: CohereTranscribeConfig,
}

#[repr(C)]
pub struct LmConfig {
    pub model: Text,

    pub scale: f32,
}

#[repr(C)]
pub struct HomophoneReplacerConfig {
    pub dict_dir: Text,

    pub lexicon: Text,

    pub rule_fsts: Text,
}

#[repr(C)]
pub struct CtcFstDecoderConfig {
    pub graph: Text,

    pub max_active: i32,
}

#[repr(C)]
pub struct RecognizerConfig {
    pub feat: FeatureConfig,

    pub model: ModelConfig,

    pub lm: LmConfig,

    pub decoding_method: Text,

    pub max_active_paths: i32,

    pub hotwords_file: Text,

    pub hotwords_score: f32,

    pub rule_fsts: Text,

    pub rule_fars: Text,

    pub blank_penalty: f32,

    pub hr: HomophoneReplacerConfig,

    pub ctc_fst_decoder: CtcFstDecoderConfig,
}

/// 结构体后面再垫一段零：万一库比这里多读几个尾部字段，读到的也是「不用」。
#[repr(C)]
pub struct PaddedConfig {
    pub config: RecognizerConfig,

    pub padding: [u8; 512],
}

impl PaddedConfig {
    /// 全零的一份（所有模型都不用、所有参数取库的缺省）。
    pub fn zeroed() -> Box<Self> {
        // SAFETY: 结构体里只有裸指针、整数、浮点与字节数组，全零位模式对它们都合法（空指针、0、0.0）
        Box::new(unsafe { std::mem::zeroed() })
    }
}
