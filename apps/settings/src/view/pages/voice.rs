//! 「语音」页（仅 Windows）：识别服务（本机离线 / 豆包 / 关闭）、豆包密钥与资源 ID、长按触发时长，
//! 以及离线模型的状态、下载 / 取消 / 测试 / 删除。与「云服务」页（云联想、翻译）分开存。

use bamboo_slip::taffy::NodeId;
use bamboo_slip::{
    Button, RenderError, Segmented, SettingRow, Stepper, Ui, group, paragraph, section_title,
};

use qingjian_platform::{PROVIDER_NONE, PROVIDER_OFFLINE, PROVIDER_VOLCENGINE};

use crate::i18n::{tr, tr_args};
use crate::jobs::voice::{Stage, model_installed, model_size, model_total_bytes};
use crate::message::{FieldId, Message};
use crate::view::Page;
use crate::view::pages::cloud::field;
use crate::view::parts::actions::with_notice;
use crate::view::parts::controls::choice;

/// 长按触发的毫秒范围与步进。
const HOLD_MIN: u64 = 150;
const HOLD_MAX: u64 = 1500;
const HOLD_STEP: u64 = 50;

pub fn voice(ui: &mut Ui<Message>, page: &Page) -> Result<Vec<NodeId>, RenderError> {
    let theme = &page.theme;
    let config = &page.settings.config.voice;
    let mut out = Vec::new();

    // 服务选择 + 长按触发时长
    let providers = [PROVIDER_NONE, PROVIDER_OFFLINE, PROVIDER_VOLCENGINE];
    let provider_labels = [
        tr("voice.provider_none"),
        tr("voice.provider_offline"),
        tr("voice.provider_volcengine"),
    ];
    let selected = providers
        .iter()
        .position(|p| *p == config.provider)
        .unwrap_or(0);
    let provider =
        Segmented::new(&provider_labels, selected).build(ui, theme, "voice-provider", |index| {
            Message::set("voice", "provider", providers[index])
        })?;
    let mut rows = vec![
        SettingRow::new(tr("voice.provider"))
            .info(tr("voice.provider_info"))
            .control(provider)
            .build(ui, theme)?,
    ];

    let hold = config.hold_ms.clamp(HOLD_MIN, HOLD_MAX);
    let hold_control = Stepper::new(&tr_args("voice.hold_ms", &[("n", &hold)]))
        .limits(hold > HOLD_MIN, hold < HOLD_MAX)
        .build(
            ui,
            theme,
            "voice-hold",
            Message::set(
                "voice",
                "hold_ms",
                hold.saturating_sub(HOLD_STEP).clamp(HOLD_MIN, HOLD_MAX) as i64,
            ),
            Message::set(
                "voice",
                "hold_ms",
                hold.saturating_add(HOLD_STEP).clamp(HOLD_MIN, HOLD_MAX) as i64,
            ),
        )?;
    rows.push(
        SettingRow::new(tr("voice.hold"))
            .info(tr("voice.hold_info"))
            .control(hold_control)
            .build(ui, theme)?,
    );
    out.push(section_title(ui, theme, tr("voice.section_trigger"))?);
    out.push(group(ui, theme, &rows)?);

    // 按服务展开各自的设置
    match config.provider.as_str() {
        PROVIDER_VOLCENGINE => out.extend(volcengine(ui, page)?),
        PROVIDER_OFFLINE => out.extend(offline(ui, page)?),
        _ => out.push(paragraph(ui, theme, tr("voice.none_note"), None)?),
    }
    Ok(out)
}

/// 豆包：资源 ID（按时长 / 并发 / 文件）、API Key、测试连接、清除密钥。
fn volcengine(ui: &mut Ui<Message>, page: &Page) -> Result<Vec<NodeId>, RenderError> {
    let theme = &page.theme;
    let config = &page.settings.config.voice;
    let resources = [
        "volc.seedasr.sauc.duration",
        "volc.seedasr.sauc.concurrent",
        "volc.seedasr.auc",
    ];
    let selected = resources
        .iter()
        .position(|r| *r == config.resource_id)
        .unwrap_or(0);
    let resource = choice(
        ui,
        page,
        "voice-resource",
        &resources,
        resources[selected],
        |r| match r {
            "volc.seedasr.sauc.duration" => tr("voice.resource_duration"),
            "volc.seedasr.sauc.concurrent" => tr("voice.resource_concurrent"),
            _ => tr("voice.resource_file"),
        },
        |r| Message::set("voice", "resource_id", r),
    )?;
    let key_set = page.settings.voice_key_set();
    let key = field(
        ui,
        page,
        FieldId::VoiceApiKey,
        tr("voice.api_key"),
        Some(tr("voice.api_key_info")),
        if key_set {
            tr("voice.key_set")
        } else {
            tr("voice.key_unset")
        },
    )?;
    let running = page.settings.voice_test.running();
    let test = Button::new(if running {
        tr("voice.testing")
    } else {
        tr("voice.test")
    })
    .disabled(running)
    .build(ui, theme, "voice-test", Message::VoiceTest)?;
    let test = with_notice(
        ui,
        page,
        SettingRow::new(tr("voice.connection"))
            .info(tr("voice.connection_info"))
            .control(test),
        "voice",
    )?;
    let mut rows = vec![
        SettingRow::new(tr("voice.resource"))
            .info(tr("voice.resource_info"))
            .control(resource)
            .build(ui, theme)?,
        key,
        test,
    ];
    if key_set {
        let clear = Button::new(tr("voice.clear_key")).build(
            ui,
            theme,
            "voice-clear-key",
            Message::VoiceClearKey,
        )?;
        rows.push(
            SettingRow::new(tr("voice.clear_key"))
                .control(clear)
                .build(ui, theme)?,
        );
    }
    Ok(vec![
        section_title(ui, theme, tr("voice.section_volcengine"))?,
        group(ui, theme, &rows)?,
        paragraph(ui, theme, tr("voice.volcengine_note"), None)?,
    ])
}

/// 离线模型：状态、下载 / 取消 / 测试识别 / 删除。
fn offline(ui: &mut Ui<Message>, page: &Page) -> Result<Vec<NodeId>, RenderError> {
    let theme = &page.theme;
    let settings = page.settings;
    let root = settings.voice_model_root();
    let installed = model_installed(&root);
    let stage = settings.voice_download.stage();
    let busy = settings.voice_download.busy();

    let status = match &stage {
        Some(Stage::Downloading { .. }) => {
            let (done, total) = settings.voice_download.progress();
            if total > 0 {
                let percent = done.saturating_mul(100).checked_div(total).unwrap_or(0);
                tr_args(
                    "voice.offline_downloading",
                    &[
                        ("percent", &percent),
                        ("done", &megabytes(done)),
                        ("total", &megabytes(total)),
                    ],
                )
            } else {
                tr("voice.offline_connecting").to_owned()
            }
        }
        Some(Stage::Failed(error)) => error.clone(),
        Some(Stage::Cancelled) | Some(Stage::Done) | None if installed => tr_args(
            "voice.offline_installed",
            &[("size", &megabytes(model_size(&root)))],
        ),
        Some(Stage::Cancelled) | Some(Stage::Done) | None => tr_args(
            "voice.offline_missing",
            &[("total", &megabytes(model_total_bytes(&root)))],
        ),
    };
    let mut rows = vec![
        SettingRow::new(tr("voice.offline_model"))
            .detail(&status)
            .build(ui, theme)?,
    ];

    let downloading = matches!(stage, Some(Stage::Downloading { .. }));
    if downloading {
        let cancel = Button::new(tr("voice.offline_cancel")).build(
            ui,
            theme,
            "voice-model-cancel",
            Message::VoiceCancelDownload,
        )?;
        rows.push(
            SettingRow::new(tr("voice.offline_cancel"))
                .control(cancel)
                .build(ui, theme)?,
        );
    } else if !installed {
        let label = if matches!(stage, Some(Stage::Failed(_))) {
            tr("voice.offline_retry")
        } else {
            tr("voice.offline_download")
        };
        let download = Button::new(label).disabled(busy).build(
            ui,
            theme,
            "voice-model-download",
            Message::VoiceDownloadModel,
        )?;
        rows.push(SettingRow::new(label).control(download).build(ui, theme)?);
    } else {
        let running = settings.voice_test.running();
        let test = Button::new(if running {
            tr("voice.testing")
        } else {
            tr("voice.offline_test")
        })
        .disabled(running)
        .build(ui, theme, "voice-offline-test", Message::VoiceTest)?;
        rows.push(with_notice(
            ui,
            page,
            SettingRow::new(tr("voice.offline_test"))
                .info(tr("voice.offline_test_info"))
                .control(test),
            "voice",
        )?);
    }

    if !downloading && (installed || model_size(&root) > 0) {
        let armed = settings.voice_remove_armed;
        let mut remove = Button::new(if armed {
            tr("voice.offline_delete_confirm")
        } else {
            tr("voice.offline_delete")
        });
        if armed {
            remove = remove.primary();
        }
        let remove = remove.build(ui, theme, "voice-model-remove", Message::VoiceRemoveModel)?;
        rows.push(
            SettingRow::new(tr("voice.offline_delete"))
                .control(remove)
                .build(ui, theme)?,
        );
    }

    Ok(vec![
        section_title(ui, theme, tr("voice.section_offline"))?,
        group(ui, theme, &rows)?,
        paragraph(ui, theme, tr("voice.offline_note"), None)?,
    ])
}

fn megabytes(bytes: u64) -> String {
    format!("{} MB", (bytes + 524_288) / 1_048_576)
}
