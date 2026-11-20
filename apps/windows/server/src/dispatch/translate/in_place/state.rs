//! 原地翻译（双击右 Shift，或 Ctrl+Alt+T 时输入法读到空选区）在取选中文字时的进行态。

use std::time::Instant;

use qingjian_platform::protocol::SessionId;

/// 读选区走到哪一步了。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Stage {
    /// 等这个会话下一次空闲同步时让 DLL 按「翻译选中文字」读；`sent` 是请求号已发出。
    Tsf { sent: bool },

    /// 输入法读不到，界面层在用辅助接口 / 模拟复制取。
    Probe,

    /// 已取到文字并发出翻译，等 DLL 下一次空闲同步进入评审态。
    Start,
}

#[derive(Debug, Clone, Copy)]
pub(in crate::dispatch) struct InPlace {
    pub(super) session: SessionId,

    pub(super) request: u64,

    pub(super) stage: Stage,

    /// 这一步的到点时刻：读不到就换下一步，评审没接上就放弃。
    pub(super) deadline: Instant,
}
