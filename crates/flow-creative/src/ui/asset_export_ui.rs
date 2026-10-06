use super::*;
use crate::asset_export::{ExportAsset, export_asset, export_assets, export_vector};
use crate::vector_export::{VectorExport, prepare_vector_export};
use gpui_flow_core::Selection;
use gpui_kit::component::input::{Input, InputState};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone)]
pub(super) enum ExportAction {
    Open,
    Choose(usize),
    Browse,
    Write,
    Cancel,
}

pub(super) struct AssetExportDialog {
    selection: Selection,
    project: String,
    outputs: Vec<ExportChoice>,
    selected: usize,
    path: Entity<InputState>,
    choosing: bool,
    error: Option<String>,
}

#[derive(Clone, PartialEq)]
enum ExportChoice {
    Media(ExportAsset),
    Vector(VectorExport),
}
impl ExportChoice {
    fn name(&self) -> &str {
        match self {
            Self::Media(asset) => &asset.suggested_name,
            Self::Vector(vector) => vector.suggested_name(),
        }
    }
    fn label(&self) -> String {
        match self {
            Self::Media(asset) => asset.label.clone(),
            Self::Vector(vector) => format!(
                "{}{} 个矢量对象",
                if vector.is_frame() { "画框 · " } else { "" },
                vector.object_count()
            ),
        }
    }
    fn details(&self) -> String {
        match self {
            Self::Media(output) => {
                let asset = &output.asset;
                let format = Path::new(&asset.key)
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("media")
                    .to_ascii_uppercase();
                let dimensions = if asset.width > 0 && asset.height > 0 {
                    format!(" · {} × {}", asset.width, asset.height)
                } else {
                    String::new()
                };
                format!(
                    "{format}{dimensions} · {:.1} MiB",
                    asset.byte_len as f64 / 1_048_576.
                )
            }
            Self::Vector(vector) => {
                let size = vector.bounds().size;
                format!(
                    "SVG · {:.0} × {:.0} · {}",
                    size.width,
                    size.height,
                    if vector.has_white_background() {
                        "画框白底"
                    } else {
                        "透明背景"
                    }
                )
            }
        }
    }
    fn is_mask(&self) -> bool {
        matches!(self, Self::Media(output) if output.asset.media_type == MASK_MEDIA_TYPE)
    }
}

fn export_choices(
    graph: &gpui_flow_core::Graph,
    selection: &Selection,
    assets: &[ProjectAsset],
) -> Result<Vec<ExportChoice>, String> {
    if selection.nodes.len() == 1
        && selection.groups.is_empty()
        && selection.edges.is_empty()
        && let Some(id) = selection.nodes.iter().next()
        && graph
            .node(id)
            .is_some_and(crate::asset_export::has_media_output)
    {
        return export_assets(graph, id, assets)
            .map(|assets| assets.into_iter().map(ExportChoice::Media).collect())
            .map_err(|error| error.to_string());
    }
    prepare_vector_export(graph, selection).map(|vector| vec![ExportChoice::Vector(vector)])
}

fn default_directory() -> PathBuf {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .filter(|p| p.is_absolute());
    home.map(|home| {
        let downloads = home.join("Downloads");
        if downloads.is_dir() { downloads } else { home }
    })
    .or_else(|| std::env::current_dir().ok())
    .unwrap_or_else(|| PathBuf::from("/"))
}

impl CreativeCanvas {
    pub(super) fn clear_asset_export(&mut self) {
        if let Some(cancel) = &self.export_cancel {
            cancel.store(true, Ordering::Release);
        }
        self.export_dialog = None;
        self.export_serial = self.export_serial.wrapping_add(1);
    }

    pub(super) fn reconcile_asset_export(&mut self, cx: &App) {
        let current = self.export_dialog.as_ref().is_none_or(|dialog| {
            let Some(project) = &self.project else {
                return false;
            };
            project.metadata().id == dialog.project
                && self.canvas.read(cx).editor.selection == dialog.selection
                && export_choices(
                    self.canvas.read(cx).editor.graph(),
                    &dialog.selection,
                    project.assets(),
                )
                .is_ok_and(|outputs| {
                    outputs
                        .iter()
                        .any(|output| output == &dialog.outputs[dialog.selected])
                })
        });
        if !current {
            self.clear_asset_export();
        }
    }

    pub(super) fn handle_asset_export(
        &mut self,
        action: ExportAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(action, ExportAction::Cancel) {
            self.clear_asset_export();
            self.canvas.read(cx).focus_handle(cx).focus(window, cx);
            cx.notify();
            return;
        }
        if self.importing || self.saving {
            return;
        }
        if matches!(action, ExportAction::Open) {
            let selection = self.canvas.read(cx).editor.selection.clone();
            let Some(project) = &self.project else { return };
            match export_choices(
                self.canvas.read(cx).editor.graph(),
                &selection,
                project.assets(),
            ) {
                Ok(outputs) => {
                    let path = default_directory().join(outputs[0].name());
                    let project = project.metadata().id.clone();
                    self.clear_asset_export();
                    let input = cx.new(|cx| {
                        let mut state = InputState::new(window, cx).context_menu(false);
                        state.set_value(path.to_string_lossy().to_string(), window, cx);
                        state
                    });
                    self.export_dialog = Some(AssetExportDialog {
                        selection,
                        project,
                        outputs,
                        selected: 0,
                        path: input,
                        choosing: false,
                        error: None,
                    });
                    self.show_layers = false;
                    self.asset_picker = None;
                    self.export_focus.focus(window, cx);
                }
                Err(error) => self.notice = Some(format!("暂时不能导出：{error}")),
            }
            cx.notify();
            return;
        }
        self.reconcile_asset_export(cx);
        let Some(dialog) = &mut self.export_dialog else {
            return;
        };
        if dialog.choosing {
            return;
        }
        if let ExportAction::Choose(index) = action {
            if let Some(output) = dialog.outputs.get(index) {
                let current = PathBuf::from(dialog.path.read(cx).value().as_ref());
                let path = current
                    .parent()
                    .filter(|p| p.is_absolute())
                    .map(Path::to_path_buf)
                    .unwrap_or_else(default_directory)
                    .join(output.name());
                dialog.selected = index;
                dialog.error = None;
                dialog.path.update(cx, |state, cx| {
                    state.set_value(path.to_string_lossy().to_string(), window, cx)
                });
            }
            cx.notify();
            return;
        }
        if matches!(action, ExportAction::Browse) {
            let current = PathBuf::from(dialog.path.read(cx).value().as_ref());
            let directory = current
                .parent()
                .filter(|p| p.is_absolute())
                .map(Path::to_path_buf)
                .unwrap_or_else(default_directory);
            let receiver =
                cx.prompt_for_new_path(&directory, Some(dialog.outputs[dialog.selected].name()));
            dialog.choosing = true;
            let serial = self.export_serial;
            cx.spawn_in(window, async move |this, cx| {
                let result = receiver.await;
                let _ = this.update_in(cx, |this, window, cx| {
                    if this.export_serial != serial {
                        return;
                    }
                    let Some(dialog) = &mut this.export_dialog else {
                        return;
                    };
                    dialog.choosing = false;
                    match result {
                        Ok(Ok(Some(path))) => {
                            dialog.path.update(cx, |state, cx| {
                                state.set_value(path.to_string_lossy().to_string(), window, cx)
                            });
                            this.handle_asset_export(ExportAction::Write, window, cx);
                        }
                        Ok(Ok(None)) => {}
                        _ => {
                            dialog.error =
                                Some("无法打开系统保存窗口，请填写完整文件路径后导出".into());
                            dialog.path.read(cx).focus_handle(cx).focus(window, cx);
                        }
                    }
                    cx.notify();
                });
            })
            .detach();
            cx.notify();
            return;
        }
        if matches!(action, ExportAction::Write) {
            let destination = PathBuf::from(dialog.path.read(cx).value().trim());
            if !destination.is_absolute() {
                dialog.error = Some("请输入完整绝对路径，并保留原格式后缀".into());
                cx.notify();
                return;
            }
            let choice = dialog.outputs[dialog.selected].clone();
            let Some(project) = self.project.clone() else {
                return;
            };
            let cancel = Arc::new(AtomicBool::new(false));
            self.export_cancel = Some(cancel.clone());
            self.importing = true;
            let serial = self.export_serial;
            cx.spawn(async move |this, cx| {
                let result = cx
                    .background_executor()
                    .spawn(async move {
                        match choice {
                            ExportChoice::Media(output) => {
                                export_asset(&project, &output.asset.key, &destination, &cancel)
                            }
                            ExportChoice::Vector(vector) => {
                                export_vector(&project, &vector, &destination, &cancel)
                            }
                        }
                        .map_err(|error| error.to_string())
                    })
                    .await;
                let _ = this.update(cx, |this, cx| {
                    this.importing = false;
                    this.export_cancel = None;
                    if this.export_serial == serial {
                        match result {
                            Ok(output) => {
                                this.export_dialog = None;
                                this.notice = Some(format!("已导出到 {}", output.path.display()));
                            }
                            Err(error) => {
                                if let Some(dialog) = &mut this.export_dialog {
                                    dialog.error = Some(error);
                                }
                            }
                        }
                    }
                    this.queue_autosave(cx);
                    if this.return_after_save || this.pending_window_close.is_some() {
                        this.save_project(cx);
                    }
                    cx.notify();
                });
            })
            .detach();
            cx.notify();
        }
    }

    pub(super) fn asset_export_panel(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let dialog = self.export_dialog.as_ref().expect("visible export dialog");
        let busy = self.importing || self.saving || dialog.choosing;
        let selected = &dialog.outputs[dialog.selected];
        let mut outputs = div()
            .id("export-outputs")
            .max_h(px(150.))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_1();
        for (index, output) in dialog.outputs.iter().enumerate() {
            outputs = outputs.child(
                self.button(
                    SharedString::from(format!("export-output-{index}")),
                    format!("{} · {}", output.label(), output.name()),
                    Action::ExportAsset(ExportAction::Choose(index)),
                    busy,
                    cx,
                )
                .when(index == dialog.selected, |v| v.bg(rgb(0xeff4ff))),
            );
        }
        div()
            .id("asset-export-panel")
            .occlude()
            .track_focus(&self.export_focus)
            .absolute()
            .right(px(22.))
            .top(px(82.))
            .w(px(400.))
            .p_4()
            .bg(rgb(0xffffff))
            .rounded_xl()
            .shadow_md()
            .border_1()
            .border_color(rgb(0xe9eaed))
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().text_sm().child("导出素材"))
                    .child(self.icon_button(
                        "close-export",
                        "x",
                        "取消导出",
                        Action::ExportAsset(ExportAction::Cancel),
                        false,
                        cx,
                    )),
            )
            .child(outputs)
            .child(label(selected.details()))
            .when(matches!(selected, ExportChoice::Vector(_)), |v| {
                v.child(label("仅形状与笔迹 · 保留图层、线宽和透明度"))
            })
            .when(selected.is_mask(), |v| {
                v.child(label("灰度Mask：0 保留，255 编辑；未转换供应商语义"))
            })
            .child(self.button(
                "export-browse",
                "选择保存位置并导出",
                Action::ExportAsset(ExportAction::Browse),
                busy,
                cx,
            ))
            .child(label("或填写完整文件路径"))
            .child(
                Input::new(&dialog.path)
                    .h(px(34.))
                    .disabled(busy)
                    .aria_label("导出文件绝对路径"),
            )
            .when_some(dialog.error.clone(), |v, error| {
                v.child(div().text_xs().text_color(rgb(0xa45d48)).child(error))
            })
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .child(label(if self.export_cancel.is_some() {
                        "正在导出，可取消"
                    } else {
                        "原素材保留 · 不覆盖已有文件"
                    }))
                    .child(
                        self.button(
                            "export-write",
                            "导出",
                            Action::ExportAsset(ExportAction::Write),
                            busy,
                            cx,
                        )
                        .bg(rgb(0x282a30))
                        .text_color(rgb(0xffffff)),
                    ),
            )
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    cx.stop_propagation();
                    window.prevent_default();
                    this.handle_asset_export(ExportAction::Cancel, window, cx);
                }
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;
    #[test]
    fn media_and_vector_choices_use_distinct_outputs_and_reject_mixed_selection() {
        let asset = ProjectAsset {
            key: "assets/a-123.png".into(),
            name: "Image".into(),
            media_type: "image/png".into(),
            byte_len: 64,
            width: 20,
            height: 10,
        };
        let mut graph = gpui_flow_core::Graph::new();
        graph
            .add_node(
                standard_registry()
                    .make_node(
                        "image",
                        "image",
                        WorldPoint::ZERO,
                        Some(serde_json::json!({"asset":asset.key,"caption":""})),
                    )
                    .unwrap(),
            )
            .unwrap();
        graph
            .add_node(
                crate::shapes::create_shape_node(
                    "shape",
                    "Shape",
                    WorldPoint::ZERO,
                    WorldSize::new(100., 80.),
                    crate::shapes::ShapeData::new(crate::shapes::ShapeKind::Rectangle),
                )
                .unwrap(),
            )
            .unwrap();
        let mut selection = Selection {
            nodes: ["image".into()].into_iter().collect(),
            ..Default::default()
        };
        let assets = [asset];
        assert!(matches!(
            &export_choices(&graph, &selection, &assets).unwrap()[0],
            ExportChoice::Media(_)
        ));
        selection.nodes = ["shape".into()].into_iter().collect();
        assert!(matches!(
            &export_choices(&graph, &selection, &assets).unwrap()[0],
            ExportChoice::Vector(_)
        ));
        selection.nodes.insert("image".into());
        assert!(export_choices(&graph, &selection, &assets).is_err());
        let mut frame = gpui_flow_core::Group::new(
            "frame",
            "Artboard",
            gpui_flow_core::Rect::new(WorldPoint::ZERO, WorldSize::new(320., 240.)),
        )
        .with_nodes(["shape".into()]);
        frame.data = serde_json::json!({"flow_creative_frame": 1});
        graph.add_group(frame).unwrap();
        selection.nodes.clear();
        selection.groups.insert("frame".into());
        let choices = export_choices(&graph, &selection, &assets).unwrap();
        assert!(matches!(&choices[0], ExportChoice::Vector(v)
            if v.is_frame() && v.has_white_background() && v.bounds().size == WorldSize::new(320., 240.)));
        graph
            .groups
            .get_mut(&"frame".into())
            .unwrap()
            .node_ids
            .insert("image".into());
        assert!(export_choices(&graph, &selection, &assets).is_err());
    }
}
