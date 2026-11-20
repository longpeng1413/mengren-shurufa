//! 翻译窗口在等选区时的状态，以及哪些会话的 DLL 能在空闲同步时读选区。

use std::collections::HashSet;
use std::time::Instant;

use qingjian_platform::protocol::SessionId;

/// 一次在等的读选区请求。
#[derive(Debug, Clone, Copy)]
pub(super) struct SelectionRead {
    pub(super) request: u64,

    pub(super) session: SessionId,

    /// 到点还没收到选区就开空窗口。
    pub(super) deadline: Instant,

    /// 请求号已随 ModeSync 发给 DLL，不再重复发。
    pub(super) sent: bool,
}

#[derive(Debug, Default)]
pub(in crate::dispatch) struct TranslatorWindowState {
    pub(super) pending: Option<SelectionRead>,

    /// 声明过 `reads_selection` 的会话（旧 DLL 不在其中）。一经声明就记住到会话关闭：
    /// 失焦时补的那次同步带 `false`，不能把它抹掉。
    pub(super) readers: HashSet<SessionId>,

    /// 声明过 `in_place_review` 的会话：认得原地翻译评审（0.4.14 起的 DLL）。
    pub(super) reviewers: HashSet<SessionId>,
}
