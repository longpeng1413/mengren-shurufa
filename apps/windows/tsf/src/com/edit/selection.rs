//! 读选区会话：在异步只读会话里取当前选区的文本与屏幕矩形回给 Server。两种用途见 [`SelectionUse`]：
//! 「翻译选中文字」读到非空选区才置 [`Shared::set_translating`]；翻译窗口只取文字，不进原地评审。

use std::rc::Rc;

use windows::Win32::UI::TextServices::{
    ITfContext, ITfEditSession, ITfEditSession_Impl, ITfRange, TF_ES_READ,
};
use windows::core::{Result, implement};

use qingjian_platform::protocol::ScreenRect;

use super::anchor::{anchor_rect, mouse_screen_rect, selection_range};
use crate::com::composition::Shared;
use crate::com::log::log;
use crate::com::service::SharedClient;

/// 原地翻译的原文字符上限（与 macOS 壳一致），超过就不译，避免整篇误触。
const MAX_TRANSLATE_CHARS: usize = 500;

/// 带进翻译窗口的字符上限；超过就截断，窗口里还能接着改。
const MAX_WINDOW_CHARS: usize = 4000;

/// 读选区是给谁用的。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SelectionUse {
    /// 「翻译选中文字」：读到就在原地出译文候选。
    Review,

    /// 翻译窗口快捷键：只把文字交给 Server 转给窗口。
    Window,
}

#[implement(ITfEditSession)]
pub(crate) struct SelectionSession {
    /// 目标文档上下文。
    context: ITfContext,

    /// 引擎层：把选区文本发给 Server。
    engine: SharedClient,

    /// 读到非空选区就置「翻译评审进行中」。
    shared: Rc<Shared>,

    /// 请求标识，回给 Server 对上是哪一次读选区请求。
    request: u64,

    purpose: SelectionUse,
}

impl ITfEditSession_Impl for SelectionSession_Impl {
    fn DoEditSession(&self, ec: u32) -> Result<()> {
        let read = read_selection(&self.context, ec, self.purpose);
        let unreadable = read.is_none();
        let (text, rect, range) = match read {
            Some((text, rect, range)) => (text, rect, Some(range)),
            None => (String::new(), mouse_screen_rect(), None),
        };
        let has_text = !text.trim().is_empty();
        // 发成功再置本地评审态，免得卡在无窗口的评审里。
        if let Ok(mut guard) = self.engine.try_borrow_mut()
            && let Some(client) = guard.as_mut()
        {
            match client.selection(self.request, text, rect, unreadable) {
                Ok(_) if has_text && self.purpose == SelectionUse::Review => match range {
                    Some(range) => self.shared.start_review(range),
                    None => self.shared.set_translating(true),
                },
                Ok(_) => {}
                Err(error) => log(&format!("回选区给 Server 失败: {error}")),
            }
        }
        Ok(())
    }
}

/// 请求一个异步只读会话读选区。`Ok` 只说明已受理。
pub(crate) fn request_selection(
    context: &ITfContext,
    client_id: u32,
    engine: SharedClient,
    shared: Rc<Shared>,
    request: u64,
    purpose: SelectionUse,
) -> Result<()> {
    let session = SelectionSession {
        context: context.clone(),
        engine,
        shared,
        request,
        purpose,
    };
    super::update::request(context, client_id, session.into(), TF_ES_READ)
}

/// 取不到选区（文档不给读）回 `None`；选区为空或过长时文本为空串（原地翻译过长给空串，翻译窗口截断）。
/// 一并交回选区范围，原地翻译接受时替换它。
fn read_selection(
    context: &ITfContext,
    ec: u32,
    purpose: SelectionUse,
) -> Option<(String, ScreenRect, ITfRange)> {
    let range = selection_range(context, ec)?;
    let rect = anchor_rect(context, ec, &range);
    let text = range_text(&range, ec, purpose);
    Some((text, rect, range))
}

/// 编辑会话都没要到（网页正文这类非可编辑内容会拒绝）：直接告诉 Server 读不了，免得它干等到超时。
pub(crate) fn report_unreadable(engine: &SharedClient, request: u64) {
    if let Ok(mut guard) = engine.try_borrow_mut()
        && let Some(client) = guard.as_mut()
        && let Err(error) = client.selection(request, String::new(), mouse_screen_rect(), true)
    {
        log(&format!("回读不了的选区给 Server 失败: {error}"));
    }
}

fn range_text(range: &ITfRange, ec: u32, purpose: SelectionUse) -> String {
    let limit = match purpose {
        SelectionUse::Review => MAX_TRANSLATE_CHARS,
        SelectionUse::Window => MAX_WINDOW_CHARS,
    };
    let mut buf = vec![0u16; limit + 1];
    let mut fetched = 0u32;
    if unsafe { range.GetText(ec, 0, &mut buf, &mut fetched) }.is_err() {
        return String::new();
    }
    let count = fetched as usize;
    if count == 0 || (count > limit && purpose == SelectionUse::Review) {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..count.min(limit)])
}
