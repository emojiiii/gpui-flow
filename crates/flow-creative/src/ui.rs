use crate::*;
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, FlowCanvas, FlowEvent, FlowOptions, FlowTheme, NodeRegistry, NodeRenderContext,
    NodeRenderer,
};
use gpui_flow_core::{FlowDocument, Node, NodeId, Point as WorldPoint, Size as WorldSize};
use gpui_flow_interaction::{CanvasCommand, SelectionMode};
use gpui_kit::component::input::{Textarea, TextareaState};
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
    fn render(
        &self,
        node: &Node,
        context: NodeRenderContext,
        _: &mut Window,
        _: &mut App,
    ) -> AnyElement {
        let z = context.zoom;
        let content_height = (node.size.height - 75.).max(25.);
        let mut body = div()
            .p(px(14. * z))
            .flex()
            .flex_col()
            .gap(px(10. * z))
            .text_color(rgb(context.theme.foreground))
            .text_size(px(14. * z));
        if node.kind == "text" {
            body = body
                .child(
                    div()
                        .text_size(px(20. * z))
                        .child(node.data["text"].as_str().unwrap_or("").to_string()),
                )
                .child(
                    div()
                        .text_size(px(10. * z))
                        .text_color(rgb(context.theme.muted))
                        .child("STRING · 文字输出"),
                );
        } else if matches!(node.kind.as_str(), "image" | "compose") {
            let key = node.data["asset"].as_str().unwrap_or("");
            if let Some(path) = self.assets.paths.get(key) {
                body = body.child(
                    img(path.clone())
                        .w_full()
                        .h(px((content_height - 48.) * z))
                        .object_fit(ObjectFit::Contain),
                );
            } else {
                body = body.child(
                    div()
                        .h(px((content_height - 48.) * z))
                        .bg(rgb(0xece9e1))
                        .child(format!("未注册图像：{key}")),
                );
            }
            let field = if node.kind == "image" {
                "caption"
            } else {
                "text"
            };
            body = body.child(
                div()
                    .text_size(px(11. * z))
                    .child(node.data[field].as_str().unwrap_or("").to_string()),
            );
        } else {
            body = body.child(node.data.to_string());
        }
        body.into_any_element()
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
    for kind in ["text", "image", "compose"] {
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
        background: 0xf4f2ec,
        panel: 0xfffefa,
        card: 0xffffff,
        border: 0xdeddd5,
        foreground: 0x303d38,
        muted: 0x7d867e,
        accent: 0x39735c,
        grid: 0xdadfd6,
    }
}
#[derive(Clone)]
enum Action {
    Add(String),
    Select(NodeId),
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
    form_source: Option<(NodeId, String, Value)>,
    show_data: bool,
    json_mode: JsonMode,
    canvas_size: WorldSize,
    next_id: u64,
    notice: Option<String>,
    trace: bool,
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
                    this.sync_form(window, cx);
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
                FlowEvent::ConnectionRejected(reason) => {
                    this.notice = Some(format!("连接被拒绝：{reason}"))
                }
                _ => {}
            }
            cx.notify();
        });
        let observer = cx.observe(&canvas, |_, _, cx| cx.notify());
        Ok(Self {
            canvas,
            registry,
            form: cx.new(|cx| {
                TextareaState::new(window, cx)
                    .placeholder("选择一张卡片")
                    .context_menu(false)
            }),
            json: cx.new(|cx| {
                TextareaState::new(window, cx)
                    .placeholder("导出文档或加载 records 模板")
                    .context_menu(false)
            }),
            form_source: None,
            show_data: false,
            json_mode: JsonMode::Document,
            canvas_size: WorldSize::new(780., 700.),
            next_id: 1,
            notice: Some("拖动端口连接：仅同类型 Output → Input；没有模型调用".into()),
            trace: std::env::args().any(|a| a == "--trace"),
            _subscriptions: vec![subscription, observer],
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
        let command = match action {
            Action::Select(node) => CanvasCommand::SelectNode {
                node,
                mode: SelectionMode::Replace,
            },
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
                let at = v.editor.viewport.screen_to_world(WorldPoint::new(
                    self.canvas_size.width / 2. - 110.,
                    self.canvas_size.height / 2. - 90.,
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
fn form_identity(node: &Node) -> (NodeId, String, Value) {
    (node.id.clone(), node.kind.clone(), node.data.clone())
}
fn primary_field(node: &Node) -> Option<&'static str> {
    let field = match node.kind.as_str() {
        "text" | "compose" => "text",
        "image" => "caption",
        "palette" => "note",
        _ => return None,
    };
    node.data.get(field).and_then(Value::as_str).map(|_| field)
}

fn label(text: impl Into<SharedString>) -> Div {
    let text: SharedString = text.into();
    div().text_xs().text_color(rgb(theme().muted)).child(text)
}
impl Render for CreativeCanvas {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme();
        let v = self.canvas.read(cx);
        let editable = v.is_editable();
        let busy = v.is_file_busy();
        let graph = v.editor.graph();
        let count = graph.nodes.len();
        let links = graph.edges.len();
        let frames = graph.groups.len();
        let zoom = v.editor.viewport.zoom;
        let selection = v.editor.selection.clone();
        let rows: Vec<_> = graph
            .nodes
            .values()
            .map(|n| (n.id.clone(), n.title.clone(), n.kind.clone()))
            .collect();
        let status = self
            .notice
            .clone()
            .unwrap_or_else(|| v.status_message().to_string());
        let node = self.selected(cx);
        let kinds: Vec<_> = self
            .registry
            .definitions()
            .map(|d| (d.kind.clone(), d.title.clone()))
            .collect();
        let mut layers = div()
            .id("creative-layers")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_2();
        for (id, title, kind) in rows {
            let mut row_theme = t;
            if selection.nodes.contains(&id) {
                row_theme.card = 0xe3eee5;
                row_theme.border = t.accent;
            }
            layers = layers
                .child(
                    self.button(
                        SharedString::from(format!("select-{id}")),
                        title,
                        Action::Select(id),
                        false,
                        cx,
                    )
                    .theme(row_theme),
                )
                .child(label(kind));
        }
        let mut inspector = div()
            .w(px(if self.show_data { 370. } else { 270. }))
            .flex_shrink_0()
            .p_4()
            .bg(rgb(t.panel))
            .border_l_1()
            .border_color(rgb(t.border))
            .flex()
            .flex_col()
            .gap_3();
        if self.show_data {
            inspector=inspector.child(div().text_lg().child("数据 ⇄ 画布"))
                .child(label(if self.json_mode==JsonMode::Document{"当前格式：FlowDocument v1"}else{"当前格式：records v1"}))
                .child(div().flex().gap_2().child(self.button("export-doc","导出文档",Action::Export,false,cx)).child(self.button("records-template","records 模板",Action::Records,false,cx)))
                .child(Textarea::new(&self.json).h(px(410.)).aria_label("创作画布 JSON"))
                .child(self.button("import-json","校验并导入",Action::Import,!editable||busy,cx))
                .child(label("不猜测任意JSON语义。非法类型、缺失节点、未知kind、坏数据会整体拒绝，原画布不变。"));
        } else {
            inspector = inspector
                .child(label("PROPERTIES / 卡片属性"))
                .child(
                    div().text_lg().child(
                        node.as_ref()
                            .map(|n| n.title.clone())
                            .unwrap_or_else(|| "选择卡片开始编辑".into()),
                    ),
                )
                .child(label(
                    node.as_ref()
                        .map(|n| format!("{} · {}", n.id, n.kind))
                        .unwrap_or_else(|| format!("{} 个选择", selection.len())),
                ))
                .child(label(
                    node.as_ref()
                        .map(|n| {
                            format!(
                                "位置 {:.0}, {:.0}   尺寸 {:.0} × {:.0}",
                                n.position.x, n.position.y, n.size.width, n.size.height
                            )
                        })
                        .unwrap_or_default(),
                ))
                .child(
                    Textarea::new(&self.form)
                        .h(px(200.))
                        .disabled(node.is_none() || !editable)
                        .aria_label("卡片内容或自定义 JSON 数据"),
                )
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(self.button(
                            "apply-data",
                            "应用内容",
                            Action::Apply,
                            node.is_none() || !editable,
                            cx,
                        ))
                        .child(self.button(
                            "cancel-data",
                            "放弃修改",
                            Action::Cancel,
                            node.is_none(),
                            cx,
                        )),
                )
                .child(label("PORT CONTRACT / 端口契约"));
            if let Some(node) = &node {
                for port in &node.ports {
                    inspector = inspector.child(div().text_sm().child(format!(
                        "{} {} · {:?}",
                        if port.direction == gpui_flow_core::PortDirection::Input {
                            "←"
                        } else {
                            "→"
                        },
                        port.label,
                        port.data_type
                    )));
                }
            }
            inspector=inspector.child(label("仅同型输出→输入。实线是已提交连接，拖动预览不写入文档。"))
                .child(self.button("delete-selection","删除选择",Action::Delete,selection.is_empty()||!editable,cx))
                .child(div().flex_1()).child(label("Frame 为显式成员的视觉分组，不是嵌套裁切容器。图片来自host注册asset，JSON不会自动联网。"));
        }
        let owner = cx.entity().downgrade();
        div().size_full().flex().flex_col().bg(rgb(t.background)).text_color(rgb(t.foreground))
            .child(div().h(px(76.)).flex_shrink_0().px_4().bg(rgb(t.panel)).border_b_1().border_color(rgb(t.border)).flex().items_center().justify_between()
                .child(div().child(div().text_xl().child("Flow Creative")).child(label("灵感、素材与创作方案，在同一块画布相遇")))
                .child(div().flex().gap_2()
                    .child(self.button("save","保存",Action::Save,busy,cx)).child(self.button("open","打开",Action::Open,busy||!editable,cx))
                    .child(self.button("undo","撤销",Action::Undo,!editable,cx)).child(self.button("redo","重做",Action::Redo,!editable,cx))
                    .child(self.button("data-toggle",if self.show_data{"返回属性"}else{"数据 / JSON"},Action::Data,false,cx))))
            .child(div().flex_1().min_h_0().flex()
                .child(div().w(px(174.)).flex_shrink_0().p_3().bg(rgb(t.panel)).border_r_1().border_color(rgb(t.border)).flex().flex_col().gap_2()
                    .child(label("CREATE / 添加卡片"))
                    .children(kinds.into_iter().map(|(kind,title)|self.button(SharedString::from(format!("add-{kind}")),format!("+ {}",title.split(" / ").next().unwrap_or(&title)),Action::Add(kind),!editable||count>=MAX_CARDS,cx)))
                    .child(label("ORGANIZE / Frame"))
                    .child(self.button("create-frame","选择建 Frame",Action::Frame,selection.nodes.is_empty()||!editable,cx))
                    .child(self.button("ungroup","解除 Frame",Action::Ungroup,selection.groups.is_empty()||!editable,cx))
                    .child(label("LAYERS / 图层"))
                    .child(layers)
                    .child(self.button("readonly",if editable{"切为只读"}else{"恢复编辑"},Action::ReadOnly,false,cx)))
                .child(div().flex_1().min_w_0().flex().flex_col()
                    .child(div().h(px(50.)).px_3().flex_shrink_0().flex().items_center().justify_between()
                        .child(label("SHIFT 多选 · SPACE 平移 · 拖端口连线"))
                        .child(div().flex().items_center().gap_2().child(self.button("zoom-out","−",Action::Zoom(1./1.2),false,cx)).child(label(format!("{:.0}%",zoom*100.))).child(self.button("zoom-in","+",Action::Zoom(1.2),false,cx)).child(self.button("fit","适配",Action::Fit,false,cx))))
                    .child(div().relative().flex_1().min_h_0().overflow_hidden()
                        .child(canvas(move|bounds,_,cx|{let _=owner.update(cx,|this,_|this.canvas_size=WorldSize::new(bounds.size.width.into(),bounds.size.height.into()));},|_,_,_,_|{}).absolute().size_full())
                        .child(self.canvas.clone())))
                .child(inspector))
            .child(div().h(px(62.)).flex_shrink_0().px_4().py_2().bg(rgb(t.panel)).border_t_1().border_color(rgb(t.border))
                .child(div().text_sm().child(status))
                .child(label(format!("{count} 张卡片 · {links} 条连接 · {frames} 个 Frame · 本地离线，无生成计费 · Esc取消拖线/手势 · 文档不保存历史"))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;
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
