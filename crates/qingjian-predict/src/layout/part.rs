//! 版式模板里的一段：原样保留，或是要翻译的正文。

/// 模板的组成部分，按原文顺序排列。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Part {
    /// 换行、缩进、列表记号、标记、纯符号数字这类不翻译的部分。
    Keep(String),

    /// 要翻译的正文，不含首尾空白与换行。
    Text(String),
}
