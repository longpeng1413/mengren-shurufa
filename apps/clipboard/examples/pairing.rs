//! 隔离的真实设备页输入回归窗口：不监听剪贴板、不注册快捷键、不连接或修改真实配对。

#![allow(dead_code, unused_imports)]

#[cfg(windows)]
#[path = "../src/app/mod.rs"]
mod app;
#[cfg(windows)]
#[path = "../src/i18n.rs"]
mod i18n;
#[cfg(windows)]
#[path = "../src/link/mod.rs"]
mod link;
#[cfg(windows)]
#[path = "../src/palette/mod.rs"]
mod palette;
#[cfg(windows)]
#[path = "../src/picture.rs"]
mod picture;
#[cfg(windows)]
#[path = "../src/system/mod.rs"]
mod system;
#[cfg(windows)]
#[path = "../src/view/mod.rs"]
mod view;

#[cfg(windows)]
struct Preview(app::ClipboardApp);

#[cfg(windows)]
impl bamboo_slip::App for Preview {
    type Message = app::Message;

    fn view(
        &self,
        ui: &mut bamboo_slip::Ui<Self::Message>,
        scale: f32,
        size: (u32, u32),
    ) -> Result<bamboo_slip::taffy::NodeId, bamboo_slip::RenderError> {
        self.0.view(ui, scale, size)
    }

    fn update(&mut self, message: Self::Message) -> bool {
        self.0.update(message)
    }

    fn setup(&mut self, text: &mut bamboo_slip::TextPainter) {
        self.0.setup(text);
    }
}

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use bamboo_slip::{App, TextEdit, WindowOptions};
    use qingjian_clipboard::{Capture, ClipFilter, ClipSettings, ClipStore};
    let clipboard = !std::env::args().any(|arg| arg == "--pairing");
    let directory = std::env::temp_dir().join(format!("mengren-pair-ui-{}", std::process::id()));
    std::fs::create_dir_all(&directory)?;
    let database = directory.join("history.db");
    let mut store = ClipStore::open(&database, 20)?;
    {
        for index in 1..=10 {
            let text = if index == 9 {
                "较长的合成测试内容，用来检查文字折行和预览的高度。".repeat(20)
            } else {
                format!("第 {index} 条测试内容：你好")
            };
            store.add(
                &Capture::text(text, "Synthetic fixture".into(), vec![]),
                index,
            )?;
        }
    }
    let mut model = app::ClipboardApp::new(
        store,
        &database,
        system::Watch {
            path: None,
            settings: ClipSettings::default(),
            managed: false,
        },
        None,
        Some(true),
        false,
    );
    model.update(app::Message::Filter(ClipFilter::All));
    model.showing_devices = !clipboard;
    model.pairing = link::Pairing::Entering {
        address: TextEdit::new("synthetic-test-only"),
        code: TextEdit::new(""),
    };
    bamboo_slip::run_window(
        Preview(model),
        WindowOptions::new("猛人隔离界面回归（无网络）", (420.0, 520.0)),
    )?;
    Ok(())
}

#[cfg(not(windows))]
fn main() {}
