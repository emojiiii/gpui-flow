use crate::*;
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, FlowCanvas, FlowEvent, FlowOptions, FlowTheme, NodeRegistry, NodeRenderContext,
    NodeRenderer,
};
use gpui_flow_core::{FlowDocument, Node, NodeId, Point as WorldPoint, Size as WorldSize};
use gpui_flow_interaction::{CanvasCommand, SelectionMode};
use gpui_kit::component::input::{InputEvent, Textarea, TextareaState};
use serde_json::Value;
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

/// Data contains asset keys only; the host explicitly authorizes local sources.
#[derive(Clone, Default)]
pub struct CreativeAssets {
    paths: BTreeMap<String, PathBuf>,
}
impl CreativeAssets {
    pub fn register(&mut self, key: impl Into<String>, path: impl Into<PathBuf>) {
        self.paths.insert(key.into(), path.into());
    }
    pub fn demo() -> Self {
        let mut this = Self::default();
        this.register(
            "botanical",
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/botanical.svg"),
        );
        this.register(
            "sunset",
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/sunset.svg"),
        );
        this
    }
}
struct CardRenderer {
    assets: Arc<CreativeAssets>,
}
impl NodeRenderer for CardRenderer {
    fn full_bleed(&self) -> bool {
        true
    }
    fn render(
        &self,
        node: &Node,
        context: NodeRenderContext,
        _: &mut Window,
        _: &mut App,
    ) -> AnyElement {
        let z = context.zoom;
        let key = node.data["asset"].as_str().unwrap_or("");
        if let Some(path) = self.assets.paths.get(key) {
            return img(path.clone())
                .size_full()
                .object_fit(ObjectFit::Cover)
                .with_loading(|| {
                    div()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_sm()
                        .text_color(rgb(0x90939d))
                        .child("正在读取本地图片…")
                        .into_any_element()
                })
                .with_fallback(|| {
                    div()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_sm()
                        .text_color(rgb(0xb07150))
                        .child("图片解码失败，请替换素材")
                        .into_any_element()
                })
                .into_any_element();
        }
        if node.kind == "text" {
            return div()
                .size_full()
                .p(px(24. * z))
                .bg(rgb(0xffffff))
                .text_size(px(21. * z))
                .text_color(rgb(0x32343a))
                .child(node.data["text"].as_str().unwrap_or("").to_string())
                .into_any_element();
        }
        let (symbol, title, hint, background) = match node.kind.as_str() {
            "generate_image" => ("▧", "图片生成", "选择节点，写下你的画面", 0xeaf0fc),
            "generate_video" => ("▷", "视频生成", "从一个镜头开始", 0xfff1e9),
            "generate_audio" => ("♫", "音频生成", "声音、音乐与独立音效", 0xe7f5f0),
            "generate_model" => ("◇", "3D 模型生成", "描述形状、材质与风格", 0xeeeefa),
            _ => ("▧", "导入本地图片", "PNG · JPG · GIF · WebP", 0xf0f1f4),
        };
        let status = match node.data["status"].as_str() {
            Some("queued") => "已交给宿主排队",
            Some("running") => "宿主正在生成",
            Some("failed") => "生成失败，详情见宿主",
            Some("complete") => "宿主已完成，等待可预览素材",
            _ => hint,
        };
        div()
            .size_full()
            .bg(rgb(background))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(10. * z))
            .child(
                div()
                    .text_size(px(44. * z))
                    .text_color(rgb(0x8a94ab))
                    .child(symbol),
            )
            .child(
                div()
                    .text_size(px(16. * z))
                    .text_color(rgb(0x697184))
                    .child(title),
            )
            .child(
                div()
                    .text_size(px(11. * z))
                    .text_color(rgb(0x8e95a2))
                    .child(status),
            )
            .into_any_element()
    }
}
/// Example extension: custom payload + exact named port + an ordinary public NodeRenderer.
pub struct PaletteRenderer;
impl NodeRenderer for PaletteRenderer {
    fn render(
        &self,
        node: &Node,
        context: NodeRenderContext,
        _: &mut Window,
        _: &mut App,
    ) -> AnyElement {
        let z = context.zoom;
        let colors = node.data["colors"].as_array().cloned().unwrap_or_default();
        div()
            .p(px(14. * z))
            .flex()
            .flex_col()
            .gap(px(12. * z))
            .child(
                div().flex().gap(px(7. * z)).children(
                    colors
                        .iter()
                        .filter_map(|c| c.as_str())
                        .filter_map(|s| u32::from_str_radix(s.trim_start_matches('#'), 16).ok())
                        .map(|c| div().w(px(43. * z)).h(px(45. * z)).rounded_md().bg(rgb(c))),
                ),
            )
            .child(
                div()
                    .text_size(px(11. * z))
                    .text_color(rgb(context.theme.muted))
                    .child(node.data["note"].as_str().unwrap_or("").to_string()),
            )
            .into_any_element()
    }
}
pub fn demo_renderers(assets: CreativeAssets) -> NodeRegistry {
    let assets = Arc::new(assets);
    let mut registry = NodeRegistry::default();
    for kind in [
        "text",
        "image",
        "compose",
        "generate_image",
        "generate_video",
        "generate_audio",
        "generate_model",
    ] {
        registry.register(
            kind,
            CardRenderer {
                assets: assets.clone(),
            },
        );
    }
    registry.register("palette", PaletteRenderer);
    registry
}
fn theme() -> FlowTheme {
    FlowTheme {
        background: 0xf5f6f8,
        panel: 0xffffff,
        card: 0xffffff,
        border: 0xe6e7eb,
        foreground: 0x27282d,
        muted: 0x90939d,
        accent: 0x4d8bea,
        grid: 0xe1e3e8,
    }
}
#[derive(Clone)]
enum Action {
    Home,
    ImportImage,
    ReplaceImage,
    ImportImagePath,
    CloseImportPath,
    Duplicate,
    Generate,
    Colors,
    PortColor(gpui_flow_core::DataType, u32),
    CustomPortColor(gpui_flow_core::DataType),
    Setting(&'static str, Value),
    Add(String),
    Undo,
    Redo,
    Delete,
    Frame,
    Ungroup,
    Fit,
    Zoom(f32),
    ReadOnly,
    Data,
    Export,
    Records,
    Import,
    Apply,
    Cancel,
    Save,
    Open,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum JsonMode {
    Document,
    Records,
}

/// Reusable host shell. It embeds one FlowCanvas and the existing editor/ports/history.
/// Register payload contracts before construction; UI renderers remain a separate host concern.
pub struct CreativeCanvas {
    canvas: Entity<FlowCanvas>,
    registry: Arc<CreativeRegistry>,
    form: Entity<TextareaState>,
    json: Entity<TextareaState>,
    custom_color: Entity<TextareaState>,
    local_path: Entity<TextareaState>,
    show_import_path: bool,
    form_source: Option<(NodeId, String, Value)>,
    show_data: bool,
    json_mode: JsonMode,
    canvas_size: WorldSize,
    next_id: u64,
    notice: Option<String>,
    trace: bool,
    project: Option<LocalProject>,
    last_saved_document: Option<FlowDocument>,
    save_serial: u64,
    form_serial: u64,
    saving: bool,
    importing: bool,
    return_after_save: bool,
    pending_window_close: Option<AnyWindowHandle>,
    show_colors: bool,
    generation_available: bool,
    _subscriptions: Vec<Subscription>,
}
impl CreativeCanvas {
    pub fn new(
        registry: Arc<CreativeRegistry>,
        document: FlowDocument,
        renderers: NodeRegistry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Self, CreativeError> {
        registry.validate_document(&document)?;
        let guard = registry.clone();
        let validator = registry.clone();
        let canvas = cx.new(|cx| {
            let mut view = FlowCanvas::new(document.graph, cx).expect("document validated");
            view.editor.viewport = document.viewport;
            view.set_document_validator(Some(Arc::new(move |document| {
                validator
                    .validate_document(document)
                    .map_err(|e| e.to_string())
            })))
            .expect("initial document validated");
            view.registry = renderers;
            view.theme = theme();
            view.options = FlowOptions::canvas_only();
            view.options.show_controls = false;
            view.options.show_minimap = false;
            view.show_grid = false;
            view.set_selection_on_top(true);
            view.install_plugin(CreativeGuard(guard), cx)
                .expect("unique guard id");
            view
        });
        let subscription = cx.subscribe_in(&canvas, window, |this, canvas, event, window, cx| {
            match event {
                FlowEvent::DocumentChanged | FlowEvent::SelectionChanged => {
                    // A successful graph edit supersedes an earlier rejection. Selection
                    // alone must not erase an error the user still needs to understand.
                    if matches!(event, FlowEvent::DocumentChanged) {
                        this.notice = Some("画布已更新".into());
                    }
                    // Events can be queued after both the document and selection
                    // changed. Commit the old draft before rendering a new form.
                    this.apply_form_edit(cx);
                    this.sync_form(window, cx);
                    if matches!(event, FlowEvent::DocumentChanged) {
                        this.queue_autosave(cx);
                    }
                    if this.trace {
                        let v = canvas.read(cx);
                        eprintln!(
                            "creative: cards={} links={} frames={} selected={:?}",
                            v.editor.graph().nodes.len(),
                            v.editor.graph().edges.len(),
                            v.editor.graph().groups.len(),
                            v.editor.selection.nodes
                        );
                    }
                }
                FlowEvent::ViewportChanged => this.queue_autosave(cx),
                FlowEvent::SaveRequested => {
                    this.apply_form_edit(cx);
                    this.save_project(cx);
                }
                FlowEvent::ConnectionRejected(reason) => {
                    this.notice = Some(format!("连接被拒绝：{reason}"))
                }
                _ => {}
            }
            cx.notify();
        });
        let observer = cx.observe(&canvas, |_, _, cx| cx.notify());
        let form = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder("写下你的想法…")
                .context_menu(false)
        });
        let form_subscription = cx.subscribe(&form, |this, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                this.queue_form_edit(cx);
            }
            if matches!(event, InputEvent::Blur) {
                this.apply_form_edit(cx);
            }
        });
        Ok(Self {
            canvas,
            registry,
            form,
            json: cx.new(|cx| {
                TextareaState::new(window, cx)
                    .placeholder("导出文档或加载 records 模板")
                    .context_menu(false)
            }),
            custom_color: cx.new(|cx| {
                TextareaState::new(window, cx)
                    .placeholder("#RRGGBB，例如 #E86452")
                    .context_menu(false)
            }),
            local_path: cx.new(|cx| {
                TextareaState::new(window, cx)
                    .placeholder("本地图片绝对路径")
                    .context_menu(false)
            }),
            show_import_path: false,
            form_source: None,
            show_data: false,
            json_mode: JsonMode::Document,
            canvas_size: WorldSize::new(780., 700.),
            next_id: 1,
            notice: Some("拖动端口连接：仅同类型 Output → Input；没有模型调用".into()),
            trace: std::env::args().any(|a| a == "--trace"),
            project: None,
            last_saved_document: None,
            save_serial: 0,
            form_serial: 0,
            saving: false,
            importing: false,
            return_after_save: false,
            pending_window_close: None,
            show_colors: false,
            generation_available: false,
            _subscriptions: vec![subscription, observer, form_subscription],
        })
    }
    pub fn canvas(&self) -> Entity<FlowCanvas> {
        self.canvas.clone()
    }
    pub fn set_document_path(&mut self, path: Option<PathBuf>, cx: &mut Context<Self>) {
        self.canvas.update(cx, |v, _| v.document_path = path);
    }
    fn selected(&self, cx: &App) -> Option<Node> {
        let v = self.canvas.read(cx);
        if v.editor.selection.nodes.len() != 1 {
            return None;
        }
        v.editor
            .selection
            .nodes
            .iter()
            .next()
            .and_then(|id| v.editor.graph().node(id))
            .cloned()
    }
    fn sync_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let node = self.selected(cx);
        let source = node.as_ref().map(form_identity);
        if source == self.form_source {
            return;
        }
        self.form_source = source;
        let value = node
            .as_ref()
            .map(|n| {
                if let Some(field) = primary_field(n) {
                    n.data[field].as_str().unwrap_or("").to_string()
                } else {
                    serde_json::to_string_pretty(&n.data).unwrap_or_default()
                }
            })
            .unwrap_or_default();
        self.form.update(cx, |s, cx| s.set_value(value, window, cx));
    }
    fn command(&mut self, command: CanvasCommand, cx: &mut Context<Self>) -> bool {
        self.notice = None;
        let result = self
            .canvas
            .update(cx, |v, cx| v.execute_command(command, cx));
        self.notice = Some(match &result {
            Ok(()) => "操作完成".into(),
            Err(e) => e.to_string(),
        });
        cx.notify();
        result.is_ok()
    }
    fn action(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
        if action_flushes_form(&action) {
            self.apply_form_edit(cx);
        }
        let command = match action {
            Action::Home => {
                self.apply_form_edit(cx);
                self.return_after_save = true;
                self.save_project(cx);
                return;
            }
            Action::ImportImagePath => {
                self.import_explicit_image_path(cx);
                return;
            }
            Action::CloseImportPath => {
                self.show_import_path = false;
                cx.notify();
                return;
            }
            Action::ImportImage => {
                self.import_image(false, cx);
                return;
            }
            Action::ReplaceImage => {
                self.import_image(true, cx);
                return;
            }
            Action::Duplicate => CanvasCommand::Duplicate {
                offset: WorldPoint::new(32., 32.),
            },
            Action::Colors => {
                self.show_colors = !self.show_colors;
                cx.notify();
                return;
            }
            Action::CustomPortColor(data_type) => {
                let value = self.custom_color.read(cx).value();
                let value = value.trim().trim_start_matches('#');
                if value.len() == 6
                    && let Ok(color) = u32::from_str_radix(value, 16)
                {
                    self.action(Action::PortColor(data_type, color), window, cx);
                    return;
                }
                self.notice = Some("请输入6位十六进制颜色，例如 #E86452".into());
                cx.notify();
                return;
            }
            Action::Setting(key, value) => {
                let Some(mut node) = self.selected(cx) else {
                    return;
                };
                if !node.data["settings"].is_object() {
                    node.data["settings"] = serde_json::json!({});
                }
                node.data["settings"][key] = value;
                CanvasCommand::SetNodeData {
                    node: node.id,
                    data: node.data,
                }
            }
            Action::PortColor(data_type, color) => {
                let mut graph = self.canvas.read(cx).editor.graph().clone();
                if graph.data.is_null() {
                    graph.data = serde_json::json!({});
                }
                if !graph.data.is_object() {
                    self.notice = Some(
                        "当前文档的 metadata 不是对象，请先在数据面板保留内容并转成对象后设置颜色"
                            .into(),
                    );
                    cx.notify();
                    return;
                }
                if !graph.data["port_colors"].is_object() {
                    graph.data["port_colors"] = serde_json::json!({});
                }
                graph.data["port_colors"][gpui_flow::port_type_key(&data_type)] =
                    serde_json::json!(color);
                CanvasCommand::ImportGraph(graph)
            }
            Action::Generate => {
                self.action(Action::Apply, window, cx);
                if !self.generation_available {
                    self.notice = Some("尚未连接生成服务。提示词与参数仅保存在本地项目。".into());
                    cx.notify();
                    return;
                }
                if let Some(node) = self.selected(cx) {
                    match GenerationRequest::from_node(&node, self.canvas.read(cx).editor.graph()) {
                        Ok(request) => {
                            cx.emit(CreativeEvent::GenerationRequested(request));
                            self.notice = Some("生成请求已交给宿主；状态和输出由宿主返回".into());
                        }
                        Err(error) => self.notice = Some(error.to_string()),
                    }
                }
                cx.notify();
                return;
            }
            Action::Undo => CanvasCommand::Undo,
            Action::Redo => CanvasCommand::Redo,
            Action::Delete => CanvasCommand::DeleteSelection,
            Action::Frame => CanvasCommand::GroupSelection {
                title: "Frame / 创作分组".into(),
                padding: 28.,
            },
            Action::Ungroup => CanvasCommand::UngroupSelection,
            Action::Fit => CanvasCommand::Fit {
                canvas: self.canvas_size,
                padding: 42.,
            },
            Action::Zoom(factor) => CanvasCommand::Zoom {
                position: WorldPoint::new(
                    self.canvas_size.width / 2.,
                    self.canvas_size.height / 2.,
                ),
                zoom: (self.canvas.read(cx).editor.viewport.zoom * factor).clamp(0.1, 3.),
            },
            Action::Add(kind) => {
                let v = self.canvas.read(cx);
                let id = v
                    .editor
                    .graph()
                    .fresh_id(&format!("creative-{}", self.next_id));
                self.next_id += 1;
                let node_height = self
                    .registry
                    .definition(&kind)
                    .map_or(240., |d| d.size.height)
                    * v.editor.viewport.zoom;
                let composer_height = if GenerationKind::from_node_kind(&kind).is_some() {
                    286.
                } else {
                    146.
                };
                let at = v.editor.viewport.screen_to_world(WorldPoint::new(
                    self.canvas_size.width / 2. - 110.,
                    ((self.canvas_size.height - node_height - composer_height - 24.) / 2.).max(80.),
                ));
                match self.registry.make_node(&kind, id, at, None) {
                    Ok(node) => {
                        let id = node.id.clone();
                        if self.command(CanvasCommand::AddNode(node), cx) {
                            self.command(
                                CanvasCommand::SelectNode {
                                    node: id,
                                    mode: SelectionMode::Replace,
                                },
                                cx,
                            );
                        }
                    }
                    Err(e) => self.notice = Some(e.to_string()),
                }
                cx.notify();
                return;
            }
            Action::ReadOnly => {
                self.canvas
                    .update(cx, |v, cx| v.set_editable(!v.is_editable(), cx));
                cx.notify();
                return;
            }
            Action::Data => {
                self.show_data = !self.show_data;
                cx.notify();
                return;
            }
            Action::Export => {
                match self
                    .registry
                    .encode_document(&self.canvas.read(cx).editor.document())
                {
                    Ok(value) => {
                        self.json_mode = JsonMode::Document;
                        self.json.update(cx, |s, cx| s.set_value(value, window, cx));
                        self.notice=Some("已导出当前 FlowDocument，含位置/尺寸/类型端口/连接/Frame/自定义数据与视口".into());
                    }
                    Err(e) => self.notice = Some(e.to_string()),
                }
                cx.notify();
                return;
            }
            Action::Records => {
                self.json_mode = JsonMode::Records;
                let value = serde_json::to_string_pretty(&sample_records())
                    .expect("sample records serialize");
                self.json.update(cx, |s, cx| s.set_value(value, window, cx));
                self.notice = Some("records v1：明确kind/ID/端口；缺少位置时使用确定网格".into());
                cx.notify();
                return;
            }
            Action::Import => {
                let value = self.json.read(cx).value();
                let result = match self.json_mode {
                    JsonMode::Document => self.registry.parse_document(&value),
                    JsonMode::Records => self
                        .registry
                        .parse_records(&value, LayoutPolicy::Grid { columns: 3 }),
                };
                let result = result.and_then(|document| {
                    if let Some(project) = &self.project {
                        project
                            .validate_document(&document, &self.registry)
                            .map_err(|e| CreativeError(e.to_string()))?;
                    }
                    Ok(document)
                });
                match result {
                    Ok(document) => {
                        if self.command(CanvasCommand::ImportGraph(document.graph), cx) {
                            self.canvas.update(cx, |v, cx| {
                                v.editor.viewport = document.viewport;
                                cx.notify();
                            });
                            self.form_source = None;
                            self.sync_form(window, cx);
                            self.notice =
                                Some("已原子导入；图内容可一步撤销，视口属于导航状态".into());
                        }
                    }
                    Err(e) => self.notice = Some(format!("导入被拒绝，原画布保留：{e}")),
                }
                cx.notify();
                return;
            }
            Action::Apply => {
                let Some(mut node) = self.selected(cx) else {
                    return;
                };
                if self.form_source.as_ref() != Some(&form_identity(&node)) {
                    self.notice = Some("卡片已变化，请重新编辑".into());
                    self.sync_form(window, cx);
                    cx.notify();
                    return;
                }
                let value = self.form.read(cx).value();
                if let Some(field) = primary_field(&node) {
                    node.data[field] = Value::String(value.to_string());
                } else {
                    match serde_json::from_str(&value) {
                        Ok(data) => node.data = data,
                        Err(e) => {
                            self.notice = Some(e.to_string());
                            cx.notify();
                            return;
                        }
                    }
                }
                CanvasCommand::SetNodeData {
                    node: node.id,
                    data: node.data,
                }
            }
            Action::Cancel => {
                self.form_source = None;
                self.sync_form(window, cx);
                self.notice = Some("已放弃尚未应用的表单修改".into());
                cx.notify();
                return;
            }
            Action::Save => {
                self.apply_form_edit(cx);
                if self.project.is_some() {
                    self.save_project(cx);
                    return;
                }
                self.canvas.update(cx, |v, cx| v.save_file(cx));
                self.notice = None;
                cx.notify();
                return;
            }
            Action::Open => {
                self.canvas.update(cx, |v, cx| v.open_file(cx));
                self.notice = None;
                cx.notify();
                return;
            }
        };
        self.command(command, cx);
    }
    fn button(
        &self,
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        action: Action,
        disabled: bool,
        cx: &mut Context<Self>,
    ) -> ControlButton {
        ControlButton::new(id, label)
            .theme(theme())
            .disabled(disabled)
            .on_click(
                cx.listener(move |this, _, window, cx| this.action(action.clone(), window, cx)),
            )
    }
}
fn action_flushes_form(action: &Action) -> bool {
    !matches!(
        action,
        Action::Apply | Action::Cancel | Action::Undo | Action::Redo
    )
}
fn form_data_update(node: &Node, source: &(NodeId, String, Value), text: &str) -> Option<Value> {
    if source != &form_identity(node) {
        return None;
    }
    let field = primary_field(node)?;
    if node.data[field].as_str() == Some(text) {
        return None;
    }
    let mut data = node.data.clone();
    data[field] = Value::String(text.to_string());
    Some(data)
}
fn form_identity(node: &Node) -> (NodeId, String, Value) {
    (node.id.clone(), node.kind.clone(), node.data.clone())
}
fn primary_field(node: &Node) -> Option<&'static str> {
    let field = match node.kind.as_str() {
        "text" | "compose" => "text",
        "image" => "caption",
        "palette" => "note",
        "generate_image" | "generate_video" | "generate_audio" | "generate_model" => "prompt",
        _ => return None,
    };
    node.data.get(field).and_then(Value::as_str).map(|_| field)
}

fn label(text: impl Into<SharedString>) -> Div {
    let text: SharedString = text.into();
    div().text_xs().text_color(rgb(theme().muted)).child(text)
}
mod project_ui;
mod workbench;
pub use workbench::CreativeWorkbench;

#[derive(Clone, Debug)]
pub enum CreativeEvent {
    BackToProjects,
    GenerationRequested(GenerationRequest),
}
impl EventEmitter<CreativeEvent> for CreativeCanvas {}

impl Render for CreativeCanvas {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme();
        let view = self.canvas.read(cx);
        let editable = view.is_editable() && !self.importing;
        let selection = view.editor.selection.clone();
        let count = view.editor.graph().nodes.len();
        let zoom = view.editor.viewport.zoom;
        let viewport = view.editor.viewport;
        let node = self.selected(cx);
        let project_name = self
            .project
            .as_ref()
            .map(|p| p.metadata().name.clone())
            .unwrap_or_else(|| "Flow Creative".into());
        let dirty = self
            .last_saved_document
            .as_ref()
            .is_some_and(|d| d != &view.editor.document());
        let save_status = if self.importing {
            "正在复制素材…"
        } else if self.saving {
            "正在保存…"
        } else if dirty {
            "等待保存"
        } else if self.project.is_some() {
            "已保存到本地"
        } else {
            "独立画布"
        };
        let owner = cx.entity().downgrade();
        let mut surface = div()
            .relative()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .child(
                canvas(
                    move |bounds, _, cx| {
                        let _ = owner.update(cx, |this, _| {
                            this.canvas_size =
                                WorldSize::new(bounds.size.width.into(), bounds.size.height.into())
                        });
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .child(self.canvas.clone());
        let tools = [
            ("text", "T", "文字"),
            ("image", "▧", "图片"),
            ("generate_image", "A", "图像"),
            ("generate_video", "▷", "视频"),
            ("generate_audio", "♫", "音频"),
            ("generate_model", "3D", "模型"),
        ];
        let mut rail = div()
            .occlude()
            .absolute()
            .left(px(18.))
            .top(px(72.))
            .w(px(66.))
            .p_2()
            .bg(rgb(t.panel))
            .rounded_xl()
            .shadow_sm()
            .border_1()
            .border_color(rgb(t.border))
            .flex()
            .flex_col()
            .gap_1();
        for (kind, symbol, name) in tools {
            rail = rail
                .child(self.button(
                    SharedString::from(format!("tool-{kind}")),
                    symbol,
                    if kind == "image" && self.project.is_some() {
                        Action::ImportImage
                    } else {
                        Action::Add(kind.into())
                    },
                    !editable || count >= MAX_CARDS || self.saving,
                    cx,
                ))
                .child(
                    div()
                        .text_size(px(9.))
                        .text_color(rgb(t.muted))
                        .text_center()
                        .child(name),
                );
        }
        surface = surface.child(rail);
        if count == 0 {
            surface = surface.child(
                div()
                    .absolute()
                    .left(px((self.canvas_size.width / 2. - 230.).max(100.)))
                    .top(px((self.canvas_size.height / 2. - 110.).max(100.)))
                    .w(px(460.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_4()
                    .child(
                        div()
                            .text_size(px(30.))
                            .text_color(rgb(0xa1a5af))
                            .child("你的创意，从这里开始"),
                    )
                    .child(label("导入参考图、添加文字，或放入一个生成节点"))
                    .child(self.button(
                        "empty-import",
                        "＋ 导入本地图片",
                        Action::ImportImage,
                        self.project.is_none() || self.saving,
                        cx,
                    ))
                    .child(label("所有内容保存在本机 · 不会自动上传或调用模型")),
            );
        }
        if let Some(node) = &node {
            let r = viewport.world_rect_to_screen(node.bounds());
            let toolbar_left = (r.origin.x + r.size.width / 2. - 184.)
                .clamp(90., (self.canvas_size.width - 380.).max(90.));
            let toolbar_top = (r.origin.y - 78.).max(8.);
            let mut toolbar = div()
                .occlude()
                .absolute()
                .left(px(toolbar_left))
                .top(px(toolbar_top))
                .p_1()
                .bg(rgb(t.panel))
                .rounded_lg()
                .shadow_md()
                .border_1()
                .border_color(rgb(t.border))
                .flex()
                .gap_1()
                .child(self.button("object-copy", "复制", Action::Duplicate, !editable, cx))
                .child(self.button("object-frame", "分组", Action::Frame, !editable, cx))
                .child(self.button("object-colors", "端口颜色", Action::Colors, !editable, cx));
            if node.kind == "image" {
                toolbar = toolbar.child(self.button(
                    "object-replace",
                    "替换图片",
                    Action::ReplaceImage,
                    !editable || self.project.is_none() || self.saving,
                    cx,
                ));
            }
            toolbar =
                toolbar.child(self.button("object-delete", "删除", Action::Delete, !editable, cx));
            surface = surface.child(toolbar);
            let generation = GenerationKind::from_node_kind(&node.kind);
            let composer_position = anchored_panel(
                r,
                self.canvas_size,
                WorldSize::new(380., if generation.is_some() { 286. } else { 146. }),
            );
            let composer_left = composer_position.x;
            let composer_top = composer_position.y;
            let mut composer = div()
                .occlude()
                .absolute()
                .left(px(composer_left))
                .top(px(composer_top))
                .w(px(380.))
                .p_3()
                .rounded_xl()
                .bg(rgb(t.panel))
                .border_1()
                .border_color(rgb(t.border))
                .shadow_md()
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    Textarea::new(&self.form)
                        .h(px(if generation.is_some() { 88. } else { 66. }))
                        .disabled(!editable)
                        .aria_label(if generation.is_some() {
                            "生成提示词"
                        } else {
                            "节点内容"
                        }),
                );
            if generation.is_some() {
                let mut parameters = div().flex().items_center().gap_1();
                if generation != Some(GenerationKind::Audio)
                    && generation != Some(GenerationKind::Model)
                {
                    for ratio in ["1:1", "16:9", "9:16"] {
                        parameters = parameters.child(self.button(
                            SharedString::from(format!("ratio-{ratio}")),
                            ratio,
                            Action::Setting("aspect_ratio", Value::String(ratio.into())),
                            !editable,
                            cx,
                        ));
                    }
                }
                if generation == Some(GenerationKind::Video)
                    || generation == Some(GenerationKind::Audio)
                {
                    let duration = node.data["settings"]["duration_seconds"]
                        .as_u64()
                        .unwrap_or(5);
                    parameters = parameters.child(self.button(
                        "duration",
                        format!("{}s", duration),
                        Action::Setting(
                            "duration_seconds",
                            serde_json::json!(if duration == 5 { 10 } else { 5 }),
                        ),
                        !editable,
                        cx,
                    ));
                }
                composer = composer
                    .child(parameters)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(label(if self.generation_available {
                                "宿主模型 · 使用已配置参数"
                            } else {
                                "未连接模型 · 仅本地编辑"
                            }))
                            .child(self.button(
                                "generate-selected",
                                "生成 ↗",
                                Action::Generate,
                                !editable || !self.generation_available,
                                cx,
                            )),
                    )
                    .child(label("先保存提示词；接入宿主后才会发送生成请求"));
            }
            composer = composer.child(
                div()
                    .flex()
                    .gap_2()
                    .child(self.button("apply-data", "保存内容", Action::Apply, !editable, cx))
                    .child(self.button("cancel-data", "还原", Action::Cancel, false, cx))
                    .child(label(format!("{} 个类型端口", node.ports.len()))),
            );
            surface = surface.child(composer);
            if self.show_colors {
                let mut colors = div()
                    .occlude()
                    .absolute()
                    .right(px(18.))
                    .top(px(16.))
                    .w(px(268.))
                    .p_4()
                    .bg(rgb(t.panel))
                    .rounded_xl()
                    .shadow_md()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(div().text_sm().child("按类型统一端口颜色"))
                    .child(label("同一类型的输入、输出和连线一起更新"))
                    .child(
                        Textarea::new(&self.custom_color)
                            .h(px(38.))
                            .aria_label("自定义端口十六进制颜色"),
                    );
                for (index, port) in node.ports.iter().enumerate() {
                    let data_type = port.data_type.clone();
                    let current = gpui_flow::graph_port_color(
                        self.canvas.read(cx).editor.graph(),
                        &data_type,
                    );
                    let mut row = div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(div().w(px(62.)).text_xs().child(port.label.clone()));
                    for (j, color) in [0x4785e7, 0xa274e7, 0xea9062, 0x47ad96, 0xd172a7, 0xe5a534]
                        .into_iter()
                        .enumerate()
                    {
                        let data_type = data_type.clone();
                        row = row.child(
                            div()
                                .id(SharedString::from(format!("port-color-{index}-{j}")))
                                .size(px(20.))
                                .rounded_full()
                                .bg(rgb(color))
                                .border_2()
                                .border_color(rgb(if current == color {
                                    t.foreground
                                } else {
                                    t.panel
                                }))
                                .cursor_pointer()
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.action(
                                        Action::PortColor(data_type.clone(), color),
                                        window,
                                        cx,
                                    )
                                })),
                        );
                    }
                    colors = colors.child(row).child(self.button(
                        SharedString::from(format!("custom-port-{index}")),
                        "使用自定义色",
                        Action::CustomPortColor(data_type.clone()),
                        !editable,
                        cx,
                    ));
                }
                colors =
                    colors.child(self.button("close-colors", "完成", Action::Colors, false, cx));
                surface = surface.child(colors);
            }
        }
        if node.is_none()
            && selection.nodes.len() > 1
            && let Some(bounds) = self
                .canvas
                .read(cx)
                .editor
                .graph()
                .node_bounds(selection.nodes.iter())
        {
            let r = viewport.world_rect_to_screen(bounds);
            surface = surface.child(
                div()
                    .absolute()
                    .left(px((r.center().x - 150.).max(90.)))
                    .top(px((r.origin.y - 58.).max(8.)))
                    .p_1()
                    .rounded_lg()
                    .bg(rgb(t.panel))
                    .shadow_md()
                    .flex()
                    .gap_1()
                    .child(label(format!("{} 个对象", selection.nodes.len())))
                    .child(self.button("multi-frame", "分组", Action::Frame, !editable, cx))
                    .child(self.button("multi-copy", "复制", Action::Duplicate, !editable, cx))
                    .child(self.button("multi-delete", "删除", Action::Delete, !editable, cx)),
            );
        }
        if self.show_import_path {
            surface = surface.child(
                div()
                    .occlude()
                    .absolute()
                    .right(px(18.))
                    .top(px(16.))
                    .w(px(380.))
                    .p_4()
                    .rounded_xl()
                    .bg(rgb(t.panel))
                    .shadow_md()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(div().text_sm().child("导入已选择的本地图片"))
                    .child(label("文件选择器不可用时，可粘贴图片的绝对路径"))
                    .child(
                        Textarea::new(&self.local_path)
                            .h(px(72.))
                            .aria_label("要导入的本地图片绝对路径"),
                    )
                    .child(self.button(
                        "import-local-path",
                        "复制到项目并导入",
                        Action::ImportImagePath,
                        self.importing || self.saving,
                        cx,
                    ))
                    .child(self.button(
                        "close-import-path",
                        "取消",
                        Action::CloseImportPath,
                        false,
                        cx,
                    )),
            );
        }
        if self.show_data {
            surface = surface.child(
                div()
                    .occlude()
                    .absolute()
                    .right(px(18.))
                    .top(px(16.))
                    .w(px(390.))
                    .p_4()
                    .rounded_xl()
                    .bg(rgb(t.panel))
                    .shadow_md()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(div().text_lg().child("数据与自定义节点"))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(self.button(
                                "export-doc",
                                "导出 JSON",
                                Action::Export,
                                false,
                                cx,
                            ))
                            .child(self.button(
                                "records-template",
                                "records 模板",
                                Action::Records,
                                false,
                                cx,
                            )),
                    )
                    .child(
                        Textarea::new(&self.json)
                            .h(px(320.))
                            .aria_label("创作画布 JSON"),
                    )
                    .child(self.button("import-json", "校验并导入", Action::Import, !editable, cx))
                    .child(self.button(
                        "add-palette",
                        "＋ 自定义色板",
                        Action::Add("palette".into()),
                        !editable,
                        cx,
                    ))
                    .child(self.button(
                        "ungroup",
                        "解除分组",
                        Action::Ungroup,
                        selection.groups.is_empty() || !editable,
                        cx,
                    ))
                    .child(self.button(
                        "readonly",
                        if editable {
                            "切为只读"
                        } else {
                            "恢复编辑"
                        },
                        Action::ReadOnly,
                        self.importing,
                        cx,
                    ))
                    .when(self.project.is_none(), |panel| {
                        panel.child(self.button(
                            "open-document",
                            "打开 FlowDocument",
                            Action::Open,
                            !editable,
                            cx,
                        ))
                    })
                    .child(self.button("close-data", "关闭", Action::Data, false, cx)),
            );
        }
        surface = surface
            .child(
                div()
                    .absolute()
                    .left(px(18.))
                    .bottom(px(18.))
                    .flex()
                    .items_center()
                    .gap_2()
                    .p_1()
                    .bg(rgb(t.panel))
                    .rounded_lg()
                    .shadow_sm()
                    .child(self.button("zoom-out", "−", Action::Zoom(1. / 1.2), false, cx))
                    .child(label(format!("{:.0}%", zoom * 100.)))
                    .child(self.button("zoom-in", "＋", Action::Zoom(1.2), false, cx))
                    .child(self.button("fit", "适配", Action::Fit, false, cx)),
            )
            .child(
                div()
                    .absolute()
                    .right(px(22.))
                    .bottom(px(24.))
                    .child(label("Space 拖动画布 · Shift 多选 · 同色端口连接")),
            );
        if let Some(notice) = &self.notice {
            surface = surface.child(
                div()
                    .absolute()
                    .top(px(12.))
                    .left(px(100.))
                    .px_3()
                    .py_2()
                    .rounded_lg()
                    .bg(rgb(t.panel))
                    .text_size(px(11.))
                    .text_color(rgb(t.muted))
                    .child(notice.clone()),
            );
        }
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(t.background))
            .text_color(rgb(t.foreground))
            .child(
                div()
                    .h(px(58.))
                    .px_4()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_between()
                    .border_b_1()
                    .border_color(rgb(t.border))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(self.button(
                                "home",
                                "‹ 项目",
                                Action::Home,
                                self.project.is_none() || self.importing,
                                cx,
                            ))
                            .child(div().text_sm().child(project_name))
                            .child(label(save_status)),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(self.button("undo", "撤销", Action::Undo, !editable, cx))
                            .child(self.button("redo", "重做", Action::Redo, !editable, cx))
                            .child(self.button(
                                "save",
                                "保存",
                                Action::Save,
                                self.saving || self.importing,
                                cx,
                            ))
                            .child(self.button("data-toggle", "数据", Action::Data, false, cx)),
                    ),
            )
            .child(surface)
    }
}

fn anchored_panel(node: gpui_flow_core::Rect, canvas: WorldSize, panel: WorldSize) -> WorldPoint {
    let right = node.origin.x + node.size.width;
    let bottom = node.origin.y + node.size.height;
    let centered = (node.center().x - panel.width / 2.)
        .clamp(90., (canvas.width - panel.width - 12.).max(90.));
    if bottom + 12. + panel.height <= canvas.height - 12. {
        return WorldPoint::new(centered, (bottom + 12.).max(8.));
    }
    if right + 12. + panel.width <= canvas.width - 12. {
        return WorldPoint::new(
            right + 12.,
            node.origin
                .y
                .clamp(8., (canvas.height - panel.height - 12.).max(8.)),
        );
    }
    if node.origin.x - panel.width - 12. >= 90. {
        return WorldPoint::new(
            node.origin.x - panel.width - 12.,
            node.origin
                .y
                .clamp(8., (canvas.height - panel.height - 12.).max(8.)),
        );
    }
    WorldPoint::new(
        centered,
        (node.origin.y - panel.height - 32.)
            .clamp(8., (canvas.height - panel.height - 12.).max(8.)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;
    #[test]
    fn graph_switching_actions_flush_the_pending_form_first() {
        for action in [
            Action::Add("text".into()),
            Action::Duplicate,
            Action::Setting("aspect_ratio", serde_json::json!("16:9")),
            Action::ImportImage,
            Action::ReplaceImage,
            Action::ImportImagePath,
            Action::ReadOnly,
            Action::Home,
            Action::Save,
        ] {
            assert!(action_flushes_form(&action));
        }
        assert!(!action_flushes_form(&Action::Cancel));
        assert!(!action_flushes_form(&Action::Undo));
    }
    #[test]
    fn pending_draft_is_preserved_before_a_parameter_change_but_not_over_undo() {
        let mut node = standard_registry()
            .make_node("generate_image", "draft", WorldPoint::ZERO, None)
            .unwrap();
        let original = form_identity(&node);
        node.data = form_data_update(&node, &original, "the final words before clicking").unwrap();
        node.data["settings"]["aspect_ratio"] = serde_json::json!("16:9");
        assert_eq!(node.data["prompt"], "the final words before clicking");
        assert!(form_data_update(&node, &original, "stale text").is_none());
    }
    #[test]
    fn composer_tracks_transformed_node_and_avoids_media_near_bottom() {
        let canvas = WorldSize::new(1400., 842.);
        let panel = WorldSize::new(380., 286.);
        let upper =
            gpui_flow_core::Rect::new(WorldPoint::new(450., 100.), WorldSize::new(320., 280.));
        assert_eq!(anchored_panel(upper, canvas, panel).y, 392.);
        let lower =
            gpui_flow_core::Rect::new(WorldPoint::new(450., 500.), WorldSize::new(320., 280.));
        let p = anchored_panel(lower, canvas, panel);
        assert!(p.x >= lower.origin.x + lower.size.width + 12.);
        assert!(p.y + panel.height <= canvas.height);
    }
    #[test]
    fn form_identity_includes_kind_across_import_and_undo() {
        let data =
            serde_json::json!({"text":"brief","caption":"image caption","asset":"botanical"});
        let a = Node::new("same", "A", WorldPoint::ZERO)
            .with_kind("text")
            .with_data(data.clone());
        let b = Node::new("same", "A", WorldPoint::ZERO)
            .with_kind("image")
            .with_data(data);
        assert_ne!(form_identity(&a), form_identity(&b));
        assert_eq!(primary_field(&a), Some("text"));
        assert_eq!(primary_field(&b), Some("caption"));
    }
    #[test]
    fn custom_schema_reusing_standard_kind_falls_back_to_full_json() {
        for data in [
            serde_json::json!(42),
            serde_json::json!([1, 2]),
            serde_json::json!("custom"),
            serde_json::json!({"text":42}),
        ] {
            let n = Node::new("custom", "Custom", WorldPoint::ZERO)
                .with_kind("text")
                .with_data(data);
            assert_eq!(primary_field(&n), None);
        }
    }
}
