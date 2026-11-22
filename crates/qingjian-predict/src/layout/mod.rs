//! 翻译时保留原文版式：换行、空行、缩进、行首列表记号与 `<tag>` 这类标记原样留下，只把正文交给翻译服务。
//! 有道这类整段翻译的接口会把标记当文字译坏（`</prompt>` 变成 `< /提示>`），所以先拆后拼。

mod part;
mod template;

pub use self::template::Template;

#[cfg(test)]
mod tests;
