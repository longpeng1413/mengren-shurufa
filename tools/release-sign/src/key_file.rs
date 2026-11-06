//! 发版密钥文件；Windows 的 .dpapi 文件只对当前系统用户可解密，禁止写明文临时文件。

use std::io::Write;
use std::path::Path;

pub fn write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let bytes = protect(path, bytes, true)?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)?.write_all(&bytes)
}

pub fn read(path: &Path) -> std::io::Result<Vec<u8>> {
    protect(path, &std::fs::read(path)?, false)
}

fn protect(path: &Path, bytes: &[u8], encrypt: bool) -> std::io::Result<Vec<u8>> {
    if path
        .extension()
        .is_some_and(|extension| extension == "dpapi")
    {
        #[cfg(windows)]
        return crypt(bytes, encrypt);
        #[cfg(not(windows))]
        return Err(std::io::Error::other(
            "DPAPI requires the original Windows user",
        ));
    }
    let _ = encrypt;
    Ok(bytes.to_vec())
}

#[cfg(windows)]
fn crypt(bytes: &[u8], encrypt: bool) -> std::io::Result<Vec<u8>> {
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
    };
    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len().try_into().map_err(std::io::Error::other)?,
        pbData: bytes.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    // SAFETY: 输入覆盖调用期；系统分配的输出复制后用 LocalFree 释放。
    unsafe {
        let result = if encrypt {
            CryptProtectData(
                &input,
                windows::core::PCWSTR::null(),
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                None,
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        };
        result.map_err(std::io::Error::other)?;
        let result = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(output.pbData.cast())));
        Ok(result)
    }
}
