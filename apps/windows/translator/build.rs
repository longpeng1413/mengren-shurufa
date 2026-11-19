//! 原生控件视觉样式、逐显示器 DPI 和程序图标。
fn main() {
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        embed_manifest::embed_manifest(embed_manifest::new_manifest("Mengren.Translator"))
            .expect("translator manifest");
        #[cfg(windows)]
        winresource::WindowsResource::new()
            .set_icon("../tsf/resources/qingjian.ico")
            .compile()
            .expect("translator icon");
    }
    println!("cargo:rerun-if-changed=../tsf/resources/qingjian.ico");
    println!("cargo:rerun-if-changed=build.rs");
}
