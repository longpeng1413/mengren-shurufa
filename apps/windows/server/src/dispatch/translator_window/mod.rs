//! 翻译窗口快捷键（组合键 / 双击右 Ctrl）：前台是能读选区的猛人会话时，借它下一次空闲同步
//! （`ModeSync.read_selection`）读选区，选中的文字带进窗口直接翻译。输入法读不到（网页正文这类不可编辑内容、
//! 旧 DLL、不是猛人会话、到点没回）就交给界面层用辅助接口 / 模拟复制去取（[`TranslatorOpen::Probe`]）；
//! 私密输入什么都不读。选区由 DLL 用 `Selection` 回，经 [`Router::translator_selection`] 认领。

mod keys;
mod open;
mod state;

use std::time::{Duration, Instant};

use qingjian_platform::protocol::SessionId;

pub use self::keys::TranslatorKeys;
pub use self::open::TranslatorOpen;
use self::state::SelectionRead;
pub(super) use self::state::TranslatorWindowState;
use super::Router;

/// 等选区的上限：DLL 每 320 ms 空闲同步一次，再加只读编辑会话的时间。
const READ_TIMEOUT: Duration = Duration::from_millis(700);

impl Router {
    /// 快捷键或悬浮条要开翻译窗口。
    pub fn open_translator(&mut self) {
        if self.translator_window.pending.is_some() {
            return;
        }
        let foreground = self.foreground_session();
        if foreground
            .and_then(|session| self.sessions.get(&session))
            .is_some_and(|info| info.private)
        {
            self.status.open_translator(TranslatorOpen::Plain);
            return;
        }
        let Some(session) =
            foreground.filter(|session| self.translator_window.readers.contains(session))
        else {
            self.status.open_translator(TranslatorOpen::Probe);
            return;
        };
        self.selection_seq += 1;
        self.translator_window.pending = Some(SelectionRead {
            request: self.selection_seq,
            session,
            deadline: Instant::now() + READ_TIMEOUT,
            sent: false,
        });
        tracing::info!(
            ?session,
            request = self.selection_seq,
            "translator window waits for the selection"
        );
    }

    /// 记下这个会话的 DLL 能不能读选区（每次 SyncMode 都带）。
    /// 记下这个会话的 DLL 能读选区 / 能做原地评审（每次 SyncMode 都带）；只记不清，会话关了才忘。
    pub(super) fn note_selection_reader(&mut self, session: SessionId, reads: bool, reviews: bool) {
        if reads {
            self.translator_window.readers.insert(session);
        }
        if reviews {
            self.translator_window.reviewers.insert(session);
        }
    }

    /// 这个会话的 DLL 能不能接原地翻译评审。
    pub(in crate::dispatch) fn reviews_in_place(&self, session: SessionId) -> bool {
        self.translator_window.reviewers.contains(&session)
    }

    /// 这个会话的 DLL 能不能在空闲同步时读选区。
    pub(in crate::dispatch) fn reads_selection(&self, session: SessionId) -> bool {
        self.translator_window.readers.contains(&session)
    }

    /// 会话关了就忘掉它。
    pub(super) fn forget_selection_reader(&mut self, session: SessionId) {
        self.translator_window.readers.remove(&session);
        self.translator_window.reviewers.remove(&session);
    }

    /// 回给这个会话的 ModeSync 要不要带读选区请求；每个请求只发一次。
    pub(super) fn translator_read_request(&mut self, session: SessionId) -> Option<u64> {
        let pending = self.translator_window.pending.as_mut()?;
        if pending.session != session || pending.sent {
            return None;
        }
        pending.sent = true;
        Some(pending.request)
    }

    /// DLL 回来的选区是不是翻译窗口要的；是就带着文字开窗口（没读到就让界面层去取），回 `true`。
    pub(super) fn translator_selection(
        &mut self,
        session: SessionId,
        request: u64,
        text: &str,
        unreadable: bool,
    ) -> bool {
        if self
            .translator_window
            .pending
            .is_none_or(|pending| pending.session != session || pending.request != request)
        {
            return false;
        }
        self.translator_window.pending = None;
        let text = text.trim();
        tracing::info!(
            ?session,
            chars = text.chars().count(),
            unreadable,
            "translator window got the selection"
        );
        self.status.open_translator(if text.is_empty() {
            TranslatorOpen::Probe
        } else {
            TranslatorOpen::Text(text.to_owned())
        });
        true
    }

    /// tick 里看一眼：到点没收到选区就开空窗口。
    pub(super) fn advance_translator_window(&mut self) {
        if self
            .translator_window
            .pending
            .is_some_and(|pending| Instant::now() >= pending.deadline)
        {
            self.translator_window.pending = None;
            tracing::info!("translator window: no selection reply in time");
            self.status.open_translator(TranslatorOpen::Probe);
        }
    }

    /// 在等选区时 tick 要勤一点。
    pub(super) fn translator_window_waiting(&self) -> bool {
        self.translator_window.pending.is_some()
    }
}

#[cfg(test)]
mod tests;
