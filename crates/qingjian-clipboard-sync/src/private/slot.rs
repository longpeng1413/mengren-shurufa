//! 单次输入槽和单调时钟过期时间。

pub(crate) struct Slot {
    pub secret: super::Secret,

    pub expires: std::time::Instant,
}
