//! 模型里的一个文件：名字、字节数与 SHA-256，下载完逐一核对。

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelFile {
    pub name: &'static str,

    pub size: u64,

    /// 小写十六进制。
    pub sha256: &'static str,
}
