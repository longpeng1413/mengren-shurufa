//! 循环播放的按钮文字与菜单：翻译窗口与划词卡片各有一个「循环」按钮，改的是同一份设置。

use qingjian_platform::windows_text::tr;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;

use super::speech::LOOP_COUNTS;
use super::{LOOP, SOURCE_LOOP, Window, popup};

/// 菜单里次数各项从第几项开始（第一项是开关，第二项是分隔线）。
const FIRST_COUNT: usize = 2;

impl Window {
    /// 按钮上的字：开着是「循环 3 次」，关着是「不循环」。
    pub(super) fn loop_label(&self) -> String {
        let repeat = &self.speech.repeat;
        if repeat.on {
            count_label(repeat.count)
        } else {
            tr("loop_off").to_owned()
        }
    }

    /// 按钮可能出现的所有文字，量宽度用（换次数时按钮不跟着变宽变窄）。
    pub(super) fn loop_labels() -> Vec<String> {
        std::iter::once(tr("loop_off").to_owned())
            .chain(LOOP_COUNTS.iter().map(|&count| count_label(count)))
            .collect()
    }

    /// 「循环」菜单：第一项开关，分隔线下选次数（选次数顺带打开）。`owner` 收菜单的自绘消息，`anchor` 是按钮。
    pub(super) fn loop_menu(&mut self, owner: HWND, anchor: HWND) {
        let counts: Vec<String> = LOOP_COUNTS
            .iter()
            .map(|&count| count_label(count))
            .collect();
        let mut labels = vec![tr("loop_enable"), ""];
        labels.extend(counts.iter().map(String::as_str));
        let (on, count) = (self.speech.repeat.on, self.speech.repeat.count);
        let mut checked =
            vec![FIRST_COUNT + LOOP_COUNTS.iter().position(|&n| n == count).unwrap_or(1)];
        if on {
            checked.push(0);
        }
        let mut rect = RECT::default();
        if unsafe { GetWindowRect(anchor, &mut rect) }.is_err() {
            return;
        }
        match popup::show(owner, rect, self.font, &labels, &checked) {
            Some(0) => self.speech.repeat.set(!on, count),
            Some(index) if index >= FIRST_COUNT => {
                if let Some(&count) = LOOP_COUNTS.get(index - FIRST_COUNT) {
                    self.speech.repeat.set(true, count);
                }
            }
            _ => return,
        }
        let label = self.loop_label();
        self.set(LOOP, &label);
        self.set(SOURCE_LOOP, &label);
        self.card.set_loop_label(&label);
    }
}

fn count_label(count: u32) -> String {
    tr("loop_count").replace("{n}", &count.to_string())
}
