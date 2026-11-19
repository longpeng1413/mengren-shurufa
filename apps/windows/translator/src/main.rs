//! 猛人翻译窗口入口。
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
#[cfg(windows)]
mod window;
#[cfg(windows)]
fn main() -> windows::core::Result<()> {
    window::run()
}
#[cfg(not(windows))]
fn main() {
    eprintln!("仅 Windows");
}
