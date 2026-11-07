//! 私密输入的单任务许可，防止重复按键与自动填入争抢同一内存槽。
mod gate;
mod permit;

pub use gate::InputGate;
pub use permit::InputPermit;
