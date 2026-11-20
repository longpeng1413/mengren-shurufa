//! 打开默认麦克风并初始化共享模式录音。
//!
//! 共享模式不带转换标志时，Initialize 只接受与设备混音格式同采样率、同声道数的格式，
//! 笔记本阵列麦克风多是 48 kHz 多声道，直接要 16 kHz 单声道会被拒（AUDCLNT_E_UNSUPPORTED_FORMAT）。
//! 所以先请系统自动转换（AUTOCONVERTPCM），仍被拒再按混音格式录、由 [`super::format`] 与
//! [`super::resample`] 自己折成单声道 16 kHz。每一步的设备名、格式、标志与 HRESULT 都写 info 日志。

use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Media::Audio::{
    AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
    AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, IAudioClient, IMMDevice, IMMDeviceEnumerator,
    MMDeviceEnumerator, WAVEFORMATEX, WAVEFORMATEXTENSIBLE, eCapture, eCommunications, eConsole,
};
use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance, CoTaskMemFree, STGM_READ};

use super::error::{CaptureError, CaptureErrorKind};
use super::format::StreamFormat;

/// 缓冲 1 秒（100 ns 单位）。
const BUFFER: i64 = 10_000_000;

const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;

/// 初始化好的录音客户端与它交出的格式。
pub struct OpenedDevice {
    pub client: IAudioClient,

    pub format: StreamFormat,

    pub name: String,
}

/// 打开默认录音设备。调用方线程已初始化 COM。
pub unsafe fn open() -> Result<OpenedDevice, CaptureError> {
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(|error| {
                CaptureError::from_windows("CoCreateInstance", &error, CaptureErrorKind::Failed)
            })?;
        let (device, role) = default_device(&enumerator)?;
        let name = friendly_name(&device);
        let client = activate(&device)?;
        let mix = client.GetMixFormat().map_err(|error| {
            CaptureError::from_windows("GetMixFormat", &error, CaptureErrorKind::Failed)
        })?;
        let mix_format = parse(mix);
        tracing::info!(
            device = %name,
            role,
            mix = %mix_format.map_or_else(|| describe_raw(mix), |f| f.to_string()),
            "voice capture device"
        );
        let result = initialize(&device, &client, mix, mix_format);
        CoTaskMemFree(Some(mix.cast_const().cast()));
        let (client, format) = result?;
        Ok(OpenedDevice {
            client,
            format,
            name,
        })
    }
}

/// 系统设置里的「默认录音设备」（eConsole）；取不到再看「默认通信设备」。
unsafe fn default_device(
    enumerator: &IMMDeviceEnumerator,
) -> Result<(IMMDevice, &'static str), CaptureError> {
    let mut last = None;
    for (role, label) in [(eConsole, "console"), (eCommunications, "communications")] {
        match unsafe { enumerator.GetDefaultAudioEndpoint(eCapture, role) } {
            Ok(device) => return Ok((device, label)),
            Err(error) => {
                tracing::info!(role = label, hr = %hex(&error), "no default capture endpoint");
                last = Some(error);
            }
        }
    }
    let error = last.expect("at least one role was tried");
    Err(CaptureError::from_windows(
        "GetDefaultAudioEndpoint",
        &error,
        CaptureErrorKind::NoDevice,
    ))
}

unsafe fn activate(device: &IMMDevice) -> Result<IAudioClient, CaptureError> {
    unsafe { device.Activate(CLSCTX_ALL, None) }
        .map_err(|error| CaptureError::from_windows("Activate", &error, CaptureErrorKind::Failed))
}

/// 先请系统转成 16 kHz 单声道；被拒就换一个新客户端按混音格式初始化。
unsafe fn initialize(
    device: &IMMDevice,
    client: &IAudioClient,
    mix: *const WAVEFORMATEX,
    mix_format: Option<StreamFormat>,
) -> Result<(IAudioClient, StreamFormat), CaptureError> {
    let target = StreamFormat::TARGET;
    let wanted = WAVEFORMATEX {
        wFormatTag: 1,
        nChannels: target.channels,
        nSamplesPerSec: target.rate,
        nAvgBytesPerSec: target.rate * u32::from(target.block_align),
        nBlockAlign: target.block_align,
        wBitsPerSample: 16,
        cbSize: 0,
    };
    let flags = AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY;
    let converted = if skip_system_conversion() {
        Err(windows::core::Error::from(
            windows::Win32::Media::Audio::AUDCLNT_E_UNSUPPORTED_FORMAT,
        ))
    } else {
        unsafe { client.Initialize(AUDCLNT_SHAREMODE_SHARED, flags, BUFFER, 0, &wanted, None) }
    };
    tracing::info!(
        flags = format!("0x{flags:08X}"),
        format = %target,
        hr = %converted.as_ref().map_or_else(hex, |()| "0x00000000".to_owned()),
        "voice capture Initialize (system conversion)"
    );
    let first = match converted {
        Ok(()) => return Ok((client.clone(), target)),
        Err(error) => error,
    };
    let first = CaptureError::from_windows("Initialize", &first, CaptureErrorKind::Failed);
    // 拒绝访问、被独占这类换格式也没用，直接报
    if first.kind != CaptureErrorKind::Failed {
        return Err(first);
    }
    let Some(format) = mix_format else {
        return Err(CaptureError::new(CaptureErrorKind::Format, "GetMixFormat"));
    };
    // 一个客户端 Initialize 失败后不保证能再初始化，换新的
    let fresh = unsafe { activate(device) }?;
    let native = unsafe { fresh.Initialize(AUDCLNT_SHAREMODE_SHARED, 0, BUFFER, 0, mix, None) };
    tracing::info!(
        flags = "0x00000000",
        format = %format,
        hr = %native.as_ref().map_or_else(hex, |()| "0x00000000".to_owned()),
        "voice capture Initialize (mix format, converted in process)"
    );
    native
        .map(|()| (fresh, format))
        .map_err(|error| CaptureError::from_windows("Initialize", &error, CaptureErrorKind::Failed))
}

unsafe fn parse(mix: *const WAVEFORMATEX) -> Option<StreamFormat> {
    if mix.is_null() {
        return None;
    }
    // 结构体按 1 字节对齐，逐字段按值读
    let header = unsafe { mix.read_unaligned() };
    let tag = header.wFormatTag;
    // 扩展头里 32 位容器只有 24 位有效时值在高位，按 32 位整数换算结果一样，不必看有效位
    let sub = (tag == WAVE_FORMAT_EXTENSIBLE && header.cbSize >= 22)
        .then(|| unsafe { mix.cast::<WAVEFORMATEXTENSIBLE>().read_unaligned() }.SubFormat);
    StreamFormat::from_parts(
        tag,
        header.nChannels,
        header.nSamplesPerSec,
        header.nBlockAlign,
        header.wBitsPerSample,
        sub,
    )
}

/// 认不出的格式也把原始字段写进日志，方便对着设备排查。
unsafe fn describe_raw(mix: *const WAVEFORMATEX) -> String {
    if mix.is_null() {
        return "null".to_owned();
    }
    let header = unsafe { mix.read_unaligned() };
    let (tag, channels, rate, bits) = (
        header.wFormatTag,
        header.nChannels,
        header.nSamplesPerSec,
        header.wBitsPerSample,
    );
    format!("unsupported tag=0x{tag:04X} {rate} Hz x {channels} ch {bits} bit")
}

/// 设备在系统声音设置里显示的名字；取不到不影响录音。
unsafe fn friendly_name(device: &IMMDevice) -> String {
    let read = || -> windows::core::Result<String> {
        unsafe {
            let store = device.OpenPropertyStore(STGM_READ)?;
            let value = store.GetValue(&PKEY_Device_FriendlyName)?;
            let text = PropVariantToStringAlloc(&value)?;
            let name = text.to_string().unwrap_or_default();
            CoTaskMemFree(Some(text.0.cast_const().cast()));
            Ok(name)
        }
    };
    read().unwrap_or_else(|error| format!("<unknown: {}>", hex(&error)))
}

/// 实机测试用：跳过系统转换，强制走混音格式 + 进程内转换那条路。
#[cfg(test)]
pub static FORCE_MIX_FORMAT: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

fn skip_system_conversion() -> bool {
    #[cfg(test)]
    {
        FORCE_MIX_FORMAT.load(std::sync::atomic::Ordering::Relaxed)
    }
    #[cfg(not(test))]
    {
        false
    }
}

fn hex(error: &windows::core::Error) -> String {
    format!("0x{:08X}", error.code().0 as u32)
}
