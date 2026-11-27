//! Controls for explicit, local media jobs. The host supplies resolved input
//! and actual task state; rendering never starts work or fabricates progress.
use super::*;
use crate::processing::{AudioOutputFormat, ImageOutputFormat, ProcessingKind, ProcessingSettings};

#[derive(Clone)]
pub(super) enum ProcessingAction {
    SetSettings(ProcessingSettings),
    Run,
    Cancel,
}

/// `input` must come from the connected input port and project asset resolver.
/// `running` identifies this node's active job. `busy` also covers work on a
/// different node and any host operation which prevents starting a new job.
pub(super) struct ProcessingPanelState {
    pub input: Result<ProjectAsset, String>,
    pub running: bool,
    pub busy: bool,
    pub error: Option<String>,
}

impl CreativeCanvas {
    /// Unpositioned content for the standard 360 px selection composer.
    pub(super) fn processing_panel(
        &self,
        node: &Node,
        editable: bool,
        state: ProcessingPanelState,
        cx: &mut Context<Self>,
    ) -> Div {
        let Some(kind) = ProcessingKind::from_node_kind(&node.kind) else {
            return div().child(label(tr(
                "此对象不支持本地处理",
                "This object does not support local processing",
            )));
        };
        let settings = serde_json::from_value::<ProcessingSettings>(node.data["settings"].clone())
            .map_err(|_| {
                tr(
                    "处理参数无效，请恢复默认参数",
                    "Invalid processing parameters. Restore defaults.",
                )
                .to_string()
            })
            .and_then(|settings| {
                if settings.kind() != kind || settings.validate().is_err() {
                    Err(tr(
                        "处理参数与节点不匹配，请恢复默认参数",
                        "Processing parameters do not match this node. Restore defaults.",
                    )
                    .to_string())
                } else {
                    Ok(settings)
                }
            });
        let disabled = !editable || node.locked || state.running || state.busy;
        let run_disabled = disabled || state.input.is_err() || settings.is_err();
        let symbol = match kind {
            ProcessingKind::Image => "image",
            ProcessingKind::Video => "video",
            ProcessingKind::Audio => "music",
        };
        let mut body = div().flex().flex_col().gap(px(6.)).child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .text_size(px(12.))
                .text_color(rgb(0x42464f))
                .child(chrome::icon(symbol, 15.).text_color(rgb(0x8298b8)))
                .child(tr(
                    kind.title(),
                    match kind {
                        ProcessingKind::Image => "Image Processing",
                        ProcessingKind::Video => "Video Processing",
                        ProcessingKind::Audio => "Audio Processing",
                    },
                ))
                .child(div().flex_1())
                .child(label(tr("本地处理", "Local Processing"))),
        );
        let mut input = div()
            .px_2()
            .py_1()
            .rounded(px(6.))
            .bg(rgb(0xf5f6f8))
            .flex()
            .flex_col()
            .gap(px(2.));
        input = match &state.input {
            Ok(asset) => input
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(px(11.))
                        .text_color(rgb(0x626b78))
                        .child(tr_format!("输入 · {}", "Input · {}", asset.name)),
                )
                .child(
                    div()
                        .truncate()
                        .text_size(px(10.))
                        .text_color(rgb(0x9097a1))
                        .child(input_details(asset)),
                ),
            Err(_) => input
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(rgb(0x9398a2))
                        .child(tr("输入尚未就绪", "Input not ready")),
                )
                .child(label(match kind {
                    ProcessingKind::Image => tr(
                        "连接包含本地图片的输出端口",
                        "Connect an output containing a local image",
                    ),
                    ProcessingKind::Video => tr(
                        "连接包含本地视频的输出端口",
                        "Connect an output containing a local video",
                    ),
                    ProcessingKind::Audio => tr(
                        "连接包含本地音频的输出端口",
                        "Connect an output containing local audio",
                    ),
                })),
        };
        body = body.child(input);

        if let Ok(settings) = &settings {
            body = body.child(self.processing_settings(settings, disabled, cx));
        } else {
            body = body.child(self.button(
                "processing-reset-settings",
                tr("恢复默认参数", "Restore Defaults"),
                Action::Processing(ProcessingAction::SetSettings(kind.default_settings())),
                disabled,
                cx,
            ));
        }

        let problem = settings
            .as_ref()
            .err()
            .or(state.error.as_ref())
            .or(state.input.as_ref().err());
        let (status, color) = if state.running {
            (
                tr("正在本地处理…", "Processing locally…").to_string(),
                0x6683ae,
            )
        } else if let Some(error) = problem {
            (error.clone(), 0xb07150)
        } else if state.busy {
            (
                tr(
                    "等待当前任务结束后可运行",
                    "Run when the current task finishes",
                )
                .to_string(),
                0x969ca5,
            )
        } else {
            (
                settings.as_ref().map(settings_summary).unwrap_or_default(),
                0x969ca5,
            )
        };
        body = body.child(
            div()
                .id(SharedString::from(format!("processing-status-{}", node.id)))
                .max_h(px(32.))
                .overflow_y_scroll()
                .text_size(px(10.))
                .text_color(rgb(color))
                .child(status),
        );
        let action = if state.running {
            self.button(
                "processing-cancel",
                tr("取消处理", "Cancel processing"),
                Action::Processing(ProcessingAction::Cancel),
                false,
                cx,
            )
            .bg(rgb(0xf0f1f3))
        } else {
            self.button(
                "processing-run",
                if state.error.is_some() {
                    tr("重试处理", "Retry Processing")
                } else {
                    tr("运行处理", "Run Processing")
                },
                Action::Processing(ProcessingAction::Run),
                run_disabled,
                cx,
            )
            .bg(rgb(if run_disabled { 0xeff0f3 } else { 0x24262b }))
            .text_color(rgb(if run_disabled { 0xaeb1b7 } else { 0xffffff }))
        };
        body.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(label(tr("输出保存为新素材", "Save output as a new asset")))
                .child(action),
        )
    }

    fn processing_settings(
        &self,
        settings: &ProcessingSettings,
        disabled: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut rows = div().flex().flex_col().gap_1();
        match *settings {
            ProcessingSettings::Image {
                format,
                quality,
                max_edge,
            } => {
                let mut formats = setting_row(tr("格式", "Format"));
                for (title, selected) in [
                    ("PNG", ImageOutputFormat::Png),
                    ("JPEG", ImageOutputFormat::Jpeg),
                ] {
                    formats = formats.child(self.processing_option(
                        format!("processing-image-{title}"),
                        title,
                        settings,
                        ProcessingSettings::Image {
                            format: selected,
                            quality,
                            max_edge,
                        },
                        disabled,
                        cx,
                    ));
                }
                let mut quality_row = setting_row(tr("质量", "Quality"));
                if format == ImageOutputFormat::Png {
                    quality_row =
                        quality_row.child(label(tr("PNG 无损编码", "Lossless PNG encoding")));
                } else {
                    for next in [70, 85, 95] {
                        quality_row = quality_row.child(self.processing_option(
                            format!("processing-quality-{next}"),
                            next.to_string(),
                            settings,
                            ProcessingSettings::Image {
                                format,
                                quality: next,
                                max_edge,
                            },
                            disabled,
                            cx,
                        ));
                    }
                }
                let mut size_row = setting_row(tr("最长边", "Longest Side"));
                for next in [1024, 2048, 4096] {
                    size_row = size_row.child(self.processing_option(
                        format!("processing-image-edge-{next}"),
                        format!("{next} px"),
                        settings,
                        ProcessingSettings::Image {
                            format,
                            quality,
                            max_edge: next,
                        },
                        disabled,
                        cx,
                    ));
                }
                rows = rows.child(formats).child(quality_row).child(size_row);
            }
            ProcessingSettings::Video { crf, max_edge } => {
                let mut quality_row = setting_row("CRF");
                for (title, next) in [
                    (tr("18 · 清晰", "18 · High quality"), 18),
                    (tr("23 · 均衡", "23 · Balanced"), 23),
                    (tr("28 · 小巧", "28 · Compact"), 28),
                ] {
                    quality_row = quality_row.child(self.processing_option(
                        format!("processing-crf-{next}"),
                        title,
                        settings,
                        ProcessingSettings::Video {
                            crf: next,
                            max_edge,
                        },
                        disabled,
                        cx,
                    ));
                }
                let mut size_row = setting_row(tr("最长边", "Longest Side"));
                for next in [1280, 1920, 3840] {
                    size_row = size_row.child(self.processing_option(
                        format!("processing-video-edge-{next}"),
                        format!("{next} px"),
                        settings,
                        ProcessingSettings::Video {
                            crf,
                            max_edge: next,
                        },
                        disabled,
                        cx,
                    ));
                }
                rows = rows.child(quality_row).child(size_row);
            }
            ProcessingSettings::Audio {
                format,
                bitrate_kbps,
                sample_rate,
            } => {
                let mut formats = setting_row(tr("格式", "Format"));
                for (title, selected) in [
                    ("WAV", AudioOutputFormat::Wav),
                    ("MP3", AudioOutputFormat::Mp3),
                ] {
                    formats = formats.child(self.processing_option(
                        format!("processing-audio-{title}"),
                        title,
                        settings,
                        ProcessingSettings::Audio {
                            format: selected,
                            bitrate_kbps,
                            sample_rate,
                        },
                        disabled,
                        cx,
                    ));
                }
                let mut bitrate_row = setting_row(tr("码率", "Bitrate"));
                if format == AudioOutputFormat::Wav {
                    bitrate_row = bitrate_row.child(label(tr(
                        "PCM 16-bit · 无压缩",
                        "16-bit PCM · Uncompressed",
                    )));
                } else {
                    for next in [128, 192, 320] {
                        bitrate_row = bitrate_row.child(self.processing_option(
                            format!("processing-bitrate-{next}"),
                            format!("{next} kbps"),
                            settings,
                            ProcessingSettings::Audio {
                                format,
                                bitrate_kbps: next,
                                sample_rate,
                            },
                            disabled,
                            cx,
                        ));
                    }
                }
                let mut sample_row = setting_row(tr("采样率", "Sample Rate"));
                for (title, next) in [("44.1 kHz", 44100), ("48 kHz", 48000)] {
                    sample_row = sample_row.child(self.processing_option(
                        format!("processing-sample-rate-{next}"),
                        title,
                        settings,
                        ProcessingSettings::Audio {
                            format,
                            bitrate_kbps,
                            sample_rate: next,
                        },
                        disabled,
                        cx,
                    ));
                }
                rows = rows.child(formats).child(bitrate_row).child(sample_row);
            }
        }
        rows
    }

    fn processing_option(
        &self,
        id: String,
        title: impl Into<SharedString>,
        current: &ProcessingSettings,
        next: ProcessingSettings,
        disabled: bool,
        cx: &mut Context<Self>,
    ) -> Button {
        let selected = *current == next;
        self.button(
            SharedString::from(id),
            title,
            Action::Processing(ProcessingAction::SetSettings(next)),
            disabled,
            cx,
        )
        .text_size(px(11.))
        .when(selected, |button| {
            button
                .bg(rgb(0xeff4fb))
                .text_color(rgb(if disabled { 0x9cabc0 } else { 0x5683ba }))
        })
    }
}

fn setting_row(title: &'static str) -> Div {
    let label_width = match crate::localization::language() {
        crate::localization::CreativeLanguage::SimplifiedChinese => 44.,
        crate::localization::CreativeLanguage::English => 72.,
    };
    div()
        .h(px(28.))
        .flex()
        .items_center()
        .gap_1()
        .child(label(title).w(px(label_width)).flex_shrink_0())
}

fn input_details(asset: &ProjectAsset) -> String {
    let size = if asset.byte_len >= 1024 * 1024 {
        format!("{:.1} MiB", asset.byte_len as f64 / (1024. * 1024.))
    } else if asset.byte_len >= 1024 {
        format!("{:.1} KiB", asset.byte_len as f64 / 1024.)
    } else {
        format!("{} B", asset.byte_len)
    };
    if asset.width > 0 && asset.height > 0 {
        format!(
            "{} · {} × {} · {size}",
            asset.media_type, asset.width, asset.height
        )
    } else {
        format!("{} · {size}", asset.media_type)
    }
}

/// Include the real values even if a project uses non-preset parameters.
fn settings_summary(settings: &ProcessingSettings) -> String {
    match *settings {
        ProcessingSettings::Image {
            format: ImageOutputFormat::Jpeg,
            quality,
            max_edge,
        } => tr_format!(
            "JPEG · 质量 {quality} · 最长边 {max_edge}px · 透明填白",
            "JPEG · Quality {quality} · Longest side {max_edge}px · White background"
        ),
        ProcessingSettings::Image { max_edge, .. } => {
            tr_format!(
                "PNG 无损 · 最长边 {max_edge}px · 保留透明度",
                "Lossless PNG · Longest side {max_edge}px · Preserves transparency"
            )
        }
        ProcessingSettings::Video { crf, max_edge } => {
            tr_format!(
                "MP4 / H.264 · CRF {crf} · 最长边 {max_edge}px",
                "MP4 / H.264 · CRF {crf} · Longest side {max_edge}px"
            )
        }
        ProcessingSettings::Audio {
            format: AudioOutputFormat::Wav,
            sample_rate,
            ..
        } => tr_format!(
            "WAV / PCM16 · {sample_rate} Hz · 首条音轨",
            "WAV / PCM16 · {sample_rate} Hz · First audio track"
        ),
        ProcessingSettings::Audio {
            format: AudioOutputFormat::Mp3,
            bitrate_kbps,
            sample_rate,
        } => tr_format!(
            "MP3 · {bitrate_kbps} kbps · {sample_rate} Hz · 首条音轨",
            "MP3 · {bitrate_kbps} kbps · {sample_rate} Hz · First audio track"
        ),
    }
}
