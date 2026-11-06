//! Windows 自更新：仅接收已校验安装包的本机路径，由系统处理安装权限确认。

#[cfg(windows)]
pub fn launch(path: &std::path::Path) -> Result<(), String> {
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::{HSTRING, w};
    let result = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            &HSTRING::from(path.as_os_str()),
            w!("/SILENT /SUPPRESSMSGBOXES /NORESTART /SP-"),
            None,
            SW_SHOWNORMAL,
        )
    };
    if result.0 as isize <= 32 {
        Err(crate::i18n::tr("update.install_failed").to_owned())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
pub fn launch(_path: &std::path::Path) -> Result<(), String> {
    Err(crate::i18n::tr("update.install_failed").to_owned())
}
