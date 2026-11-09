//! 运行时加载 sherpa-onnx 的 C 接口库（dlopen / LoadLibrary），不在链接期依赖：库没打进包时输入法照常运行，只是离线识别不可用。
//! 安卓的库随 APK 放在 `jniLibs`，Gradle 构建时从 pub.dev 的 `sherpa_onnx_android_arm64` 包取（版本与哈希见 `apps/android/app/build.gradle.kts`）。

pub mod config;

use std::ffi::{CStr, c_char, c_void};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use libloading::Library;

use crate::OfflineError;
use config::RecognizerConfig;

/// 随包库的版本；结构体布局（[`config`]）按它核过。库报的版本不是它就不用，免得按错位的结构体读配置而崩溃。
pub const EXPECTED_VERSION: &str = "2.0.1";

type CreateRecognizer = unsafe extern "C" fn(*const RecognizerConfig) -> *mut c_void;
type DestroyRecognizer = unsafe extern "C" fn(*mut c_void);
type CreateStream = unsafe extern "C" fn(*mut c_void) -> *mut c_void;
type DestroyStream = unsafe extern "C" fn(*mut c_void);
type AcceptWaveform = unsafe extern "C" fn(*mut c_void, i32, *const f32, i32);
type Decode = unsafe extern "C" fn(*mut c_void, *mut c_void);
type ResultJson = unsafe extern "C" fn(*mut c_void) -> *const c_char;
type DestroyResultJson = unsafe extern "C" fn(*const c_char);
type Version = unsafe extern "C" fn() -> *const c_char;

/// 加载好的库与用到的几个函数。进程里只加载一次，之后一直留着（卸载 onnxruntime 不安全，也省不了多少内存）。
pub struct SherpaApi {
    pub create_recognizer: CreateRecognizer,

    pub destroy_recognizer: DestroyRecognizer,

    pub create_stream: CreateStream,

    pub destroy_stream: DestroyStream,

    pub accept_waveform: AcceptWaveform,

    pub decode: Decode,

    pub result_json: ResultJson,

    pub destroy_result_json: DestroyResultJson,

    pub version: String,

    /// 函数指针指向这两个库里，必须跟着活着。
    _library: Library,

    _onnxruntime: Option<Library>,
}

/// 进程里那一份；第一次调用时按 `library_dir` 找库，之后传什么都用第一次的结果。
pub fn api(library_dir: Option<&Path>) -> Result<&'static SherpaApi, OfflineError> {
    static API: OnceLock<Result<SherpaApi, String>> = OnceLock::new();
    API.get_or_init(|| load(library_dir).map_err(|error| error.to_string()))
        .as_ref()
        .map_err(|error| OfflineError::Library(error.clone()))
}

fn load(library_dir: Option<&Path>) -> Result<SherpaApi, OfflineError> {
    // Windows 11 的 System32 带一份旧的 onnxruntime.dll：先按完整路径把随包那份载进来，c-api 再找依赖时就用它
    let onnxruntime = library_dir.and_then(|dir| open(&dir.join(ONNXRUNTIME)).ok());
    let library = candidates(library_dir)
        .iter()
        .find_map(|path| open(path).ok())
        .ok_or_else(|| OfflineError::Library(format!("cannot load {SHERPA}")))?;
    // SAFETY: 名字与签名对照随包版本的 c-api.h；下面先核对版本，版本不对就不用这些指针
    unsafe {
        let version: Version = *library.get::<Version>(b"SherpaOnnxGetVersionStr\0")?;
        let raw = version();
        let version = if raw.is_null() {
            String::new()
        } else {
            CStr::from_ptr(raw).to_string_lossy().into_owned()
        };
        if version != EXPECTED_VERSION {
            return Err(OfflineError::Library(format!(
                "unexpected sherpa-onnx version {version}, expected {EXPECTED_VERSION}"
            )));
        }
        Ok(SherpaApi {
            create_recognizer: *library.get(b"SherpaOnnxCreateOfflineRecognizer\0")?,
            destroy_recognizer: *library.get(b"SherpaOnnxDestroyOfflineRecognizer\0")?,
            create_stream: *library.get(b"SherpaOnnxCreateOfflineStream\0")?,
            destroy_stream: *library.get(b"SherpaOnnxDestroyOfflineStream\0")?,
            accept_waveform: *library.get(b"SherpaOnnxAcceptWaveformOffline\0")?,
            decode: *library.get(b"SherpaOnnxDecodeOfflineStream\0")?,
            result_json: *library.get(b"SherpaOnnxGetOfflineStreamResultAsJson\0")?,
            destroy_result_json: *library.get(b"SherpaOnnxDestroyOfflineStreamResultJson\0")?,
            version,
            _library: library,
            _onnxruntime: onnxruntime,
        })
    }
}

/// 先按名字找（安卓的库在 APK 里，不一定解压到磁盘，只能交给系统按名字找），再按给定目录的完整路径找。
fn candidates(library_dir: Option<&Path>) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(dir) = library_dir {
        paths.push(dir.join(SHERPA));
    }
    if cfg!(target_os = "android") || library_dir.is_none() {
        paths.insert(0, PathBuf::from(SHERPA));
    }
    paths
}

fn open(path: &Path) -> Result<Library, libloading::Error> {
    // SAFETY: 载入的是 sherpa-onnx / onnxruntime 本身，它们的初始化代码没有特殊要求
    unsafe { Library::new(path) }
}

#[cfg(windows)]
const SHERPA: &str = "sherpa-onnx-c-api.dll";
#[cfg(windows)]
const ONNXRUNTIME: &str = "onnxruntime.dll";

#[cfg(target_vendor = "apple")]
const SHERPA: &str = "libsherpa-onnx-c-api.dylib";
#[cfg(target_vendor = "apple")]
const ONNXRUNTIME: &str = "libonnxruntime.dylib";

#[cfg(not(any(windows, target_vendor = "apple")))]
const SHERPA: &str = "libsherpa-onnx-c-api.so";
#[cfg(not(any(windows, target_vendor = "apple")))]
const ONNXRUNTIME: &str = "libonnxruntime.so";
