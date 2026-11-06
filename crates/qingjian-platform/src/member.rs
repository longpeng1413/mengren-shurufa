//! Windows 会员凭据：使用当前 Windows 用户的 DPAPI 加密，不进入配置导出和日志。

use qingjian_predict::subscription::Account;
use std::path::Path;
use windows::Win32::Foundation::{HLOCAL, LocalFree};
use windows::Win32::Security::Cryptography::{
    CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
};

pub fn path() -> Option<std::path::PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|base| {
        std::path::PathBuf::from(base)
            .join("MengrenShurufa")
            .join("member.dpapi")
    })
}

pub struct Store;

impl qingjian_predict::subscription::CredentialStore for Store {
    fn lock(&self) -> Result<Box<dyn Send>, String> {
        let path = path()
            .ok_or("No local data directory")?
            .with_extension("lock");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|_| "Cannot create credential directory")?;
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)
            .map_err(|_| "Cannot lock credentials")?;
        file.lock().map_err(|_| "Cannot lock credentials")?;
        Ok(Box::new(file))
    }
    fn load(&self) -> Result<Account, String> {
        load(&path().ok_or("No local data directory")?)
            .map_err(|_| "ChatGPT sign-in required".to_owned())
    }
    fn save(&self, account: &Account) -> Result<(), String> {
        save(&path().ok_or("No local data directory")?, account)
            .map_err(|_| "Cannot save ChatGPT account".to_owned())
    }
}

pub fn install() {
    qingjian_predict::subscription::install_store(std::sync::Arc::new(Store));
    qingjian_predict::youdao::install_secret_loader(load_youdao_secret);
}

pub fn save_youdao_secret(secret: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let path = path()
        .ok_or("No local data directory")?
        .with_file_name("youdao.dpapi");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, crypt(secret.as_bytes(), true)?)?;
    Ok(())
}

fn load_youdao_secret() -> Option<String> {
    let path = path()?.with_file_name("youdao.dpapi");
    String::from_utf8(crypt(&std::fs::read(path).ok()?, false).ok()?).ok()
}

/// 中转令牌独立于会员和有道，不写入普通配置或导出文件。
pub fn save_relay_secret(secret: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if !(24..=256).contains(&secret.len()) || !secret.bytes().all(|byte| byte.is_ascii_graphic()) {
        return Err("Invalid relay access token".into());
    }
    let path = path()
        .ok_or("No local data directory")?
        .with_file_name("clipboard-relay.dpapi");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, crypt(secret.as_bytes(), true)?)?;
    std::fs::rename(temporary, path)?;
    Ok(())
}

pub fn load_relay_secret() -> Option<String> {
    let path = path()?.with_file_name("clipboard-relay.dpapi");
    String::from_utf8(crypt(&std::fs::read(path).ok()?, false).ok()?).ok()
}

pub fn clear_relay_secret() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let path = path()
        .ok_or("No local data directory")?
        .with_file_name("clipboard-relay.dpapi");
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// 将整套凭据加密后替换磁盘记录；不会创建明文临时文件。
pub fn save(
    path: &Path,
    account: &Account,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let plain = serde_json::to_vec(account)?;
    let encrypted = crypt(&plain, true)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension("tmp");
    std::fs::write(&temp, encrypted)?;
    std::fs::rename(&temp, path)?;
    Ok(())
}

pub fn load(path: &Path) -> Result<Account, Box<dyn std::error::Error + Send + Sync>> {
    Ok(serde_json::from_slice(&crypt(
        &std::fs::read(path)?,
        false,
    )?)?)
}

fn crypt(input: &[u8], encrypt: bool) -> Result<Vec<u8>, windows::core::Error> {
    let input = CRYPT_INTEGER_BLOB {
        cbData: input.len().try_into().map_err(|_| {
            windows::core::Error::from_hresult(windows::Win32::Foundation::E_INVALIDARG)
        })?,
        pbData: input.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    // SAFETY: input 在系统调用期间有效；DPAPI 分配的输出在复制后用 LocalFree 释放。
    unsafe {
        if encrypt {
            CryptProtectData(
                &input,
                windows::core::PCWSTR::null(),
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )?;
        } else {
            CryptUnprotectData(
                &input,
                None,
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )?;
        }
        let result = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(output.pbData.cast())));
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn dpapi_round_trip_and_corrupt_data() {
        let encrypted = super::crypt(b"member test credential", true).unwrap();
        assert_ne!(encrypted, b"member test credential");
        assert_eq!(
            super::crypt(&encrypted, false).unwrap(),
            b"member test credential"
        );
        assert!(super::crypt(b"invalid", false).is_err());
    }
}
