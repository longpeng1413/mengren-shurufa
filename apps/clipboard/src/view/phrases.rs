//! 自定义短语竖排列表，无码条目也能点选填写。

use super::{Context, gap, pad, text_node};
use crate::{app::Message, i18n::tr};
use bamboo_slip::taffy::{Dimension, FlexDirection, NodeId, Size, Style};
use bamboo_slip::{Button, RenderError, ScrollArea, Ui, Visual};

pub(super) fn phrases(ui: &mut Ui<Message>, context: &Context) -> Result<Vec<NodeId>, RenderError> {
    let theme = &context.theme;
    let title = text_node(
        ui,
        tr("phrases.title").into(),
        theme.text(theme.font.size, 600, theme.colors.text),
        Style::default(),
    )?;
    let back =
        Button::new(tr("devices.back")).build(ui, theme, "phrases-back", Message::ClosePhrases)?;
    let sync =
        Button::new(tr("phrases.sync")).build(ui, theme, "phrases-sync", Message::SyncPhrases)?;
    let header = ui.scene().node(
        Style {
            gap: gap(theme, 8.0),
            flex_shrink: 0.0,
            ..Style::default()
        },
        Visual::Group,
        &[title, sync, back],
    )?;
    let mut rows = Vec::new();
    for (i, p) in context
        .app
        .phrases
        .iter()
        .enumerate()
        .filter(|(_, p)| p.enabled)
    {
        let preview: String = p
            .text
            .lines()
            .take(4)
            .collect::<Vec<_>>()
            .join("\n")
            .chars()
            .take(180)
            .collect();
        let label = if p.code.is_empty() {
            format!("{}\n{}", p.group, preview)
        } else {
            format!("{} · {}\n{}", p.group, p.code, preview)
        };
        let text = text_node(
            ui,
            label,
            theme
                .text(theme.font.size, 400, theme.colors.text)
                .wrapped(),
            Style {
                size: Size {
                    width: Dimension::percent(1.0),
                    height: Dimension::auto(),
                },
                ..Style::default()
            },
        )?;
        let row = ui.scene().node(
            Style {
                padding: pad(theme, 10.0, 10.0),
                flex_shrink: 0.0,
                ..Style::default()
            },
            Visual::solid(theme.colors.surface, theme.px(6.0)),
            &[text],
        )?;
        let key = ui.key(("phrase", i));
        ui.interact(
            row,
            key,
            bamboo_slip::Interact::new().on_click(Message::PastePhrase(i)),
        );
        rows.push(row);
    }
    if rows.is_empty() {
        rows.push(text_node(
            ui,
            tr("phrases.empty").into(),
            theme
                .text(theme.small, 400, theme.colors.secondary)
                .wrapped(),
            Style::default(),
        )?);
    }
    let content = ui.scene().node(
        Style {
            flex_direction: FlexDirection::Column,
            gap: gap(theme, 8.0),
            ..Style::default()
        },
        Visual::Group,
        &rows,
    )?;
    let area = ScrollArea::new(Style {
        flex_grow: 1.0,
        flex_basis: Dimension::length(0.0),
        ..Style::default()
    })
    .build(ui, theme, "phrases-list", content)?;
    Ok(vec![header, area])
}
