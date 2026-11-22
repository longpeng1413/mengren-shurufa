//! 卡片的内容：词卡显示音标、原形与考试标签、释义，句子卡显示原文、翻译状态、译文，没读到文字时显示提示。
//! 排版与摆放在 [`super::arrange`]。

use std::time::Instant;

use qingjian_platform::windows_text::tr;
use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateFontW, DEFAULT_CHARSET, DeleteObject, HFONT,
    OUT_DEFAULT_PRECIS, VARIABLE_PITCH,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SetFocus, VK_LBUTTON, VK_RBUTTON,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetForegroundWindow, GetParent, GetWindowRect, IsWindowVisible, SW_HIDE, SW_SHOW,
    SendMessageW, SetForegroundWindow, SetWindowTextW, ShowWindow, WM_COPY,
};
use windows::core::{HSTRING, w};

/// 短语里最多几个实词时列词典：再多就是句子了。
const MAX_DICTIONARY_WORDS: usize = 6;

/// 词典区里每个词最多几行释义。
const MAX_DEFINITION_LINES: usize = 4;

/// 悬停卡片：鼠标离开卡片多远就收起（96 DPI 下的像素）；词就在卡片正上方，停在词上不会收。
const HOVER_MARGIN: i32 = 48;

use super::{
    BODY, CLOSE, COPY, Card, DETAILS, EXTRA, LOOP, OPEN, ORIGINAL, PHONETIC, SPEAK_RESULT,
    SPEAK_SOURCE, TITLE,
};

impl Card {
    /// 弹卡片：单个英文词且词表里有就是词卡，否则发翻译。返回词卡的词（自动发音用）。
    pub(in crate::window) fn show(
        &mut self,
        x: i32,
        y: i32,
        text: String,
        language: usize,
        hover: bool,
    ) -> Option<String> {
        self.anchor = (x, y);
        self.hover = hover;
        self.text = text.trim().to_owned();
        self.word = self.words.get().and_then(|words| words.lookup(&self.text));
        // 英文原文是短语：现在就能列词典；中文原文等译文回来再列译文里的词
        let original = self.text.clone();
        self.details = if self.word.is_none() {
            self.dictionary(&original)
        } else {
            String::new()
        };
        self.read_when_translated = false;
        self.manual = false;
        let now = Instant::now();
        // 词卡不翻译；原文为空（Server 没读到选区）时只显示提示
        if self.word.is_some() || self.text.is_empty() {
            self.model.change(String::new(), language, now);
        } else {
            self.model.change(self.text.clone(), language, now);
            self.model.send();
        }
        self.fill();
        self.place();
        // 悬停卡片不抢焦点：用户可能正在读或打字
        if !hover {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_SHOW);
                let _ = SetForegroundWindow(self.hwnd);
                let _ = SetFocus(Some(self.hwnd));
            }
        }
        self.word.as_ref().map(|word| word.word.clone())
    }

    /// 译文到了：刷新状态与译文，按新的文字量重新定高。
    pub(in crate::window) fn render(&mut self) {
        if self.word.is_none() && self.visible() {
            if self.details.is_empty() {
                let output = self.model.output.clone();
                self.details = self.dictionary(&output);
            }
            self.fill();
            self.place();
        }
    }

    pub(in crate::window) fn hide(&mut self) {
        self.read_when_translated = false;
        if self.visible() {
            let _ = unsafe { ShowWindow(self.hwnd, SW_HIDE) };
        }
        let language = self.model.language;
        self.model.change(String::new(), language, Instant::now());
    }

    /// 英文短语的词典区：每个实词一段，首行是词与音标，下面是几行释义；不是英文短语就为空。
    fn dictionary(&mut self, english: &str) -> String {
        let Some(parts) = qingjian_translate::phrase_words(english, MAX_DICTIONARY_WORDS) else {
            return String::new();
        };
        let parts: Vec<String> = parts.into_iter().map(str::to_owned).collect();
        let Some(words) = self.words.get() else {
            return String::new();
        };
        let mut blocks = Vec::new();
        for part in parts {
            let Some(card) = words.lookup(&part) else {
                continue;
            };
            let mut head = card.word.clone();
            if let Some(phonetic) = &card.phonetic {
                let region = if card.american {
                    format!("{} ", tr("card_us"))
                } else {
                    String::new()
                };
                head.push_str(&format!("  {region}/{phonetic}/"));
            }
            let mut lines = vec![head];
            lines.extend(card.definitions.into_iter().take(MAX_DEFINITION_LINES));
            blocks.push(lines.join("\n"));
        }
        blocks.join("\n\n")
    }

    /// 激活没成功时的兜底：卡片不在前台时收不到「点到别处」，定时器里看到在卡片外按着鼠标就收起；
    /// 悬停卡片本来就不在前台，鼠标离开卡片一段距离就收起。
    pub(in crate::window) fn watch_outside_click(&mut self) {
        // 拖边框时按着鼠标、指针也常在卡片外，不算点到别处
        if !self.visible() || super::sizing() {
            return;
        }
        unsafe {
            if GetForegroundWindow() == self.hwnd {
                return;
            }
            if self.hover {
                let mut cursor = POINT::default();
                let mut rect = RECT::default();
                if GetCursorPos(&mut cursor).is_ok() && GetWindowRect(self.hwnd, &mut rect).is_ok()
                {
                    let margin = HOVER_MARGIN * GetDpiForWindow(self.hwnd).max(96) as i32 / 96;
                    let near = (rect.left - margin..rect.right + margin).contains(&cursor.x)
                        && (rect.top - margin..rect.bottom + margin).contains(&cursor.y);
                    if !near {
                        self.hide();
                        return;
                    }
                }
            }
            let pressed = [VK_LBUTTON, VK_RBUTTON]
                .iter()
                .any(|key| GetAsyncKeyState(i32::from(key.0)) as u16 & 0x8000 != 0);
            let mut cursor = POINT::default();
            let mut rect = RECT::default();
            if !pressed
                || GetCursorPos(&mut cursor).is_err()
                || GetWindowRect(self.hwnd, &mut rect).is_err()
            {
                return;
            }
            let inside = (rect.left..rect.right).contains(&cursor.x)
                && (rect.top..rect.bottom).contains(&cursor.y);
            if !inside {
                self.hide();
            }
        }
    }

    pub(in crate::window) fn visible(&self) -> bool {
        unsafe { IsWindowVisible(self.hwnd) }.as_bool()
    }

    /// 消息是不是发给卡片或它的子控件的（Esc 收卡片而不是收翻译窗口）。
    pub(in crate::window) fn owns(&self, hwnd: HWND) -> bool {
        hwnd == self.hwnd || unsafe { GetParent(hwnd) }.is_ok_and(|parent| parent == self.hwnd)
    }

    /// 复制正文：词卡是释义，句子卡是译文。
    pub(in crate::window) fn copy(&self) {
        let body = self.controls[BODY];
        unsafe {
            SendMessageW(body, 0xB1, Some(WPARAM(0)), Some(LPARAM(-1)));
            SendMessageW(body, WM_COPY, None, None);
            SendMessageW(body, 0xB1, Some(WPARAM(0)), Some(LPARAM(0)));
        }
    }

    /// 按词卡 / 句子卡填文字、决定哪些控件显示。
    fn fill(&self) {
        match &self.word {
            Some(word) => {
                self.set(TITLE, &word.word);
                let phonetic = word.phonetic.as_deref().map(|phonetic| {
                    if word.american {
                        format!("{} /{phonetic}/", tr("card_us"))
                    } else {
                        format!("/{phonetic}/")
                    }
                });
                self.set(PHONETIC, phonetic.as_deref().unwrap_or(""));
                let mut extra: Vec<String> = word
                    .lemma
                    .iter()
                    .map(|lemma| tr("card_lemma").replace("{word}", lemma))
                    .collect();
                extra.extend(
                    word.tags
                        .iter()
                        .filter_map(|tag| tag_name(tag))
                        .map(str::to_owned),
                );
                self.set(EXTRA, &extra.join(" · "));
                self.set(ORIGINAL, "");
                self.set(DETAILS, "");
                self.set(BODY, &word.definitions.join("\n"));
                self.set(SPEAK_SOURCE, tr("card_speak"));
                self.set(COPY, tr("card_copy"));
            }
            // 没读到选中的文字：说明原因，只留「在翻译窗口打开」（可以粘贴进去翻译）
            None if self.text.is_empty() => {
                self.set(TITLE, tr("title"));
                self.set(PHONETIC, "");
                self.set(EXTRA, "");
                self.set(ORIGINAL, "");
                self.set(BODY, tr("card_none"));
                self.set(DETAILS, "");
            }
            None => {
                self.set(TITLE, tr("result"));
                self.set(PHONETIC, "");
                self.set(DETAILS, &self.details);
                self.set(EXTRA, self.model.status);
                self.set(ORIGINAL, &self.text);
                self.set(BODY, &self.model.output);
                self.set(SPEAK_SOURCE, tr("read_source"));
                self.set(SPEAK_RESULT, tr("read_result"));
                self.set(COPY, tr("copy"));
            }
        }
        self.set(OPEN, tr("card_open"));
        self.set(LOOP, &self.loop_label);
        self.set(CLOSE, "×");
    }

    /// 这个 DPI 下的标题、正文、小字字体，DPI 变了就重建。
    pub(super) fn fonts(&mut self, dpi: u32) -> (HFONT, HFONT, HFONT) {
        if let Some((title, font, small, at)) = self.fonts
            && at == dpi
        {
            return (title, font, small);
        }
        if let Some((title, font, small, _)) = self.fonts.take() {
            for old in [title, font, small] {
                let _ = unsafe { DeleteObject(old.into()) };
            }
        }
        let make = |size: i32, weight: i32| unsafe {
            CreateFontW(
                -(size * dpi as i32 / 96),
                0,
                0,
                0,
                weight,
                0,
                0,
                0,
                DEFAULT_CHARSET,
                OUT_DEFAULT_PRECIS,
                CLIP_DEFAULT_PRECIS,
                CLEARTYPE_QUALITY,
                VARIABLE_PITCH.0 as u32,
                w!("Microsoft YaHei UI"),
            )
        };
        let fonts = (make(19, 700), make(14, 400), make(12, 400));
        self.fonts = Some((fonts.0, fonts.1, fonts.2, dpi));
        fonts
    }

    /// 放得下按钮文字的宽度，不小于 `min`。
    pub(super) fn button_width(&self, id: usize, font: HFONT, min: i32, padding: i32) -> i32 {
        let text = self.text_of(id);
        let width = text
            .lines()
            .map(|line| measure_line(self.hwnd, font, line))
            .max()
            .unwrap_or(0);
        (width + padding).max(min)
    }

    pub(super) fn text_of(&self, id: usize) -> String {
        let control = self.controls[id];
        unsafe {
            let length = windows::Win32::UI::WindowsAndMessaging::GetWindowTextLengthW(control);
            let mut buffer = vec![0; length.max(0) as usize + 1];
            let read =
                windows::Win32::UI::WindowsAndMessaging::GetWindowTextW(control, &mut buffer);
            String::from_utf16_lossy(&buffer[..read.max(0) as usize]).replace("\r\n", "\n")
        }
    }

    /// 设控件文字；文本框只认 `\r\n` 换行。
    pub(super) fn set(&self, id: usize, text: &str) {
        let shown = text.replace("\r\n", "\n").replace('\n', "\r\n");
        let _ = unsafe { SetWindowTextW(self.controls[id], &HSTRING::from(shown)) };
    }
}

/// 单行文字的宽度（物理像素）。
fn measure_line(hwnd: HWND, font: HFONT, text: &str) -> i32 {
    use windows::Win32::Foundation::SIZE;
    use windows::Win32::Graphics::Gdi::{GetDC, GetTextExtentPoint32W, ReleaseDC, SelectObject};
    let wide: Vec<u16> = text.encode_utf16().collect();
    let mut size = SIZE::default();
    unsafe {
        let dc = GetDC(Some(hwnd));
        let old = SelectObject(dc, font.into());
        let _ = GetTextExtentPoint32W(dc, &wide, &mut size);
        SelectObject(dc, old);
        ReleaseDC(Some(hwnd), dc);
    }
    size.cx
}

/// 考试标签的显示名。
fn tag_name(tag: &str) -> Option<&'static str> {
    Some(tr(match tag {
        "zk" => "tag_zk",
        "gk" => "tag_gk",
        "cet4" => "tag_cet4",
        "cet6" => "tag_cet6",
        "ky" => "tag_ky",
        "toefl" => "tag_toefl",
        "ielts" => "tag_ielts",
        "gre" => "tag_gre",
        _ => return None,
    }))
}
