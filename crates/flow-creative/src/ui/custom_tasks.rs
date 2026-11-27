//! Explicit host execution and per-project custom-node definitions.
use super::*;
use crate::custom_nodes::*;
use gpui_flow_core::{PortDirection, PortRef};
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) struct CustomJob {
    pub node: NodeId,
    pub cancel: Arc<AtomicBool>,
}
#[derive(Clone)]
pub(super) enum CustomAction {
    Run,
    Cancel,
}
#[derive(Clone)]
pub(super) struct CustomPreview {
    pub value: Option<CustomValue>,
    pub status: String,
}
fn custom_type_label(data_type: &gpui_flow_core::DataType) -> String {
    use gpui_flow_core::DataType;
    match data_type {
        DataType::String => tr("文字", "Text").into(),
        DataType::Number => tr("数值", "Number").into(),
        DataType::Bool => tr("布尔", "Boolean").into(),
        DataType::Object => tr("对象", "Object").into(),
        DataType::Named(name) => match name.as_str() {
            "image" => tr("图片", "Image"),
            "video" => tr("视频", "Video"),
            "audio" => tr("音频", "Audio"),
            "model" => tr("模型", "Model"),
            "mask" => tr("遮罩", "Mask"),
            _ => name,
        }
        .into(),
        _ => tr("未支持类型", "Unsupported type").into(),
    }
}

impl CreativeCanvas {
    pub fn register_custom_executor(
        &mut self,
        id: impl Into<String>,
        executor: Arc<dyn CustomNodeExecutor>,
        cx: &mut Context<Self>,
    ) -> Result<(), CreativeError> {
        self.custom_executors.register(id, executor)?;
        cx.notify();
        Ok(())
    }
    pub(super) fn custom_executor_ids(&self) -> Vec<String> {
        self.custom_executors.ids()
    }
    pub(super) fn install_custom_design(
        &mut self,
        mut definition: CustomNodeDefinition,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if self.saving || self.importing || !self.canvas.read(cx).is_editable() {
            return Err(tr("当前画布不可编辑", "The canvas is read-only").into());
        }
        // A mismatched pass-through design remains a valid, explicit contract;
        // choose another host interface before executing it.
        if definition.executor == "local.pass_through" {
            crate::custom_host::configure_pass_through(&mut definition)
                .map_err(|e| e.to_string())?;
        }
        definition.validate().map_err(|e| e.to_string())?;
        let mut graph = self.canvas.read(cx).editor.graph().clone();
        let mut definitions = custom_definitions_from_graph(&graph).map_err(|e| e.to_string())?;
        if definitions.iter().any(|d| d.kind == definition.kind) {
            return Err(tr("节点类型名称已存在", "The node type name already exists").into());
        }
        definitions.push(definition.clone());
        if !graph.data.is_object() {
            graph.data = serde_json::json!({});
        }
        graph.data[CUSTOM_NODE_TYPES_KEY] =
            serde_json::to_value(definitions).map_err(|e| e.to_string())?;
        let registry =
            registry_with_custom_definitions(&self.registry, &graph).map_err(|e| e.to_string())?;
        let id: NodeId = graph.fresh_id("custom-object").into();
        let mut position = self
            .canvas
            .read(cx)
            .editor
            .viewport
            .screen_to_world(WorldPoint::new(
                self.canvas_size.width / 2. - 180.,
                self.canvas_size.height / 2. - 120.,
            ));
        if let Some(source) = self.selected(cx) {
            position = WorldPoint::new(
                source.position.x + source.size.width + 96.,
                source.position.y,
            );
        }
        let mut node = registry
            .make_node(&definition.kind, id.clone(), position, None)
            .map_err(|e| e.to_string())?;
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
        graph.add_node(node).map_err(|e| e.to_string())?;
        // Creating from a selected object makes its single matching first input
        // explicit on the canvas. No hidden reference or implicit execution.
        if let Some(source) = self.selected(cx)
            && let Some(input) = definition.inputs.first()
            && let Some(port) = source.ports.iter().find(|p| {
                p.direction == PortDirection::Output && p.data_type == input.data_type.data_type()
            })
        {
            graph
                .connect(
                    graph.fresh_id("custom-input"),
                    PortRef::new(source.id, port.id.clone()),
                    PortRef::new(id.clone(), input.id.clone()),
                )
                .map_err(|e| e.to_string())?;
        }
        self.registry
            .validate_graph(&graph)
            .map_err(|e| e.to_string())?;
        if !self.command(CanvasCommand::ImportGraph(graph), cx) {
            return Err(self
                .notice
                .clone()
                .unwrap_or_else(|| tr("创建节点失败", "Could not create node").into()));
        }
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
        self.refresh_assets(cx);
        Ok(())
    }
    pub(super) fn sync_custom_views(&mut self, cx: &App) {
        let graph = self.canvas.read(cx).editor.graph();
        let mut views = BTreeMap::new();
        let mut resolver = CustomOutputResolver::new(graph);
        for node in graph
            .nodes
            .values()
            .filter(|n| n.kind.starts_with("custom."))
        {
            let outputs = node.outputs().collect::<Vec<_>>();
            let values = outputs
                .iter()
                .map(|p| resolver.resolve(&PortRef::new(node.id.clone(), p.id.clone())))
                .collect::<Result<Vec<_>, _>>();
            let (value, status) = match values {
                Ok(values) => (
                    if values.len() == 1 {
                        values.into_iter().next()
                    } else {
                        None
                    },
                    tr_format!(
                        "{} 个类型化输出已就绪",
                        "{} typed outputs ready",
                        outputs.len()
                    ),
                ),
                Err(_) => (
                    None,
                    match node.data["status"].as_str() {
                        Some("failed") => tr(
                            "执行失败，请查看下方详情",
                            "Execution failed. See details below.",
                        )
                        .into(),
                        Some("cancelled") => tr("执行已取消", "Execution canceled").into(),
                        Some("unavailable") => {
                            tr("宿主接口尚未接入", "Host executor unavailable").into()
                        }
                        Some("complete") => tr(
                            "输入或参数已变更，请重新运行",
                            "Inputs or parameters changed. Run again.",
                        )
                        .into(),
                        _ => tr(
                            "连接输入后运行已注册接口",
                            "Connect inputs to run the registered executor",
                        )
                        .into(),
                    },
                ),
            };
            views.insert(node.id.clone(), CustomPreview { value, status });
        }
        *self.custom_views.lock().unwrap() = views;
    }
    pub(super) fn custom_action(&mut self, action: CustomAction, cx: &mut Context<Self>) {
        if matches!(action, CustomAction::Cancel) {
            if let Some(job) = &self.custom_job {
                job.cancel.store(true, Ordering::Release);
                self.notice = Some(
                    tr(
                        "正在等待宿主接口取消…",
                        "Waiting for the host executor to cancel…",
                    )
                    .into(),
                );
                cx.notify();
            }
            return;
        }
        if self.saving
            || self.importing
            || self.custom_job.is_some()
            || !self.canvas.read(cx).is_editable()
        {
            return;
        }
        let Some(node) = self
            .selected(cx)
            .filter(|n| n.kind.starts_with("custom.") && !n.locked)
        else {
            return;
        };
        let result = (|| {
            let graph = self.canvas.read(cx).editor.graph();
            let definition = custom_definitions_from_graph(graph)?
                .into_iter()
                .find(|d| d.kind == node.kind)
                .ok_or_else(|| {
                    CreativeError(
                        tr(
                            "自定义节点声明不存在",
                            "The custom node definition is missing",
                        )
                        .into(),
                    )
                })?;
            if !self.custom_executors.is_available(&definition.executor) {
                return Err(CreativeError(
                    tr(
                        "宿主接口尚未接入，当前不能执行",
                        "The host executor is unavailable and cannot run",
                    )
                    .into(),
                ));
            }
            let request = CustomNodeRequest::from_node(&node, graph, &definition)?;
            let project = self.project.as_ref().ok_or_else(|| {
                CreativeError(tr("请先打开本地项目", "Open a local project first").into())
            })?;
            for input in request.inputs.values() {
                crate::custom_host::validate_custom_value_assets(&input.value, project.assets())?;
            }
            let project_id = project.metadata().id.clone();
            let expected = node.data.clone();
            let target = node.id.clone();
            let executors = self.custom_executors.clone();
            let cancel = Arc::new(AtomicBool::new(false));
            self.custom_job = Some(CustomJob {
                node: target.clone(),
                cancel: cancel.clone(),
            });
            self.importing = true;
            self.canvas.update(cx, |v, cx| v.set_editable(false, cx));
            cx.notify();
            cx.spawn(async move |this, cx| {
                let task_definition = definition.clone();
                let task_request = request.clone();
                let result = cx
                    .background_executor()
                    .spawn(
                        async move { executors.execute(&task_definition, &task_request, &cancel) },
                    )
                    .await;
                let _ = this.update(cx, |this, cx| {
                    this.custom_job = None;
                    this.importing = false;
                    this.canvas.update(cx, |v, cx| v.set_editable(true, cx));
                    let graph = this.canvas.read(cx).editor.graph();
                    let current = graph.node(&target).cloned();
                    let same = this
                        .project
                        .as_ref()
                        .is_some_and(|p| p.metadata().id == project_id)
                        && current.as_ref().is_some_and(|n| {
                            n.data == expected
                                && CustomNodeRequest::from_node(n, graph, &definition)
                                    .is_ok_and(|r| r == request)
                        });
                    if same {
                        let result = result.and_then(|result| {
                            let project = this.project.as_ref().expect("same project checked");
                            for value in result.outputs.values() {
                                crate::custom_host::validate_custom_value_assets(
                                    value,
                                    project.assets(),
                                )?;
                            }
                            Ok(result)
                        });
                        if let Ok(mut data) = CustomNodeData::from_value(&expected, &definition) {
                            match result {
                                Ok(result) => {
                                    if let Err(e) = data.apply_result(&definition, &request, result)
                                    {
                                        this.notice = Some(e.to_string());
                                    }
                                }
                                Err(error) => {
                                    data.status = match error {
                                        CustomExecutionError::Cancelled => {
                                            CustomNodeStatus::Cancelled
                                        }
                                        CustomExecutionError::Unavailable { .. } => {
                                            CustomNodeStatus::Unavailable
                                        }
                                        _ => CustomNodeStatus::Failed,
                                    };
                                    data.outputs.clear();
                                    data.completed_request = None;
                                    data.error =
                                        Some(error.to_string().chars().take(1000).collect());
                                }
                            }
                            let image_size = if data.outputs.len() == 1 {
                                data.outputs.values().next().and_then(|value| match value {
                                    CustomValue::Image { asset_key } => this
                                        .project
                                        .as_ref()
                                        .and_then(|p| {
                                            p.assets().iter().find(|a| a.key == *asset_key)
                                        })
                                        .map(|a| {
                                            project_ui::imported_image_size(a.width, a.height)
                                        }),
                                    _ => None,
                                })
                            } else {
                                None
                            };
                            if let Ok(data) = serde_json::to_value(data) {
                                let mut graph = this.canvas.read(cx).editor.graph().clone();
                                if let Some(node) = graph.nodes.get_mut(&target) {
                                    node.data = data;
                                    if let Some(size) = image_size {
                                        node.size = size;
                                    }
                                }
                                this.command(CanvasCommand::ImportGraph(graph), cx);
                            }
                        }
                    } else {
                        this.notice = Some(
                            tr(
                                "节点或输入已变化，未接收旧执行结果",
                                "The node or inputs changed. The previous result was discarded.",
                            )
                            .into(),
                        );
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
    pub(super) fn custom_composer(
        &self,
        node: &Node,
        editable: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let graph = self.canvas.read(cx).editor.graph();
        let definition = custom_definitions_from_graph(graph)
            .ok()
            .and_then(|d| d.into_iter().find(|d| d.kind == node.kind));
        let running = self.custom_job.as_ref().is_some_and(|j| j.node == node.id);
        let ready = definition
            .as_ref()
            .is_some_and(|d| self.custom_executors.is_available(&d.executor));
        let valid = definition
            .as_ref()
            .is_some_and(|d| CustomNodeRequest::from_node(node, graph, d).is_ok());
        let mut panel = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(div().text_sm().child(tr("自定义节点", "Custom Node")));
        for input in node.inputs() {
            let value = graph
                .edges
                .values()
                .find(|e| e.target.node == node.id && e.target.port == input.id)
                .and_then(|e| resolve_graph_output(graph, &e.source).ok());
            panel = panel.child(label(format!(
                "{} · {} · {}",
                input.label,
                custom_type_label(&input.data_type),
                if value.is_some() {
                    tr("已连接", "Connected")
                } else {
                    tr("等待有效输入", "Waiting for valid input")
                }
            )));
        }
        panel = panel.child(label(if running {
            tr("宿主接口执行中…", "Host executor running…").into()
        } else if !ready {
            tr(
                "接口尚未接入，当前不能执行",
                "The executor is unavailable and cannot run",
            )
            .into()
        } else {
            tr_format!(
                "接口 · {}",
                "Executor · {}",
                definition
                    .as_ref()
                    .map(|d| d.executor.as_str())
                    .unwrap_or_default()
            )
        }));
        if let Some(error) = node.data["error"].as_str() {
            panel = panel.child(
                div()
                    .text_size(px(11.))
                    .text_color(rgb(0xb07150))
                    .child(error.chars().take(150).collect::<String>()),
            );
        }
        panel.child(self.button(
            "run-custom",
            if running {
                tr("取消执行", "Cancel execution")
            } else {
                tr("运行接口", "Run Executor")
            },
            Action::Custom(if running {
                CustomAction::Cancel
            } else {
                CustomAction::Run
            }),
            !running && (!editable || !ready || !valid || self.importing || self.saving),
            cx,
        ))
    }
}
