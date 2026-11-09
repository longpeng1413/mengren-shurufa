//! 壳经通道交给识别线程的指令。

pub enum Command {
    Audio(Vec<i16>),

    /// 说完了。
    Finish,
}
