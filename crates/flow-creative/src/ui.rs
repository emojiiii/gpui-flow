use crate::TextStyle;
use crate::*;
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, FlowCanvas, FlowEvent, FlowOptions, FlowTheme, NodeRegistry, NodeRenderContext,
    NodeRenderer,
};
use gpui_flow_core::{FlowDocument, Node, NodeId, Point as WorldPoint, Size as WorldSize};
use gpui_flow_interaction::{CanvasCommand, SelectionMode};
use gpui_kit::base::Button;
use gpui_kit::component::input::{InputEvent, Textarea, TextareaState};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

type TextEditCallback = Arc<dyn Fn(NodeId, &mut Window, &mut App) + Send + Sync>;

/// Data contains asset keys only; the host explicitly authorizes local sources.
#[derive(Clone, Default)]
pub struct CreativeAssets {
    paths: BTreeMap<String, PathBuf>,
    dimensions: BTreeMap<String, (u32, u32)>,
    media_types: BTreeMap<String, String>,
    models: Arc<model_view::ModelViews>,
    media: Arc<media_view::MediaViews>,
    editing_text: Arc<Mutex<Option<NodeId>>>,
    begin_text_edit: Option<TextEditCallback>,
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
    fn transparent_background(&self, node: &Node) -> bool {
        node.kind == "text"
    }
    fn object_caption(&self, node: &Node) -> Option<SharedString> {
        if node.kind == "text" {
            return Some("".into());
        }
        if let Some((width, height)) = node.data["asset"]
            .as_str()
            .and_then(|key| self.assets.dimensions.get(key))
            && *width > 0
            && *height > 0
        {
            Some(format!("{}   {} × {}", node.title, width, height).into())
        } else {
            Some(node.title.clone().into())
        }
    }
    fn full_bleed(&self) -> bool {
        true
    }
    fn render(
        &self,
        node: &Node,
        context: NodeRenderContext,
        _: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let z = context.zoom;
        let key = node.data["asset"].as_str().unwrap_or("");
        if let Some(path) = self.assets.paths.get(key) {
            if self
                .assets
                .media_types
                .get(key)
                .is_some_and(|kind| kind.starts_with("video/") || kind.starts_with("audio/"))
            {
                return self.assets.media.render(node, path, context.selected, cx);
            }
            if matches!(node.kind.as_str(), "model" | "generate_model") {
                return self.assets.models.render(node, path, cx);
            }
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
            let style = TextStyle::from_data(&node.data).unwrap_or_default();
            let editing = self
                .assets
                .editing_text
                .lock()
                .is_ok_and(|id| id.as_ref() == Some(&node.id));
            let id = node.id.clone();
            return div()
                .id(SharedString::from(format!("text-object-{}", node.id)))
                .size_full()
                .px(px(10. * z))
                .py(px(8. * z))
                .text_size(px(style.font_size * z))
                .font_weight(if style.bold {
                    FontWeight::BOLD
                } else {
                    FontWeight::NORMAL
                })
                .text_align(text_objects::alignment(style.alignment))
                .line_height(relative(1.25))
                .text_color(rgb(0x32343a))
                .when(!editing, |body| {
                    body.child(node.data["text"].as_str().unwrap_or("").to_string())
                })
                .when_some(self.assets.begin_text_edit.clone(), |body, callback| {
                    body.on_mouse_down(MouseButton::Left, move |event, window, cx| {
                        // Consume the second down before the canvas acquires its
                        // drag capture, whose mouse-up handler consumes clicks.
                        if event.click_count == 2 && !event.modifiers.shift {
                            cx.stop_propagation();
                            window.prevent_default();
                            callback(id.clone(), window, cx);
                        }
                    })
                })
                .into_any_element();
        }
        let (symbol, background) = match node.kind.as_str() {
            "generate_image" => ("image", 0xeaf0f8),
            "generate_video" => ("video", 0xeef0f5),
            "generate_audio" => ("music", 0xf3eff8),
            "generate_model" | "model" => ("box", 0xecf2f1),
            _ => ("image", 0xf0f1f4),
        };
        let status = match node.data["status"].as_str() {
            Some("queued") => Some("等待生成"),
            Some("running") => Some("正在生成…"),
            Some("failed") => Some("生成失败"),
            Some("complete") => Some("等待宿主提供素材"),
            _ => None,
        };
        div()
            .size_full()
            .bg(rgb(background))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_3()
            .child(chrome::icon(symbol, 38. * z).text_color(rgb(0xb8c9df)))
            .when_some(status, |body, status| {
                body.child(
                    div()
                        .text_size(px(11. * z))
                        .text_color(rgb(0x8c96a6))
                        .child(status),
                )
            })
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
        "video",
        "audio",
        "model",
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
    RenameProject,
    ConfirmRename,
    ImportImage,
    ImportModel,
    PanTool(bool),
    QuickEdit,
    EditText,
    FinishText,
    Media(media_view::MediaAction),
    TextSize(f32),
    TextBold,
    TextAlign(TextAlignment),
    UseReference,
    OriginalRatio,
    Layers,
    SelectObject(NodeId),
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
    ModelOrbit(f32, f32),
    ModelZoom(f32),
    ModelFit,
    ReadOnly,
    Data,
    Export,
    Records,
    Import,
    Apply,
    Save,
    Open,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum JsonMode {
    Document,
    Records,
}
type FormIdentity = (NodeId, String, Value);
struct DeferredFormEdit {
    source: FormIdentity,
    text: String,
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
    project_name_input: Entity<TextareaState>,
    show_rename: bool,
    pending_project_name: Option<String>,
    show_import_path: bool,
    quick_edit: bool,
    show_layers: bool,
    import_model: bool,
    model_views: Arc<model_view::ModelViews>,
    media_views: Arc<media_view::MediaViews>,
    editing_text: Arc<Mutex<Option<NodeId>>>,
    form_source: Option<(NodeId, String, Value)>,
    deferred_edits: BTreeMap<NodeId, DeferredFormEdit>,
    show_data: bool,
    json_mode: JsonMode,
    canvas_size: WorldSize,
    canvas_origin: WorldPoint,
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
            view.set_port_labels(false);
            view.install_plugin(CreativeGuard(guard), cx)
                .expect("unique guard id");
            view
        });
        let subscription = cx.subscribe_in(&canvas, window, |this, canvas, event, window, cx| {
            match event {
                FlowEvent::DocumentChanged | FlowEvent::SelectionChanged => {
                    this.model_views.retain(canvas.read(cx).editor.graph());
                    this.media_views.retain(canvas, cx);
                    // A successful graph edit supersedes an earlier rejection. Selection
                    // alone must not erase an error the user still needs to understand.
                    if matches!(event, FlowEvent::DocumentChanged) {
                        this.notice = None;
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
                    .placeholder("本地素材绝对路径")
                    .context_menu(false)
            }),
            project_name_input: cx.new(|cx| {
                TextareaState::new(window, cx)
                    .placeholder("项目名称")
                    .context_menu(false)
            }),
            show_rename: false,
            pending_project_name: None,
            show_import_path: false,
            quick_edit: false,
            show_layers: false,
            import_model: false,
            model_views: Arc::default(),
            media_views: Arc::default(),
            editing_text: Arc::default(),
            form_source: None,
            deferred_edits: BTreeMap::new(),
            show_data: false,
            json_mode: JsonMode::Document,
            canvas_size: WorldSize::new(780., 700.),
            canvas_origin: WorldPoint::ZERO,
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
        if source.as_ref().map(|s| &s.0) != self.form_source.as_ref().map(|s| &s.0) {
            self.clear_text_edit();
            self.quick_edit = false;
            self.show_colors = false;
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
        if self.form.read(cx).value().as_ref() != value {
            self.form.update(cx, |s, cx| s.set_value(value, window, cx));
        }
    }
    fn command(&mut self, command: CanvasCommand, cx: &mut Context<Self>) -> bool {
        self.notice = None;
        let result = self
            .canvas
            .update(cx, |v, cx| v.execute_command(command, cx));
        self.notice = Some(match &result {
            Ok(()) => String::new(),
            Err(e) => e.to_string(),
        });
        if result.is_ok() {
            self.notice = None;
        }
        cx.notify();
        result.is_ok()
    }
    fn action(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
        if action_flushes_form(&action) {
            self.apply_form_edit(cx);
        }
        let command = match action {
            Action::Media(action) => {
                if let Some(node) = self.selected(cx) {
                    self.media_views.command(&node.id, action, cx);
                }
                return;
            }
            Action::EditText => {
                if let Some(node) = self.selected(cx).filter(|n| n.kind == "text") {
                    self.begin_text_edit(node.id, window, cx);
                }
                return;
            }
            Action::FinishText => {
                self.clear_text_edit();
                self.canvas.read(cx).focus_handle(cx).focus(window, cx);
                cx.notify();
                return;
            }
            Action::TextSize(delta) => {
                self.edit_text_style(
                    |style| style.font_size = (style.font_size + delta).clamp(12., 144.),
                    window,
                    cx,
                );
                return;
            }
            Action::TextBold => {
                self.edit_text_style(|style| style.bold = !style.bold, window, cx);
                return;
            }
            Action::TextAlign(alignment) => {
                self.edit_text_style(|style| style.alignment = alignment, window, cx);
                self.clear_text_edit();
                self.canvas.read(cx).focus_handle(cx).focus(window, cx);
                return;
            }
            Action::RenameProject => {
                if let Some(project) = &self.project {
                    let name = project.metadata().name.clone();
                    self.project_name_input
                        .update(cx, |input, cx| input.set_value(name, window, cx));
                    self.show_rename = !self.show_rename;
                    cx.notify();
                }
                return;
            }
            Action::ConfirmRename => {
                let name = self.project_name_input.read(cx).value().trim().to_string();
                if name.is_empty() {
                    self.notice = Some("请输入项目名称".into());
                    cx.notify();
                    return;
                }
                self.pending_project_name = Some(name);
                self.show_rename = false;
                self.save_project(cx);
                return;
            }
            Action::PanTool(enabled) => {
                self.canvas
                    .update(cx, |canvas, cx| canvas.set_pan_mode(enabled, cx));
                return;
            }
            Action::QuickEdit => {
                self.quick_edit = !self.quick_edit;
                cx.notify();
                return;
            }
            Action::Layers => {
                self.show_layers = !self.show_layers;
                cx.notify();
                return;
            }
            Action::SelectObject(node) => CanvasCommand::SelectNode {
                node,
                mode: SelectionMode::Replace,
            },
            Action::UseReference => {
                self.use_image_reference(cx);
                return;
            }
            Action::OriginalRatio => {
                let Some(node) = self.selected(cx) else {
                    return;
                };
                let Some(asset) = self.project.as_ref().and_then(|p| {
                    p.assets()
                        .iter()
                        .find(|a| Some(a.key.as_str()) == node.data["asset"].as_str())
                }) else {
                    return;
                };
                if asset.width == 0 || asset.height == 0 {
                    return;
                }
                CanvasCommand::ResizeNode {
                    node: node.id,
                    size: WorldSize::new(
                        node.size.width,
                        node.size.width * asset.height as f32 / asset.width as f32,
                    ),
                }
            }
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
                self.import_model = false;
                self.import_image(false, cx);
                return;
            }
            Action::ImportModel => {
                self.import_model = true;
                self.import_image(false, cx);
                return;
            }
            Action::ReplaceImage => {
                self.import_model = false;
                self.import_image(true, cx);
                return;
            }
            Action::Duplicate => CanvasCommand::Duplicate {
                offset: WorldPoint::new(32., 32.),
            },
            Action::ModelOrbit(yaw, pitch) => {
                self.change_model_camera(yaw, pitch, 1., false, cx);
                return;
            }
            Action::ModelZoom(factor) => {
                self.change_model_camera(0., 0., factor, false, cx);
                return;
            }
            Action::ModelFit => {
                self.change_model_camera(0., 0., 1., true, cx);
                return;
            }
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
                if key == "aspect_ratio"
                    && matches!(node.kind.as_str(), "generate_image" | "generate_video")
                {
                    let long = node.size.width.max(node.size.height).clamp(160., 720.);
                    node.size = match node.data["settings"][key].as_str() {
                        Some("16:9") => WorldSize::new(long, long * 9. / 16.),
                        Some("9:16") => WorldSize::new(long * 9. / 16., long),
                        _ => WorldSize::new(long, long),
                    };
                    let mut graph = self.canvas.read(cx).editor.graph().clone();
                    graph.nodes.insert(node.id.clone(), node);
                    self.command(CanvasCommand::ImportGraph(graph), cx);
                    return;
                }
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
            Action::Undo => {
                self.clear_text_edit();
                self.deferred_edits.clear();
                self.form_serial += 1;
                self.form_source = None;
                CanvasCommand::Undo
            }
            Action::Redo => {
                self.clear_text_edit();
                self.deferred_edits.clear();
                self.form_serial += 1;
                self.form_source = None;
                CanvasCommand::Redo
            }
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
                    170.
                } else {
                    104.
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
    ) -> Button {
        let label: SharedString = label.into();
        Button::new(id)
            .accessibility_label(label.clone())
            .disabled(disabled)
            .h(px(28.))
            .px_2()
            .rounded(px(6.))
            .text_size(px(12.))
            .text_color(rgb(if disabled { 0xaeb1b7 } else { 0x42464f }))
            .bg(rgba(0xffffff00))
            .hover(|s| s.bg(rgb(0xf0f1f3)))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_any_mouse_down(|_, window, _| window.prevent_default())
            .child(label)
            .on_click(
                cx.listener(move |this, _, window, cx| this.action(action.clone(), window, cx)),
            )
    }
}
fn action_flushes_form(action: &Action) -> bool {
    !matches!(action, Action::Apply | Action::Undo | Action::Redo)
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
        "image" | "model" | "video" | "audio" => "caption",
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
mod chrome;
mod media_view;
mod model_view;
mod project_ui;
mod text_objects;
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
            .id("creative-surface")
            .relative()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .child(
                canvas(
                    move |bounds, window, cx| {
                        let _ = owner.update(cx, |this, cx| {
                            let origin =
                                WorldPoint::new(bounds.origin.x.into(), bounds.origin.y.into());
                            let size =
                                WorldSize::new(bounds.size.width.into(), bounds.size.height.into());
                            if this.canvas_origin != origin || this.canvas_size != size {
                                this.canvas_origin = origin;
                                this.canvas_size = size;
                                // Layout runs after this view rendered. Notify on
                                // the next turn instead of waiting for input.
                                cx.on_next_frame(window, |_, _, cx| cx.notify());
                            }
                        });
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .child(self.canvas.clone())
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                let point = window.mouse_position();
                let screen =
                    WorldPoint::new(f32::from(point.x), f32::from(point.y)) - this.canvas_origin;
                let at = this.canvas.read(cx).editor.viewport.screen_to_world(screen);
                this.import_dropped_paths(paths.paths().to_vec(), at, cx);
            }));
        let pan_mode = self.canvas.read(cx).pan_mode();
        let mut rail = div()
            .occlude()
            .absolute()
            .left(px(18.))
            .top(px((self.canvas_size.height / 2. - 192.).max(96.)))
            .w(px(48.))
            .p(px(6.))
            .rounded(px(14.))
            .bg(rgb(t.panel))
            .shadow_sm()
            .border_1()
            .border_color(rgb(0xe9eaed))
            .flex()
            .flex_col()
            .items_center()
            .gap_1()
            .child(
                self.icon_button(
                    "tool-select",
                    "mouse-pointer-2",
                    "选择对象",
                    Action::PanTool(false),
                    false,
                    cx,
                )
                .when(!pan_mode, |b| b.bg(rgb(0xeef1f6))),
            )
            .child(
                self.icon_button(
                    "tool-hand",
                    "hand",
                    "移动画布 · 按住空格临时使用",
                    Action::PanTool(true),
                    false,
                    cx,
                )
                .when(pan_mode, |b| b.bg(rgb(0xeef1f6))),
            )
            .child(div().w(px(24.)).h(px(1.)).my_1().bg(rgb(0xe9eaed)))
            .child(self.icon_button(
                "tool-import",
                "upload",
                "导入图片、视频或音频",
                Action::ImportImage,
                !editable || self.saving || count >= MAX_CARDS,
                cx,
            ));
        for (kind, symbol, name) in [
            ("text", "type", "文字"),
            ("generate_image", "image", "图片生成"),
            ("generate_video", "video", "视频生成"),
            ("generate_audio", "music", "音频生成"),
            ("generate_model", "box", "3D 模型生成"),
        ] {
            rail = rail.child(self.icon_button(
                SharedString::from(format!("tool-{kind}")),
                symbol,
                name,
                Action::Add(kind.into()),
                !editable || count >= MAX_CARDS,
                cx,
            ));
        }
        rail = rail
            .child(div().w(px(24.)).h(px(1.)).my_1().bg(rgb(0xe9eaed)))
            .child(self.icon_button("tool-layers", "layers", "图层", Action::Layers, false, cx));
        surface = surface.child(rail);
        if self.show_layers {
            surface = surface.child(self.layers_panel(cx));
        }
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
                            .text_size(px(22.))
                            .text_color(rgb(0xa1a5af))
                            .child("从一张图片开始"),
                    )
                    .child(label("导入本地素材，或在左侧新建生成节点"))
                    .child(self.button(
                        "empty-import",
                        "＋ 导入本地素材",
                        Action::ImportImage,
                        self.project.is_none() || self.saving,
                        cx,
                    ))
                    .child(label("拖放素材到画布，即可开始创作")),
            );
        }
        if let Some(node) = &node {
            let r = viewport.world_rect_to_screen(node.bounds());
            if node.kind == "text" && self.is_editing_text(&node.id) {
                surface = surface.child(self.text_editor(node, r, viewport.zoom, cx));
            }
            let toolbar_width = match node.kind.as_str() {
                "image" => 334.,
                "text" => 410.,
                _ => 184.,
            };
            let toolbar_left = (r.center().x - toolbar_width / 2.)
                .clamp(78., (self.canvas_size.width - toolbar_width - 12.).max(78.));
            let toolbar_top = (r.origin.y - 68.).max(62.);
            surface = surface
                .child(
                    self.object_toolbar(node, editable, cx)
                        .absolute()
                        .left(px(toolbar_left))
                        .top(px(toolbar_top)),
                )
                .child(self.object_composer(node, r, editable, cx));
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
                    .child(div().text_sm().child(if self.import_model {
                        "导入已选择的本地 GLB 模型"
                    } else {
                        "导入本地图片、视频或音频"
                    }))
                    .child(label("文件选择器不可用时，可粘贴文件的绝对路径"))
                    .child(
                        Textarea::new(&self.local_path)
                            .h(px(72.))
                            .aria_label("要导入的本地素材绝对路径"),
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
                    .child(div().text_sm().child("项目选项"))
                    .child(self.button(
                        "save-project",
                        "保存项目",
                        Action::Save,
                        self.saving || self.importing,
                        cx,
                    ))
                    .child(self.button(
                        "import-model-option",
                        "导入本地 GLB",
                        Action::ImportModel,
                        self.project.is_none() || !editable || self.saving,
                        cx,
                    ))
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
                    .occlude()
                    .absolute()
                    .left(px(18.))
                    .top(px(16.))
                    .h(px(38.))
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(self.icon_button(
                        "home",
                        "arrow-left",
                        "返回项目",
                        Action::Home,
                        self.project.is_none() || self.importing,
                        cx,
                    ))
                    .child(self.button(
                        "project-name",
                        project_name,
                        Action::RenameProject,
                        self.project.is_none() || self.saving || self.importing,
                        cx,
                    ))
                    .child(
                        div()
                            .size(px(5.))
                            .rounded_full()
                            .bg(rgb(if dirty || self.saving {
                                0xe3b771
                            } else {
                                0x92b3a0
                            })),
                    )
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(rgb(0xa6abb4))
                            .child(save_status),
                    ),
            )
            .child(
                div()
                    .occlude()
                    .absolute()
                    .right(px(20.))
                    .top(px(16.))
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(self.icon_button("undo", "undo-2", "撤销", Action::Undo, !editable, cx))
                    .child(self.icon_button("redo", "redo-2", "重做", Action::Redo, !editable, cx))
                    .child(self.icon_button(
                        "data-toggle",
                        "ellipsis",
                        "项目选项与数据",
                        Action::Data,
                        false,
                        cx,
                    )),
            )
            .child(
                div()
                    .occlude()
                    .absolute()
                    .left(px(18.))
                    .bottom(px(18.))
                    .h(px(34.))
                    .px_1()
                    .flex()
                    .items_center()
                    .gap_1()
                    .bg(rgb(t.panel))
                    .rounded_lg()
                    .shadow_sm()
                    .child(self.button("zoom-out", "−", Action::Zoom(1. / 1.2), false, cx))
                    .child(
                        div()
                            .min_w(px(38.))
                            .text_center()
                            .text_size(px(11.))
                            .text_color(rgb(0x7c838f))
                            .child(format!("{:.0}%", zoom * 100.)),
                    )
                    .child(self.button("zoom-in", "+", Action::Zoom(1.2), false, cx))
                    .child(self.icon_button("fit", "focus", "适应画布", Action::Fit, false, cx)),
            )
            .child(
                div()
                    .absolute()
                    .right(px(22.))
                    .bottom(px(26.))
                    .text_size(px(10.))
                    .text_color(rgb(0xb2b7c0))
                    .child("Space 平移 · Shift 多选"),
            );
        if self.show_rename {
            surface = surface.child(
                div()
                    .occlude()
                    .absolute()
                    .top(px(62.))
                    .left(px(58.))
                    .w(px(280.))
                    .p_3()
                    .bg(rgb(0xffffff))
                    .rounded_lg()
                    .shadow_md()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        Textarea::new(&self.project_name_input)
                            .h(px(38.))
                            .aria_label("项目名称"),
                    )
                    .child(self.button(
                        "confirm-project-name",
                        "完成",
                        Action::ConfirmRename,
                        self.saving || self.importing,
                        cx,
                    )),
            );
        }
        if let Some(notice) = &self.notice {
            surface = surface.child(
                div()
                    .occlude()
                    .absolute()
                    .right(px(22.))
                    .bottom(px(66.))
                    .max_w(px(420.))
                    .px_3()
                    .py_2()
                    .rounded_lg()
                    .bg(rgb(0xffffff))
                    .shadow_sm()
                    .text_size(px(11.))
                    .text_color(rgb(0x8b7882))
                    .child(notice.clone()),
            );
        }
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(t.background))
            .text_color(rgb(t.foreground))
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
