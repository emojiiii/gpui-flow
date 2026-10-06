use super::*;
use gpui_flow_core::PortRef;
use gpui_kit::component::tooltip::Tooltip;

pub(super) fn icon(name: &'static str, size: f32) -> gpui_kit::component::Icon {
    gpui_kit::component::Icon::default()
        .path(SharedString::from(format!("icons/{name}.svg")))
        .size(px(size))
}
pub(super) fn separator() -> Div {
    div().w(px(1.)).h(px(18.)).bg(rgb(0xe8eaed)).mx_1()
}
impl CreativeCanvas {
    pub(super) fn icon_button(
        &self,
        id: impl Into<ElementId>,
        name: &'static str,
        title: &'static str,
        action: Action,
        disabled: bool,
        cx: &mut Context<Self>,
    ) -> Button {
        Button::new(id)
            .accessibility_label(title)
            .disabled(disabled)
            .size(px(34.))
            .rounded(px(8.))
            .flex()
            .items_center()
            .justify_center()
            .text_color(rgb(if disabled { 0xb9bcc3 } else { 0x5b606a }))
            .hover(|s| s.bg(rgb(0xf0f1f3)))
            .tooltip(move |window, cx| Tooltip::new(title).build(window, cx))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_any_mouse_down(|_, window, _| window.prevent_default())
            .child(icon(name, 18.))
            .on_click(
                cx.listener(move |this, _, window, cx| this.action(action.clone(), window, cx)),
            )
    }
    pub(super) fn use_image_reference(&mut self, cx: &mut Context<Self>) {
        let Some(source) = self.selected(cx).filter(|n| n.kind == "image") else {
            return;
        };
        let mut graph = self.canvas.read(cx).editor.graph().clone();
        let id: NodeId = graph.fresh_id("image-generation").into();
        let at = WorldPoint::new(
            source.position.x + source.size.width + 100.,
            source.position.y,
        );
        let result = self
            .registry
            .make_node("generate_image", id.clone(), at, None)
            .and_then(|node| {
                graph.add_node(node)?;
                Ok(())
            })
            .and_then(|_| {
                graph.connect(
                    graph.fresh_id("reference"),
                    PortRef::new(source.id, "image"),
                    PortRef::new(id.clone(), "reference"),
                )?;
                self.registry.validate_graph(&graph)
            });
        if let Err(error) = result {
            self.notice = Some(error.to_string());
            cx.notify();
            return;
        }
        if self.command(CanvasCommand::ImportGraph(graph), cx) {
            self.command(
                CanvasCommand::SelectNode {
                    node: id,
                    mode: SelectionMode::Replace,
                },
                cx,
            );
            self.command(
                CanvasCommand::Fit {
                    canvas: self.canvas_size,
                    padding: 140.,
                },
                cx,
            );
        }
    }
    pub(super) fn object_toolbar(
        &self,
        node: &Node,
        editable: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        if self.is_image_editing(&node.id) {
            return self.image_crop_toolbar(cx);
        }
        let mut bar = div()
            .occlude()
            .h(px(38.))
            .px_1()
            .bg(rgb(0xffffff))
            .rounded(px(10.))
            .shadow_sm()
            .border_1()
            .border_color(rgb(0xe9eaed))
            .flex()
            .items_center()
            .gap_1();
        if node.kind == "image" {
            bar = bar
                .child(self.icon_button(
                    "image-crop",
                    "crop",
                    "裁剪、旋转和翻转",
                    Action::ImageEdit(image_edit_controls::ImageEditAction::Begin),
                    !editable || self.project.is_none() || self.saving,
                    cx,
                ))
                .child(self.icon_button(
                    "image-mask",
                    "paintbrush",
                    "从图片绘制遮罩",
                    Action::Mask(mask_editor::MaskAction::FromImage),
                    !editable || self.project.is_none(),
                    cx,
                ))
                .child(self.icon_button(
                    "replace-image",
                    "upload",
                    "替换本地图片",
                    Action::ReplaceImage,
                    !editable || self.project.is_none() || self.saving,
                    cx,
                ))
                .child(self.button(
                    "reference-image",
                    "图生图",
                    Action::UseReference,
                    !editable,
                    cx,
                ))
                .child(self.icon_button(
                    "original-ratio",
                    "maximize",
                    "恢复图片原比例",
                    Action::OriginalRatio,
                    !editable || self.project.is_none(),
                    cx,
                ))
                .child(separator());
        }
        if node.kind == "text" {
            bar = bar.child(self.text_style_toolbar(node, editable, cx));
        }
        if let Some(kind) = crate::processing::ProcessingKind::from_node_kind(&node.kind).or(
            match node.kind.as_str() {
                "image" => Some(crate::processing::ProcessingKind::Image),
                "video" => Some(crate::processing::ProcessingKind::Video),
                "audio" => Some(crate::processing::ProcessingKind::Audio),
                _ => None,
            },
        ) {
            bar = bar.child(self.icon_button(
                "process-media",
                "sliders-horizontal",
                "创建本地处理节点",
                Action::ProcessSelection(kind),
                !editable || self.project.is_none(),
                cx,
            ));
        }
        bar.child(self.icon_button(
            "object-copy",
            "copy",
            "复制对象",
            Action::Duplicate,
            !editable,
            cx,
        ))
        .child(self.icon_button(
            "object-frame",
            "grid-2x2",
            "编组",
            Action::Frame,
            !editable,
            cx,
        ))
        .child(self.icon_button(
            "object-colors",
            "sliders-horizontal",
            "端口颜色",
            Action::Colors,
            !editable,
            cx,
        ))
        .child(separator())
        .child(self.icon_button(
            "object-delete",
            "trash",
            "删除对象",
            Action::Delete,
            !editable,
            cx,
        ))
    }
    pub(super) fn layers_panel(&self, cx: &mut Context<Self>) -> Div {
        let nodes = self
            .canvas
            .read(cx)
            .editor
            .graph()
            .nodes
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let mut panel = div()
            .occlude()
            .absolute()
            .left(px(78.))
            .top(px(92.))
            .w(px(244.))
            .p_3()
            .rounded_xl()
            .bg(rgb(0xffffff))
            .border_1()
            .border_color(rgb(0xe9eaed))
            .shadow_md()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .pb_2()
                    .child(div().text_sm().child("图层"))
                    .child(self.icon_button(
                        "close-layers",
                        "x",
                        "关闭图层",
                        Action::Layers,
                        false,
                        cx,
                    )),
            );
        let mut rows = div()
            .id("object-layers")
            .max_h(px(420.))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_1();
        for node in nodes.into_iter().rev() {
            let selected = self
                .canvas
                .read(cx)
                .editor
                .selection
                .nodes
                .contains(&node.id);
            rows = rows.child(
                self.button(
                    SharedString::from(format!("layer-{}", node.id)),
                    node.title,
                    Action::SelectObject(node.id),
                    false,
                    cx,
                )
                .w_full()
                .when(selected, |s| s.bg(rgb(0xeff4ff)).text_color(rgb(0x5683cb))),
            );
        }
        panel = panel.child(rows);
        panel
    }

    pub(super) fn object_composer(
        &self,
        node: &Node,
        bounds: gpui_flow_core::Rect,
        editable: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        if self.is_image_editing(&node.id) {
            let p = anchored_panel(bounds, self.canvas_size, self.composer_size(node, cx));
            return div()
                .occlude()
                .absolute()
                .left(px(p.x))
                .top(px(p.y))
                .w(px(340.))
                .h(px(42.))
                .px_2()
                .rounded(px(10.))
                .bg(rgb(0xffffff))
                .shadow_sm()
                .child(self.image_edit_footer(cx));
        }
        if node.kind.starts_with("custom.") {
            let p = anchored_panel(bounds, self.canvas_size, self.composer_size(node, cx));
            return div()
                .occlude()
                .absolute()
                .left(px(p.x))
                .top(px(p.y))
                .w(px(360.))
                .max_h(px(280.))
                .p_3()
                .rounded(px(12.))
                .bg(rgb(0xffffff))
                .shadow_sm()
                .child(
                    div()
                        .id("custom-composer")
                        .max_h(px(256.))
                        .overflow_y_scroll()
                        .child(self.custom_composer(node, editable, cx)),
                );
        }
        if crate::processing::ProcessingKind::from_node_kind(&node.kind).is_some() {
            let p = anchored_panel(bounds, self.canvas_size, self.composer_size(node, cx));
            let input = self
                .project
                .as_ref()
                .ok_or_else(|| "请先打开本地项目".to_string())
                .and_then(|p| {
                    crate::processing_node::processing_source(
                        self.canvas.read(cx).editor.graph(),
                        node,
                        p.assets(),
                    )
                    .map_err(|e| e.to_string())
                });
            let state = processing_panel::ProcessingPanelState {
                input,
                running: self
                    .processing_job
                    .as_ref()
                    .is_some_and(|j| j.node == node.id),
                busy: self.saving || self.importing,
                error: node.data["error"].as_str().map(str::to_string),
            };
            return div()
                .occlude()
                .absolute()
                .left(px(p.x))
                .top(px(p.y))
                .w(px(360.))
                .p_3()
                .rounded(px(12.))
                .bg(rgb(0xffffff))
                .shadow_sm()
                .border_1()
                .border_color(rgb(0xe9eaed))
                .child(self.processing_panel(node, editable, state, cx))
                .when(
                    matches!(node.kind.as_str(), "process_video" | "process_audio")
                        && !node.data["asset"].as_str().unwrap_or_default().is_empty(),
                    |panel| panel.child(self.media_controls(node, cx)),
                );
        }
        if node.kind == "mask" {
            let p = anchored_panel(bounds, self.canvas_size, self.composer_size(node, cx));
            return div()
                .occlude()
                .absolute()
                .left(px(p.x))
                .top(px(p.y))
                .w(px(360.))
                .p_2()
                .rounded(px(12.))
                .bg(rgb(0xffffff))
                .shadow_sm()
                .border_1()
                .border_color(rgb(0xe9eaed))
                .child(self.mask_controls(node, editable, cx));
        }
        let generation = GenerationKind::from_node_kind(&node.kind);
        let model = matches!(node.kind.as_str(), "model" | "generate_model")
            && !node.data["asset"].as_str().unwrap_or_default().is_empty();
        let size = self.composer_size(node, cx);
        let p = anchored_panel(bounds, self.canvas_size, size);
        if matches!(node.kind.as_str(), "video" | "audio") {
            return div()
                .occlude()
                .absolute()
                .left(px(p.x))
                .top(px(p.y))
                .p_1()
                .rounded(px(10.))
                .bg(rgb(0xffffff))
                .shadow_sm()
                .child(self.media_controls(node, cx));
        }
        if node.kind == "text" {
            let editing = self.is_editing_text(&node.id);
            return div()
                .occlude()
                .absolute()
                .left(px(p.x))
                .top(px(p.y))
                .rounded_lg()
                .bg(rgb(0xffffff))
                .shadow_sm()
                .child(self.button(
                    "edit-canvas-text",
                    if editing {
                        "完成编辑"
                    } else {
                        "编辑文字"
                    },
                    if editing {
                        Action::FinishText
                    } else {
                        Action::EditText
                    },
                    !editable,
                    cx,
                ));
        }
        if node.kind == "image" && !self.quick_edit {
            return div()
                .occlude()
                .absolute()
                .left(px(p.x))
                .top(px(p.y))
                .rounded_lg()
                .bg(rgb(0xffffff))
                .shadow_sm()
                .child(self.button("quick-edit", "添加描述", Action::QuickEdit, !editable, cx));
        }
        let has_reference_input =
            generation.is_some() && crate::references::accepts_image_reference(node);
        let mut panel = div()
            .occlude()
            .absolute()
            .left(px(p.x))
            .top(px(p.y))
            .w(px(size.width))
            .p_2()
            .rounded(px(12.))
            .bg(rgb(0xffffff))
            .shadow_sm()
            .border_1()
            .border_color(rgb(0xe9eaed))
            .flex()
            .flex_col()
            .gap_1();
        if generation.is_some() && self.media_views.snapshot(&node.id, cx).is_some() {
            panel = panel.child(self.media_controls(node, cx));
        }
        if has_reference_input {
            panel = panel.child(self.reference_row(node, cx));
        }
        if node.kind == "edit_image" {
            panel = panel.child(self.mask_input_row(node, cx));
        }
        if generation.is_some() {
            panel = panel.child(self.reference_prompt(node, editable, cx));
        } else if !model {
            panel = panel.child(
                Textarea::new(&self.form)
                    .h(px(66.))
                    .bordered(false)
                    .appearance(false)
                    .text_size(px(13.))
                    .disabled(!editable)
                    .aria_label("对象描述"),
            );
        }
        if let Some(kind) = generation {
            let model_name = node.data["model"]
                .as_str()
                .filter(|s| !s.is_empty())
                .unwrap_or("未配置模型");
            let mut row = div().flex().items_center().gap_1().child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .gap_1()
                    .text_size(px(11.))
                    .text_color(rgb(0x9398a2))
                    .child(icon("sparkles", 13.))
                    .child(model_name.to_owned()),
            );
            if kind == GenerationKind::Model && !model {
                row = row.child(self.button(
                    "import-model",
                    "导入GLB",
                    Action::ImportModel,
                    !editable || self.project.is_none() || self.saving,
                    cx,
                ));
            }
            if matches!(kind, GenerationKind::Image | GenerationKind::Video)
                && node.kind != "edit_image"
            {
                let ratio = node.data["settings"]["aspect_ratio"]
                    .as_str()
                    .unwrap_or("1:1");
                let next = match ratio {
                    "1:1" => "16:9",
                    "16:9" => "9:16",
                    _ => "1:1",
                };
                row = row.child(self.button(
                    "object-ratio",
                    ratio.to_owned(),
                    Action::Setting("aspect_ratio", Value::String(next.into())),
                    !editable,
                    cx,
                ));
            }
            if matches!(kind, GenerationKind::Audio | GenerationKind::Video) {
                let duration = node.data["settings"]["duration_seconds"]
                    .as_u64()
                    .unwrap_or(5);
                row = row.child(self.button(
                    "duration",
                    format!("{duration}s"),
                    Action::Setting(
                        "duration_seconds",
                        serde_json::json!(if duration == 5 { 10 } else { 5 }),
                    ),
                    !editable,
                    cx,
                ));
            }
            row = row.child(
                self.icon_button(
                    "generate-selected",
                    "arrow-up",
                    if self.generation_available {
                        "生成"
                    } else {
                        "未连接生成服务"
                    },
                    Action::Generate,
                    !editable || !self.generation_available,
                    cx,
                )
                .size(px(28.))
                .bg(rgb(0x24262b))
                .text_color(rgb(if self.generation_available {
                    0xffffff
                } else {
                    0x80848c
                }))
                .rounded(px(7.)),
            );
            panel = panel.child(row);
        } else if model {
            panel = panel
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(self.button(
                            "model-left",
                            "↶",
                            Action::ModelOrbit(-0.3, 0.),
                            !editable,
                            cx,
                        ))
                        .child(self.button(
                            "model-right",
                            "↷",
                            Action::ModelOrbit(0.3, 0.),
                            !editable,
                            cx,
                        ))
                        .child(self.button(
                            "model-up",
                            "↑",
                            Action::ModelOrbit(0., 0.2),
                            !editable,
                            cx,
                        ))
                        .child(self.button(
                            "model-down",
                            "↓",
                            Action::ModelOrbit(0., -0.2),
                            !editable,
                            cx,
                        ))
                        .child(self.button("model-in", "+", Action::ModelZoom(0.8), !editable, cx))
                        .child(self.button(
                            "model-out",
                            "−",
                            Action::ModelZoom(1.25),
                            !editable,
                            cx,
                        ))
                        .child(self.button("model-fit", "复位", Action::ModelFit, !editable, cx)),
                )
                .child(
                    div()
                        .px_2()
                        .py_1()
                        .text_size(px(10.))
                        .text_color(rgb(0x969ca5))
                        .child("CPU 静态基色预览 · 无纹理/PBR/动画/骨骼"),
                );
        } else {
            panel = panel.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .px_2()
                            .text_size(px(10.))
                            .text_color(rgb(0xb0b4bb))
                            .child("自动保存"),
                    )
                    .when(node.kind == "image", |p| {
                        p.child(self.icon_button(
                            "close-quick-edit",
                            "x",
                            "收起描述",
                            Action::QuickEdit,
                            false,
                            cx,
                        ))
                    }),
            );
        }
        panel
    }
}
