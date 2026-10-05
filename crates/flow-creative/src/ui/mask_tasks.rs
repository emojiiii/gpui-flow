//! Mask graph actions and the explicitly applied, local PNG output task.
use super::*;
use crate::mask::{MaskRasterSize, MaskSource};
use gpui_flow_core::PortRef;
use std::sync::atomic::{AtomicBool, Ordering};

impl CreativeCanvas {
    pub(super) fn create_mask_from_image(&mut self, cx: &mut Context<Self>) {
        let Some(image) = self.selected(cx).filter(|n| n.kind == "image") else {
            return;
        };
        if !self.canvas.read(cx).is_editable() {
            return;
        }
        let result = (|| {
            let project = self
                .project
                .as_ref()
                .ok_or_else(|| CreativeError("请先打开本地项目".into()))?;
            let key = image.data["asset"]
                .as_str()
                .ok_or_else(|| CreativeError("图片尚无素材".into()))?;
            let asset = project
                .assets()
                .iter()
                .find(|asset| asset.key == key)
                .filter(|asset| matches!(asset.media_type.as_str(), "image/png" | "image/jpeg"))
                .ok_or_else(|| CreativeError("遮罩编辑当前支持项目中的静态PNG/JPEG图片".into()))?;
            let source = MaskSource::new(key, asset.width, asset.height)
                .map_err(|e| CreativeError(e.to_string()))?;
            let mut graph = self.canvas.read(cx).editor.graph().clone();
            let id: NodeId = graph.fresh_id("mask").into();
            let mut data = MaskNodeData::default();
            data.reset_source(source)?;
            let mut node = self.registry.make_node(
                "mask",
                id.clone(),
                WorldPoint::new(image.position.x, image.position.y + image.size.height + 96.),
                Some(serde_json::to_value(data)?),
            )?;
            node.size = project_ui::imported_image_size(asset.width, asset.height);
            graph.add_node(node.clone())?;
            graph.connect(
                graph.fresh_id("mask-image"),
                PortRef::new(image.id, "image"),
                PortRef::new(id.clone(), "image"),
            )?;
            self.registry.validate_graph(&graph)?;
            if self.command(CanvasCommand::ImportGraph(graph), cx) {
                self.command(
                    CanvasCommand::SelectNode {
                        node: id,
                        mode: SelectionMode::Replace,
                    },
                    cx,
                );
                self.focus_mask_object(&node, cx);
            }
            Ok::<_, CreativeError>(())
        })();
        if let Err(error) = result {
            self.notice = Some(error.to_string());
            cx.notify();
        }
    }
    pub(super) fn focus_mask_object(&mut self, node: &Node, cx: &mut Context<Self>) {
        let zoom = ((self.canvas_size.height - 318.).max(80.) / node.size.height)
            .min((self.canvas_size.width - 180.).max(80.) / node.size.width)
            .clamp(0.1, 1.2);
        self.command(
            CanvasCommand::Zoom {
                position: WorldPoint::new(
                    self.canvas_size.width / 2.,
                    self.canvas_size.height / 2.,
                ),
                zoom,
            },
            cx,
        );
        let actual = self
            .canvas
            .read(cx)
            .editor
            .viewport
            .world_to_screen(node.bounds().center());
        let wanted = WorldPoint::new(
            self.canvas_size.width / 2.,
            130. + node.size.height * zoom / 2.,
        );
        self.command(
            CanvasCommand::Pan {
                delta: wanted - actual,
            },
            cx,
        );
    }
    pub(super) fn create_masked_image_generator(&mut self, cx: &mut Context<Self>) {
        let Some(mask) = self.selected(cx).filter(|n| n.kind == "mask") else {
            return;
        };
        if !self.canvas.read(cx).is_editable() {
            return;
        }
        let result = (|| {
            let context = self.mask_views.context(&mask.id).map_err(CreativeError)?;
            let data = MaskNodeData::from_value(&mask.data)?;
            if !data.output_is_current(&context.source) {
                return Err(CreativeError("请先应用当前遮罩".into()));
            }
            let mut graph = self.canvas.read(cx).editor.graph().clone();
            let source = graph
                .edges
                .values()
                .find(|e| e.target.node == mask.id && e.target.port.as_str() == "image")
                .map(|e| e.source.clone())
                .ok_or_else(|| CreativeError("缺少原图输入".into()))?;
            let id: NodeId = graph.fresh_id("image-edit").into();
            let node = self.registry.make_node(
                "edit_image",
                id.clone(),
                WorldPoint::new(mask.position.x + mask.size.width + 96., mask.position.y),
                None,
            )?;
            graph.add_node(node)?;
            graph.connect(
                graph.fresh_id("edit-image"),
                source,
                PortRef::new(id.clone(), "image"),
            )?;
            graph.connect(
                graph.fresh_id("edit-mask"),
                PortRef::new(mask.id, "mask"),
                PortRef::new(id.clone(), "mask"),
            )?;
            self.registry.validate_graph(&graph)?;
            if self.command(CanvasCommand::ImportGraph(graph), cx) {
                self.clear_mask_edit();
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
            Ok::<_, CreativeError>(())
        })();
        if let Err(error) = result {
            self.notice = Some(error.to_string());
            cx.notify();
        }
    }
    pub(super) fn apply_mask(&mut self, cx: &mut Context<Self>) {
        if self.saving || self.importing || !self.canvas.read(cx).is_editable() {
            return;
        }
        let Some(node) = self.selected(cx).filter(|n| n.kind == "mask" && !n.locked) else {
            return;
        };
        let result = (|| {
            let mut project = self
                .project
                .clone()
                .ok_or_else(|| CreativeError("请先打开本地项目".into()))?;
            let context = self.mask_views.context(&node.id).map_err(CreativeError)?;
            let data = MaskNodeData::from_value(&node.data)?;
            if !data.state.source_matches(&context.source) {
                return Err(CreativeError("请确认原图并重置遮罩".into()));
            }
            let document = self.canvas.read(cx).editor.document();
            let registry = self.registry.clone();
            let expected = node.data.clone();
            let target = node.id.clone();
            let project_id = project.metadata().id.clone();
            let source = context.source;
            let expected_source = source.clone();
            let output_data = data.clone();
            let cancel = Arc::new(AtomicBool::new(false));
            self.mask_cancel = Some(cancel.clone());
            self.importing = true;
            self.canvas.update(cx, |v, cx| v.set_editable(false, cx));
            self.notice = Some("正在生成与原图同尺寸的Mask…".into());
            cx.notify();
            cx.spawn(async move |this, cx| {
                let result = cx
                    .background_executor()
                    .spawn(async move {
                        let raster = crate::mask::rasterize(
                            &data.state,
                            &source,
                            MaskRasterSize::Source,
                            &cancel,
                        )
                        .map_err(|e| e.to_string())?;
                        if cancel.load(Ordering::Acquire) {
                            return Err("遮罩应用已取消".to_string());
                        }
                        project
                            .save_document(&document, &registry)
                            .map_err(|e| e.to_string())?;
                        let asset = project
                            .store_mask(&source, &raster, &cancel)
                            .map_err(|e| e.to_string());
                        Ok::<_, String>((project, asset))
                    })
                    .await;
                let _ = this.update(cx, |this, cx| {
                    this.importing = false;
                    this.mask_cancel = None;
                    this.canvas.update(cx, |v, cx| v.set_editable(true, cx));
                    match result {
                        Ok((project, asset_result)) => {
                            let same_project = this
                                .project
                                .as_ref()
                                .is_some_and(|p| p.metadata().id == project_id);
                            let same_node = this
                                .canvas
                                .read(cx)
                                .editor
                                .graph()
                                .node(&target)
                                .is_some_and(|n| n.data == expected);
                            let same_source = this
                                .mask_views
                                .context(&target)
                                .is_ok_and(|c| c.source == expected_source);
                            if same_project {
                                this.last_saved_document = Some(project.document().clone());
                                this.project = Some(project);
                                this.refresh_assets(cx);
                                if let Ok(asset) = asset_result.as_ref()
                                    && same_node
                                    && same_source
                                {
                                    let mut data = output_data;
                                    data.asset = asset.key.clone();
                                    data.output_revision = Some(data.revision);
                                    match serde_json::to_value(data) {
                                        Ok(data) => {
                                            this.command(
                                                CanvasCommand::SetNodeData { node: target, data },
                                                cx,
                                            );
                                            this.notice = Some("Mask已应用，原图保持不变".into());
                                        }
                                        Err(error) => this.notice = Some(error.to_string()),
                                    }
                                } else if let Err(error) = asset_result {
                                    this.notice = Some(error);
                                    this.pending_window_close = None;
                                } else {
                                    this.notice = Some("遮罩编辑已改变，未覆盖新的节点内容".into());
                                }
                            }
                        }
                        Err(error) => {
                            this.notice = Some(error);
                            this.pending_window_close = None;
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
            Ok::<_, CreativeError>(())
        })();
        if let Err(error) = result {
            self.notice = Some(error.to_string());
            cx.notify();
        }
    }
    pub(super) fn cancel_mask_apply(&mut self, cx: &mut Context<Self>) {
        if let Some(cancel) = &self.mask_cancel {
            cancel.store(true, Ordering::Release);
            self.notice = Some("正在取消遮罩应用…".into());
            cx.notify();
        }
    }
    pub(super) fn mask_input_row(&self, node: &Node, cx: &App) -> Div {
        let graph = self.canvas.read(cx).editor.graph();
        let image = graph
            .edges
            .values()
            .any(|e| e.target.node == node.id && e.target.port.as_str() == "image");
        let mask = graph
            .edges
            .values()
            .any(|e| e.target.node == node.id && e.target.port.as_str() == "mask");
        let valid = crate::mask_node::validate_edit_inputs(node, graph).is_ok();
        div()
            .text_size(px(11.))
            .text_color(rgb(0x8f8293))
            .py_1()
            .child(format!(
                "Image：{} · Mask：{}",
                if image { "已连接" } else { "未连接" },
                if !mask {
                    "未连接"
                } else if valid {
                    "已应用"
                } else {
                    "需检查原图或应用"
                }
            ))
    }
}
