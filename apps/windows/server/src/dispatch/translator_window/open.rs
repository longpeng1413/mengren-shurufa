//! Router 交给界面层的「怎么开翻译窗口」：带文字、让界面层自己取选中文字，或只开 / 收窗口。

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranslatorOpen {
    /// 输入法已读到的选中文字：放进原文框立即翻译。
    Text(String),

    /// 输入法读不到或说没选中：界面层先用系统辅助接口取，再模拟 Ctrl+Insert 从剪贴板取。
    /// 「没选中」也要去取：微信、原生 EDIT 这类经兼容层的程序报的选区总是空的（2026-11-20 实测）。
    Probe,

    /// 不读（私密输入），只打开 / 收起。
    Plain,
}
