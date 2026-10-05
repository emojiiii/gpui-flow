//! One cancellable local media job at a time; inputs are project-owned copies.
use super::processing_panel::ProcessingAction;
use super::*;
use crate::processing::{ProcessingKind, execute_processing};
use crate::processing_node::{ProcessingNodeData, processing_source};
use gpui_flow_core::PortRef;
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) struct ProcessingJob {
    pub node: NodeId,
    pub cancel: Arc<AtomicBool>,
}
struct ProcessingScratch {
    directory: PathBuf,
    output: PathBuf,
}
impl ProcessingScratch {
    fn new(extension: &str) -> Result<Self, String> {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos();
        let directory =
            std::env::temp_dir().join(format!("flow-process-{}-{nonce}", std::process::id()));
        std::fs::create_dir(&directory).map_err(|e| e.to_string())?;
        let output = directory.join(format!("processed.{extension}"));
        Ok(Self { directory, output })
    }
}
impl Drop for ProcessingScratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.output);
        let _ = std::fs::remove_dir(&self.directory);
    }
}

impl CreativeCanvas {
    pub(super) fn sync_processing_views(&mut self, cx: &App) {
        let graph = self.canvas.read(cx).editor.graph();
        let errors = graph
            .nodes
            .values()
            .filter(|node| ProcessingKind::from_node_kind(&node.kind).is_some())
            .filter_map(|node| {
                crate::processing_node::processing_output(graph, node)
                    .err()
                    .map(|e| (node.id.clone(), e.to_string()))
            })
            .collect();
        *self.processing_errors.lock().unwrap() = errors;
    }
    pub(super) fn processing_action(&mut self, action: ProcessingAction, cx: &mut Context<Self>) {
        match action {
            ProcessingAction::Cancel => {
                if let Some(job) = &self.processing_job {
                    job.cancel.store(true, Ordering::Release);
                    self.notice = Some("正在取消本地处理…".into());
                    cx.notify();
                }
            }
            ProcessingAction::Run => self.run_processing(cx),
            ProcessingAction::SetSettings(settings) => {
                if self.importing || self.saving || !self.canvas.read(cx).is_editable() {
                    return;
                }
                let Some(node) = self.selected(cx).filter(|n| !n.locked) else {
                    return;
                };
                let result = (|| {
                    let mut data = ProcessingNodeData::from_value(&node.data)?;
                    data.set_settings(settings)?;
                    Ok::<_, CreativeError>(serde_json::to_value(data)?)
                })();
                match result {
                    Ok(data) => {
                        self.command(
                            CanvasCommand::SetNodeData {
                                node: node.id,
                                data,
                            },
                            cx,
                        );
                    }
                    Err(error) => {
                        self.notice = Some(error.to_string());
                        cx.notify();
                    }
                }
            }
        }
    }
    pub(super) fn create_processing_from_selection(
        &mut self,
        kind: ProcessingKind,
        cx: &mut Context<Self>,
    ) {
        if !self.canvas.read(cx).is_editable() {
            return;
        }
        let Some(source) = self.selected(cx) else {
            return;
        };
        let Some(port) = source.ports.iter().find(|p| {
            p.direction == gpui_flow_core::PortDirection::Output && p.data_type == kind.input_type()
        }) else {
            return;
        };
        let result = (|| {
            let mut graph = self.canvas.read(cx).editor.graph().clone();
            let id: NodeId = graph.fresh_id(kind.node_kind()).into();
            let mut node = self.registry.make_node(
                kind.node_kind(),
                id.clone(),
                WorldPoint::new(
                    source.position.x + source.size.width + 96.,
                    source.position.y,
                ),
                None,
            )?;
            node.size = source.size;
            for _ in 0..MAX_CARDS {
                let next = graph
                    .nodes
                    .values()
                    .filter(|other| node.bounds().expanded(24.).intersects(other.bounds()))
                    .map(|other| other.bounds().max_x() + 96.)
                    .max_by(f32::total_cmp);
                if let Some(x) = next {
                    node.position.x = x;
                } else {
                    break;
                }
            }
            graph.add_node(node)?;
            graph.connect(
                graph.fresh_id("process-input"),
                PortRef::new(source.id.clone(), port.id.clone()),
                PortRef::new(id.clone(), "input"),
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
                self.command(
                    CanvasCommand::Fit {
                        canvas: self.canvas_size,
                        padding: 160.,
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
    fn run_processing(&mut self, cx: &mut Context<Self>) {
        if self.processing_job.is_some()
            || self.saving
            || self.importing
            || !self.canvas.read(cx).is_editable()
        {
            return;
        }
        let Some(node) = self
            .selected(cx)
            .filter(|n| !n.locked && ProcessingKind::from_node_kind(&n.kind).is_some())
        else {
            return;
        };
        let result = (|| {
            let mut project = self
                .project
                .clone()
                .ok_or_else(|| CreativeError("请先打开本地项目".into()))?;
            let source =
                processing_source(self.canvas.read(cx).editor.graph(), &node, project.assets())?;
            let path = project
                .resolve_asset_path(&source.key)
                .map_err(|e| CreativeError(e.to_string()))?;
            let data = ProcessingNodeData::from_value(&node.data)?;
            let document = self.canvas.read(cx).editor.document();
            let registry = self.registry.clone();
            let expected = node.data.clone();
            let source_key = source.key.clone();
            let expected_source = source_key.clone();
            let target = node.id.clone();
            let project_id = project.metadata().id.clone();
            let cancel = Arc::new(AtomicBool::new(false));
            self.processing_job = Some(ProcessingJob {
                node: target.clone(),
                cancel: cancel.clone(),
            });
            self.importing = true;
            self.canvas.update(cx, |v, cx| v.set_editable(false, cx));
            self.notice = Some("正在本地处理媒体…".into());
            let settings = data.settings.clone();
            let result_cancel = cancel.clone();
            cx.notify();
            cx.spawn(async move |this, cx| {
                let result = cx
                    .background_executor()
                    .spawn(async move {
                        project
                            .save_document(&document, &registry)
                            .map_err(|e| e.to_string())?;
                        let output = (|| {
                            let scratch = ProcessingScratch::new(settings.extension())?;
                            execute_processing(&settings, &path, &source, &scratch.output, &cancel)
                                .map_err(|e| e.to_string())?;
                            if cancel.load(Ordering::Acquire) {
                                return Err("本地处理已取消".into());
                            }
                            let asset = match settings.kind() {
                                ProcessingKind::Image => project.import_image(&scratch.output),
                                ProcessingKind::Video | ProcessingKind::Audio => {
                                    project.import_media(&scratch.output)
                                }
                            }
                            .map_err(|e| e.to_string())?;
                            Ok::<_, String>(asset)
                        })();
                        Ok::<_, String>((project, output))
                    })
                    .await;
                let _ = this.update(cx, |this, cx| {
                    this.processing_job = None;
                    this.importing = false;
                    this.canvas.update(cx, |v, cx| v.set_editable(true, cx));
                    match result {
                        Ok((project, asset_result)) => {
                            let same_project = this
                                .project
                                .as_ref()
                                .is_some_and(|p| p.metadata().id == project_id);
                            let current =
                                this.canvas.read(cx).editor.graph().node(&target).cloned();
                            let same_node = current.as_ref().is_some_and(|n| n.data == expected);
                            let same_source = current.as_ref().is_some_and(|n| {
                                processing_source(
                                    this.canvas.read(cx).editor.graph(),
                                    n,
                                    project.assets(),
                                )
                                .is_ok_and(|a| a.key == expected_source)
                            });
                            if same_project {
                                this.last_saved_document = Some(project.document().clone());
                                this.project = Some(project);
                                this.refresh_assets(cx);
                                if same_node && same_source {
                                    let mut output_data = data;
                                    match asset_result {
                                        Ok(asset) if !result_cancel.load(Ordering::Acquire) => {
                                            output_data.asset = asset.key;
                                            output_data.source_asset = Some(source_key);
                                            output_data.output_settings =
                                                Some(output_data.settings.clone());
                                            output_data.error = None;
                                            this.notice = Some(format!(
                                                "本地处理完成 · {} 字节 · 原素材保留",
                                                asset.byte_len
                                            ));
                                        }
                                        Ok(_) => {
                                            this.notice = Some("处理已取消，未覆盖节点输出".into());
                                        }
                                        Err(error) => {
                                            output_data.error =
                                                Some(error.chars().take(1000).collect());
                                            this.notice = Some(error);
                                        }
                                    }
                                    if let Ok(data) = serde_json::to_value(output_data) {
                                        this.command(
                                            CanvasCommand::SetNodeData { node: target, data },
                                            cx,
                                        );
                                    }
                                } else {
                                    this.notice = Some("输入或节点已改变，未覆盖当前内容".into());
                                }
                            }
                        }
                        Err(error) => {
                            this.notice = Some(error);
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
}
