//! 原地翻译：双击右 Shift（或 Ctrl+Alt+T 时输入法读到空选区）把选中的文字译好放进候选窗，回车替换、Esc 取消。
//!
//! 先借前台会话的空闲同步让 DLL 按「翻译选中文字」读选区（读到就走原有评审，并记下选区好替换）；
//! 读不到（微信、WPS、原生 EDIT 这类经兼容层的程序报空选区）就交给注入的 [`SelectionProbe`]
//! （Windows 上是辅助接口 + 模拟 Ctrl+Insert）取文字，取到后发翻译，并在下一次空闲同步时让 DLL 进评审态
//! （`ModeSync.start_review`）。前台不是猛人会话时没法写回原程序，改开翻译窗口。

mod state;

use std::time::{Duration, Instant};

use qingjian_platform::protocol::{ScreenRect, SessionId};
use qingjian_platform::windows_text::tr;

pub(in crate::dispatch) use self::state::InPlace;
use self::state::Stage;
use super::Translation;
use crate::dispatch::{Router, TranslatorOpen};

/// 等 DLL 空闲同步读选区：每 320 ms 一次，再加只读编辑会话。
const TSF_TIMEOUT: Duration = Duration::from_millis(700);

/// 等界面层取文字：辅助接口限时 0.8 秒，模拟复制还要等修饰键松开、等应用写剪贴板。
const PROBE_TIMEOUT: Duration = Duration::from_secs(4);

/// 等 DLL 进评审态；过了就收起，免得候选窗挂着没人接。
const START_TIMEOUT: Duration = Duration::from_secs(2);

/// 界面层取选中文字：给请求号，取完用 [`Router::selection_probed`] 交回（可能在别的线程上取）。
pub type SelectionProbe = Box<dyn Fn(u64) + Send>;

impl Router {
    pub fn set_selection_probe(&mut self, probe: SelectionProbe) {
        self.selection_probe = Some(probe);
    }

    /// 双击右 Shift：把前台选中的文字原地翻译。
    pub fn translate_in_place(&mut self) {
        if self.in_place.is_some()
            || self.translation.is_some()
            || !self.engine.composition().is_empty()
            || self.panel_open()
        {
            tracing::info!(
                waiting = self.in_place.is_some(),
                reviewing = self.translation.is_some(),
                "in-place translation skipped: busy"
            );
            return;
        }
        if !self.engine.translation_enabled() {
            tracing::info!("in-place translation unavailable: translation is off");
            self.flash_notice(tr("disabled"));
            return;
        }
        let Some(session) = self
            .foreground_session()
            .filter(|session| self.sessions.contains_key(session))
        else {
            // 不是猛人会话：译文写不回原程序，改开翻译窗口
            self.status.open_translator(TranslatorOpen::Probe);
            return;
        };
        if self.sessions.get(&session).is_some_and(|info| info.private) {
            tracing::info!("in-place translation skipped: private field");
            return;
        }
        self.selection_seq += 1;
        let request = self.selection_seq;
        // 旧 DLL 认不得原地评审的读选区请求：直接改用兜底取法，取到后开翻译窗口
        if self.reads_selection(session) && self.reviews_in_place(session) {
            self.pending_selection = Some(request);
            self.in_place = Some(InPlace {
                session,
                request,
                stage: Stage::Tsf { sent: false },
                deadline: Instant::now() + TSF_TIMEOUT,
            });
            tracing::info!(
                ?session,
                request,
                "in-place translation waits for the selection"
            );
        } else {
            self.start_probe(session, request);
        }
    }

    /// 输入法读不到或说没选中：交给界面层去取。没注入取法（测试 / 非 Windows）就到此为止。
    pub(in crate::dispatch) fn start_probe(&mut self, session: SessionId, request: u64) {
        self.pending_selection = None;
        let Some(probe) = &self.selection_probe else {
            self.in_place = None;
            return;
        };
        probe(request);
        self.in_place = Some(InPlace {
            session,
            request,
            stage: Stage::Probe,
            deadline: Instant::now() + PROBE_TIMEOUT,
        });
        tracing::info!(
            ?session,
            request,
            "in-place translation probes the selection"
        );
    }

    /// 这次空闲同步要不要带「按原地翻译读选区」的请求号；只发一次。
    pub(in crate::dispatch) fn in_place_read_request(&mut self, session: SessionId) -> Option<u64> {
        let pending = self.in_place.as_mut()?;
        if pending.session != session || pending.stage != (Stage::Tsf { sent: false }) {
            return None;
        }
        pending.stage = Stage::Tsf { sent: true };
        Some(pending.request)
    }

    /// 这次空闲同步要不要让 DLL 进评审态。DLL 来轮询前一直带（重复进评审无害），
    /// 免得某次同步没处理就丢了、候选窗挂着没人接。
    pub(in crate::dispatch) fn in_place_start(&self, session: SessionId) -> bool {
        self.in_place
            .is_some_and(|pending| pending.session == session && pending.stage == Stage::Start)
    }

    /// DLL 进评审后第一次轮询：「进评审」送到了。
    pub(in crate::dispatch) fn ack_in_place(&mut self, session: SessionId) {
        if self.translation.is_some() && self.in_place_start(session) {
            self.in_place = None;
        }
    }

    /// 评审结束（按键、失焦、超时）：还在等 DLL 进评审的那一步一并作废。
    pub(in crate::dispatch) fn drop_in_place_start(&mut self) {
        if self
            .in_place
            .is_some_and(|pending| pending.stage == Stage::Start)
        {
            self.in_place = None;
        }
    }

    /// DLL 回的选区是不是原地翻译要的：是就把焦点对齐到这个会话，交给原有的评审流程接着处理。
    pub(in crate::dispatch) fn claim_in_place_selection(
        &mut self,
        session: SessionId,
        request: u64,
    ) {
        if self
            .in_place
            .is_some_and(|pending| pending.session == session && pending.request == request)
        {
            self.in_place = None;
            // 焦点从别的会话切过来时会重置组句状态、清掉在等的读选区请求，对齐后再记回去
            self.ensure_focus(session);
            self.pending_selection = Some(request);
        }
    }

    /// 界面层取到（或没取到）选中文字；`rect` 是光标（取不到时鼠标）位置，候选窗摆在它下面。
    pub fn selection_probed(
        &mut self,
        request: u64,
        text: Option<String>,
        rect: Option<ScreenRect>,
    ) {
        let Some(pending) = self
            .in_place
            .filter(|pending| pending.request == request && pending.stage == Stage::Probe)
        else {
            return;
        };
        let session = pending.session;
        let Some(text) = text
            .map(|text| text.trim().to_owned())
            .filter(|text| !text.is_empty())
        else {
            self.in_place = None;
            tracing::info!(?session, "in-place translation: no selection found");
            self.flash_notice(tr("selection_none"));
            return;
        };
        // 取的时候换了窗口、会话没了，或 DLL 太旧接不了评审：写不回去，改开翻译窗口
        if self.foreground_session() != Some(session)
            || !self.sessions.contains_key(&session)
            || !self.reviews_in_place(session)
        {
            self.in_place = None;
            tracing::info!(
                ?session,
                old_dll = !self.reviews_in_place(session),
                "in-place translation shown in the translator window instead"
            );
            self.status.open_translator(TranslatorOpen::Text(text));
            return;
        }
        self.ensure_focus(session);
        if let Some(rect) = rect {
            self.last_rect = Some(rect);
        }
        self.translation = Some(Translation { result: None });
        self.engine.request_translation(&text);
        self.in_place = Some(InPlace {
            session,
            request,
            stage: Stage::Start,
            deadline: Instant::now() + START_TIMEOUT,
        });
        tracing::info!(
            ?session,
            chars = text.chars().count(),
            "in-place translation requested from the probed selection"
        );
        let frame = self.self_drawn_frame();
        self.reconcile_candidates(&frame);
    }

    /// tick 里推进：读不到就换下一步，评审没接上就收起。
    pub(in crate::dispatch) fn advance_in_place(&mut self) {
        let Some(pending) = self.in_place else {
            return;
        };
        if Instant::now() < pending.deadline {
            return;
        }
        match pending.stage {
            Stage::Tsf { .. } => {
                tracing::info!("in-place translation: no selection reply in time");
                self.start_probe(pending.session, pending.request);
            }
            Stage::Probe => self.in_place = None,
            Stage::Start => {
                tracing::info!("in-place translation: input session never took the review");
                self.end_translation();
            }
        }
    }

    /// 在等时 tick 要勤一点。
    pub(in crate::dispatch) fn in_place_waiting(&self) -> bool {
        self.in_place.is_some()
    }
}

#[cfg(test)]
mod tests;
