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
use gpui_kit::component::input::{InputEvent, InputState, Textarea, TextareaState};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

type AdjustmentSlot = Arc<Mutex<Option<image_adjust_controls::AdjustmentPresentation>>>;

type TextEditCallback = Arc<dyn Fn(NodeId, &mut Window, &mut App) + Send + Sync>;

/// Data contains asset keys only; the host explicitly authorizes local sources.
#[derive(Clone, Default)]
pub struct CreativeAssets {
    paths: BTreeMap<String, PathBuf>,
    dimensions: BTreeMap<String, (u32, u32)>,
    media_types: BTreeMap<String, String>,
    models: Arc<model_view::ModelViews>,
    shapes: Arc<shape_preview::ShapeViews>,
    masks: Arc<mask_preview::MaskViews>,
    processing_errors: Arc<Mutex<BTreeMap<NodeId, String>>>,
    custom_views: Arc<Mutex<BTreeMap<NodeId, custom_tasks::CustomPreview>>>,
    custom_kinds: Vec<String>,
    media: Arc<media_view::MediaViews>,
    editing_text: Arc<Mutex<Option<NodeId>>>,
    editing_image: Arc<Mutex<Option<NodeId>>>,
    adjustment_presentation: AdjustmentSlot,
    font_families: Arc<Vec<String>>,
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
        matches!(node.kind.as_str(), "text" | "shape" | "pencil" | "image")
    }
    fn object_caption(&self, node: &Node) -> Option<SharedString> {
        if matches!(node.kind.as_str(), "text" | "shape" | "pencil") {
            return Some("".into());
        }
        let key = node.data["asset"].as_str().map(str::to_string).or_else(|| {
            self.assets
                .custom_views
                .lock()
                .unwrap()
                .get(&node.id)
                .and_then(|p| p.value.as_ref())
                .and_then(|v| v.asset_key())
                .map(str::to_string)
        });
        if let Some((width, height)) = key.as_ref().and_then(|key| self.assets.dimensions.get(key))
            && *width > 0
            && *height > 0
        {
            Some(format!("{}   {} × {}", node.title, width, height).into())
        } else {
            Some(node.title.clone().into())
        }
    }
    fn unframed_body(&self, node: &Node) -> bool {
        matches!(node.kind.as_str(), "shape" | "pencil")
    }
    fn body_hit_test(&self, node: &Node, local: WorldPoint, zoom: f32) -> Option<bool> {
        match node.kind.as_str() {
            "shape" => Some(shape_preview::hit_shape(node, local, 5. / zoom.max(0.1))),
            "pencil" => Some(pencil_preview::hit(node, local, 5. / zoom.max(0.1))),
            _ => None,
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
        if node.kind == crate::pencil::PENCIL_NODE_KIND {
            return self.assets.shapes.render_pencil(node, z, cx);
        }
        if node.kind == crate::shapes::SHAPE_NODE_KIND {
            return self.assets.shapes.render(node, z, cx);
        }
        if node.kind == "image" {
            let preview = self
                .assets
                .adjustment_presentation
                .lock()
                .unwrap()
                .as_ref()
                .filter(|p| {
                    p.node == node.id
                        && p.data == node.data
                        && node.data["asset"]
                            .as_str()
                            .and_then(|key| self.assets.paths.get(key))
                            .is_some_and(|path| path == &p.path)
                })
                .map(|p| p.preview.clone());
            if let Some(preview) = preview {
                preview.update(cx, |v, cx| {
                    v.set_size(
                        WorldSize::new(
                            (node.size.width * z - 2.).max(1.),
                            (node.size.height * z - 2.).max(1.),
                        ),
                        cx,
                    )
                });
                return preview.into_any_element();
            }
        }
        if self.assets.editing_image.lock().unwrap().as_ref() == Some(&node.id) {
            return div().size_full().bg(rgb(0xf5f6f8)).into_any_element();
        }
        if node.kind.starts_with("custom.") {
            let preview = self
                .assets
                .custom_views
                .lock()
                .unwrap()
                .get(&node.id)
                .cloned();
            if let Some(value) = preview.as_ref().and_then(|p| p.value.as_ref()) {
                if let crate::custom_nodes::CustomValue::Image { asset_key } = value
                    && let Some(path) = self.assets.paths.get(asset_key)
                {
                    return img(path.clone())
                        .size_full()
                        .object_fit(ObjectFit::Contain)
                        .into_any_element();
                }
                if let crate::custom_nodes::CustomValue::Text { text } = value {
                    return div()
                        .size_full()
                        .p_4()
                        .bg(rgb(0xffffff))
                        .text_size(px(14. * z))
                        .child(text.chars().take(200).collect::<String>())
                        .into_any_element();
                }
            }
            return div()
                .size_full()
                .p_3()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_2()
                .bg(rgb(0xf0f1f5))
                .text_size(px(12. * z))
                .text_color(rgb(0x777f8b))
                .child(node.title.clone())
                .child(
                    preview
                        .map(|p| p.status)
                        .unwrap_or_else(|| "自定义类型端口".into()),
                )
                .into_any_element();
        }
        if node.kind == "mask" {
            return self.assets.masks.render(node, cx);
        }
        if let Some(error) = self
            .assets
            .processing_errors
            .lock()
            .unwrap()
            .get(&node.id)
            .cloned()
        {
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .p_3()
                .bg(rgb(0xeef0f5))
                .text_size(px(12. * z))
                .text_color(rgb(0x8d939e))
                .child(error)
                .into_any_element();
        }
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
            // Resolve the image against NodeFrame's fixed body viewport before
            // Cover runs. A normal block leaf otherwise expands to the source
            // aspect ratio in locked Taffy, leaving the parent to crop its top.
            return img(path.clone())
                .absolute()
                .top_0()
                .left_0()
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
            let appearance =
                crate::text_appearance::TextAppearance::from_data(&node.data).unwrap_or_default();
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
                .font_weight(text_objects::weight(&node.data))
                .text_align(text_objects::alignment(style.alignment))
                .when_some(
                    appearance.installed_family(&self.assets.font_families),
                    |v, family| v.font_family(SharedString::from(family.to_string())),
                )
                .line_height(relative(1.25))
                .text_color(rgb(appearance.color))
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
            "generate_image" | "edit_image" => ("image", 0xeaf0f8),
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
        "shape",
        "pencil",
        "text",
        "image",
        "video",
        "audio",
        "model",
        "mask",
        "edit_image",
        "process_image",
        "process_video",
        "process_audio",
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
    for kind in &assets.custom_kinds {
        registry.register(
            kind.clone(),
            CardRenderer {
                assets: assets.clone(),
            },
        );
    }
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
    Context(context_menu::ContextAction),
    CanvasAppearance(canvas_tools::CanvasAppearanceAction),
    TextAppearance(text_appearance_tools::TypographyAction),
    Arrange(Option<crate::arrangement::Arrangement>),
    Shape(shape_tools::ShapeAction),
    Pencil(pencil_tools::PencilAction),
    ExportAsset(asset_export_ui::ExportAction),
    Layer(layers::LayerAction),
    FocusSelection,
    ImageEdit(image_edit_controls::ImageEditAction),
    ImageVersions(image_version_panel::VersionAction),
    NodeName(node_name::NameAction),
    ImageRotation(image_rotation_controls::RotationAction),
    ImageAdjust(image_adjust_controls::AdjustAction),
    ImageSizing(image_sizing_controls::ImageSizingAction),
    NewCustom,
    CustomDesigner(custom_designer::CustomDesignerAction),
    Custom(custom_tasks::CustomAction),
    Processing(processing_panel::ProcessingAction),
    ProcessSelection(crate::processing::ProcessingKind),
    ProcessingMenu,
    Mask(mask_editor::MaskAction),
    Home,
    RenameProject,
    ConfirmRename,
    ImportImage,
    ImportAt(WorldPoint),
    ImportModel,
    BrowseAssets,
    CloseAssets,
    AssetsPage(bool),
    FilterAssets(asset_filter::AssetFilter),
    PlaceAsset(String),
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
    TextPlacement(text_placement_tools::TextPlacementAction),
    Undo,
    Redo,
    Delete,
    Frame,
    FrameEdit(frame_tools::FrameAction),
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
    import_replacement: Option<NodeId>,
    import_at: Option<WorldPoint>,
    quick_edit: bool,
    show_layers: bool,
    layer_query: Entity<InputState>,
    show_arrangement: bool,
    typography: Option<text_appearance_tools::TypographyPanel>,
    font_families: Arc<Vec<String>>,
    background_panel: Option<canvas_tools::BackgroundPanel>,
    object_menu: Option<context_menu::ObjectMenu>,
    object_menu_focus: FocusHandle,
    owned_canvas_copy: Option<clipboard_images::OwnedCanvasCopy>,
    clipboard_cancel: Option<Arc<std::sync::atomic::AtomicBool>>,
    import_model: bool,
    asset_picker: Option<asset_browser::AssetPicker>,
    asset_picker_focus: FocusHandle,
    asset_thumbnails: Entity<asset_thumbnails::AssetThumbnails>,
    model_views: Arc<model_view::ModelViews>,
    mask_views: Arc<mask_preview::MaskViews>,
    mask_editing: Option<mask_editor::MaskEditing>,
    mask_focus: FocusHandle,
    mask_cancel: Option<Arc<std::sync::atomic::AtomicBool>>,
    processing_job: Option<processing_tasks::ProcessingJob>,
    processing_errors: Arc<Mutex<BTreeMap<NodeId, String>>>,
    show_processing_menu: bool,
    custom_designer: Option<custom_designer::CustomDesigner>,
    custom_executors: crate::custom_nodes::CustomNodeExecutors,
    custom_job: Option<custom_tasks::CustomJob>,
    custom_views: Arc<Mutex<BTreeMap<NodeId, custom_tasks::CustomPreview>>>,
    known_custom_kinds: Vec<String>,
    media_views: Arc<media_view::MediaViews>,
    editing_text: Arc<Mutex<Option<NodeId>>>,
    text_placing: bool,
    text_placement_drag: Option<text_placement_tools::TextDrag>,
    text_placement_focus: FocusHandle,
    temporary_hand: Option<tool_navigation::SuspendedTool>,
    editing_image: Arc<Mutex<Option<NodeId>>>,
    image_editing: Option<image_edit_controls::ImageEditing>,
    image_versions: Option<image_version_panel::VersionPanel>,
    node_name: Option<node_name::NamePanel>,
    image_rotation: Option<image_rotation_controls::RotationEditing>,
    image_sizing: Option<image_sizing_controls::ImageSizingFields>,
    image_adjustment: Option<image_adjust_controls::AdjustmentEditing>,
    adjustment_presentation: AdjustmentSlot,
    image_edit_focus: FocusHandle,
    image_edit_cancel: Option<Arc<std::sync::atomic::AtomicBool>>,
    shape_views: Arc<shape_preview::ShapeViews>,
    shape_fields: Option<shape_tools::ShapeFields>,
    shape_tool: Option<crate::shapes::ShapeKind>,
    shape_drag: Option<shape_tools::ShapeDrag>,
    show_shapes: bool,
    shape_focus: FocusHandle,
    pencil_focus: FocusHandle,
    pencil_active: bool,
    pencil_draft: Option<pencil_tools::PencilDraft>,
    pencil_fields: Option<pencil_tools::PencilFields>,
    pencil_style: crate::pencil::PencilStyle,
    frame_fields: Option<frame_tools::FrameFields>,
    frame_drawing: bool,
    frame_drag: Option<frame_tools::FrameDrag>,
    frame_focus: FocusHandle,
    export_dialog: Option<asset_export_ui::AssetExportDialog>,
    export_focus: FocusHandle,
    export_serial: u64,
    export_cancel: Option<Arc<std::sync::atomic::AtomicBool>>,
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
        let canvas_preferences =
            crate::canvas_preferences::read_canvas_preferences(&document.graph).unwrap_or_default();
        let guard = registry.clone();
        let validator = registry.clone();
        let menu_owner = cx.entity().downgrade();
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
            view.theme = FlowTheme {
                background: canvas_preferences.background_rgb,
                ..theme()
            };
            view.options = FlowOptions::canvas_only();
            view.options.show_controls = false;
            view.options.show_minimap = canvas_preferences.show_minimap;
            view.show_grid = false;
            view.set_selection_on_top(false);
            view.set_port_labels(false);
            view.set_context_menu_handler(Some(Arc::new(move |request, window, cx| {
                let _ =
                    menu_owner.update(cx, |this, cx| this.open_object_menu(request, window, cx));
            })));
            view.install_plugin(
                crate::image_sizing::CreativeImageResizePlugin::default(),
                cx,
            )
            .expect("unique Creative image resize plugin");
            view.install_plugin(CreativeGuard(guard), cx)
                .expect("unique guard id");
            view
        });
        let subscription = cx.subscribe_in(&canvas, window, |this, canvas, event, window, cx| {
            match event {
                FlowEvent::DocumentChanged | FlowEvent::SelectionChanged => {
                    this.discard_export_return();
                    this.reconcile_object_menu(window, cx);
                    if matches!(event, FlowEvent::DocumentChanged) {
                        this.sync_canvas_appearance(window, cx);
                    }
                    if matches!(event, FlowEvent::SelectionChanged) {
                        this.show_arrangement = false;
                    }
                    this.model_views.retain(canvas.read(cx).editor.graph());
                    this.mask_views
                        .sync(canvas.read(cx).editor.graph(), this.project.as_ref());
                    this.reconcile_mask_edit(cx);
                    this.reconcile_image_edit(cx);
                    this.reconcile_image_versions(cx);
                    this.reconcile_node_name(window, cx);
                    this.reconcile_image_rotation(cx);
                    this.reconcile_image_adjustment(cx);
                    this.reconcile_image_sizing(window, cx);
                    this.reconcile_typography(window, cx);
                    this.reconcile_asset_export(cx);
                    this.sync_frame_fields(window, cx);
                    this.sync_shape_fields(window, cx);
                    this.sync_pencil_fields(window, cx);
                    this.shape_views.retain(canvas.read(cx).editor.graph());
                    this.sync_processing_views(cx);
                    this.sync_custom_views(cx);
                    let kinds = canvas
                        .read(cx)
                        .editor
                        .graph()
                        .nodes
                        .values()
                        .filter(|n| n.kind.starts_with("custom."))
                        .map(|n| n.kind.clone())
                        .collect::<std::collections::BTreeSet<_>>()
                        .into_iter()
                        .collect::<Vec<_>>();
                    if kinds != this.known_custom_kinds {
                        this.refresh_assets(cx);
                    }
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
                FlowEvent::ViewportChanged => {
                    if this.text_placement_drag.is_some() {
                        this.clear_text_placement();
                        this.canvas.read(cx).focus_handle(cx).focus(window, cx);
                    }
                    if this.object_menu.take().is_some() {
                        this.canvas.read(cx).focus_handle(cx).focus(window, cx);
                    }
                    this.queue_autosave(cx);
                }
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
        let layer_query = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("搜索对象或画框名称…")
                .context_menu(false)
        });
        let layer_query_subscription = cx.subscribe(&layer_query, |_, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        });
        let pencil_focus = cx.focus_handle();
        let pencil_blur = cx.on_focus_out(&pencil_focus, window, |this, _, _, cx| {
            if this.pencil_draft.take().is_some() {
                cx.notify();
            }
        });
        let pencil_activation = cx.observe_window_activation(window, |this, window, cx| {
            if !window.is_window_active() && this.pencil_active {
                this.clear_pencil_tool();
                this.sync_pencil_fields(window, cx);
                cx.notify();
            }
        });
        let shape_focus = cx.focus_handle();
        let shape_blur = cx.on_focus_out(&shape_focus, window, |this, _, _, cx| {
            if this.shape_tool.is_some() || this.shape_drag.is_some() {
                this.clear_shape_tool();
                cx.notify();
            }
        });
        let shape_activation = cx.observe_window_activation(window, |this, window, cx| {
            if !window.is_window_active()
                && (this.shape_tool.is_some() || this.shape_drag.is_some())
            {
                this.clear_shape_tool();
                cx.notify();
            }
        });
        let frame_focus = cx.focus_handle();
        let text_placement_focus = cx.focus_handle();
        let text_placement_blur =
            cx.on_focus_out(&text_placement_focus, window, |this, _, _, cx| {
                if this.text_placing {
                    this.clear_text_placement();
                    cx.notify();
                }
            });
        let text_placement_activation = cx.observe_window_activation(window, |this, window, cx| {
            if !window.is_window_active() && this.text_placing {
                this.clear_text_placement();
                cx.notify();
            }
        });
        let frame_focus_out = cx.on_focus_out(&frame_focus, window, |this, _, _, cx| {
            if this.frame_drawing || this.frame_drag.is_some() {
                this.clear_frame_tool();
                cx.notify();
            }
        });
        let frame_activation = cx.observe_window_activation(window, |this, window, cx| {
            if !window.is_window_active() && (this.frame_drawing || this.frame_drag.is_some()) {
                this.clear_frame_tool();
                cx.notify();
            }
        });
        let temporary_hand_activation = cx.observe_window_activation(window, |this, window, cx| {
            if !window.is_window_active() && this.temporary_hand.is_some() {
                this.finish_temporary_hand(false, window, cx);
            }
        });
        let asset_thumbnails = cx.new(asset_thumbnails::AssetThumbnails::new);
        let thumbnail_observer = cx.observe(&asset_thumbnails, |_, _, cx| cx.notify());
        cx.on_release(|this: &mut Self, _| {
            this.cancel_clipboard_paste();
            if let Some(cancel) = &this.export_cancel {
                cancel.store(true, std::sync::atomic::Ordering::Release);
            }
            if let Some(cancel) = &this.image_edit_cancel {
                cancel.store(true, std::sync::atomic::Ordering::Release);
            }
            if let Some(job) = &this.custom_job {
                job.cancel.store(true, std::sync::atomic::Ordering::Release);
            }
            if let Some(job) = &this.processing_job {
                job.cancel.store(true, std::sync::atomic::Ordering::Release);
            }
            if let Some(cancel) = &this.mask_cancel {
                cancel.store(true, std::sync::atomic::Ordering::Release);
            }
        })
        .detach();
        let export_theme = cx.observe_global::<gpui_kit::component::Theme>(|this, cx| {
            this.reconcile_asset_export(cx);
            cx.notify();
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
            import_replacement: None,
            import_at: None,
            quick_edit: false,
            show_layers: false,
            layer_query,
            show_arrangement: false,
            typography: None,
            background_panel: None,
            object_menu: None,
            object_menu_focus: cx.focus_handle(),
            font_families: Arc::new(
                cx.text_system()
                    .all_font_names()
                    .into_iter()
                    .filter(|name| {
                        crate::text_appearance::TextAppearance {
                            font_family: Some(name.clone()),
                            ..Default::default()
                        }
                        .validate()
                        .is_ok()
                    })
                    .collect(),
            ),
            owned_canvas_copy: None,
            clipboard_cancel: None,
            import_model: false,
            asset_picker: None,
            asset_picker_focus: cx.focus_handle(),
            asset_thumbnails,
            model_views: Arc::default(),
            mask_views: Arc::default(),
            mask_editing: None,
            mask_focus: cx.focus_handle(),
            mask_cancel: None,
            processing_job: None,
            processing_errors: Arc::default(),
            show_processing_menu: false,
            custom_designer: None,
            custom_executors: crate::custom_host::local_custom_executors(),
            custom_job: None,
            custom_views: Arc::default(),
            known_custom_kinds: Vec::new(),
            media_views: Arc::default(),
            editing_text: Arc::default(),
            text_placing: false,
            text_placement_drag: None,
            text_placement_focus,
            temporary_hand: None,
            editing_image: Arc::default(),
            image_editing: None,
            image_versions: None,
            node_name: None,
            image_rotation: None,
            image_sizing: None,
            image_adjustment: None,
            adjustment_presentation: Arc::default(),
            image_edit_focus: cx.focus_handle(),
            image_edit_cancel: None,
            shape_views: Arc::default(),
            shape_fields: None,
            shape_tool: None,
            shape_drag: None,
            show_shapes: false,
            shape_focus,
            pencil_focus,
            pencil_active: false,
            pencil_draft: None,
            pencil_fields: None,
            pencil_style: Default::default(),
            frame_fields: None,
            frame_drawing: false,
            frame_drag: None,
            frame_focus,
            export_dialog: None,
            export_focus: cx.focus_handle(),
            export_serial: 0,
            export_cancel: None,
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
            _subscriptions: vec![
                subscription,
                observer,
                form_subscription,
                layer_query_subscription,
                thumbnail_observer,
                pencil_blur,
                pencil_activation,
                shape_blur,
                shape_activation,
                frame_focus_out,
                frame_activation,
                text_placement_blur,
                text_placement_activation,
                temporary_hand_activation,
                export_theme,
            ],
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
            .filter(|node| gpui_flow_core::node_is_visible(v.editor.graph(), &node.id))
            .cloned()
    }
    fn sync_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let node = self.selected(cx);
        if node.as_ref().is_some_and(|n| {
            crate::layer_guard::node_is_locked(self.canvas.read(cx).editor.graph(), &n.id)
        }) {
            self.clear_text_edit();
        }
        let source = node.as_ref().map(form_identity);
        if source == self.form_source {
            return;
        }
        if source.as_ref().map(|s| &s.0) != self.form_source.as_ref().map(|s| &s.0) {
            self.clear_text_edit();
            if self
                .mask_editing
                .as_ref()
                .is_some_and(|edit| source.as_ref().map(|s| &s.0) != Some(&edit.node))
            {
                self.clear_mask_edit();
            }
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
        if self
            .layer_query
            .read(cx)
            .focus_handle(cx)
            .is_focused(window)
        {
            self.canvas.read(cx).focus_handle(cx).focus(window, cx);
        }
        if !matches!(&action, Action::ExportAsset(_)) {
            self.discard_export_return();
        }
        if self.temporary_hand.is_some() {
            self.finish_temporary_hand(false, window, cx);
        }
        if !matches!(&action, Action::TextPlacement(_)) && self.text_placing {
            self.clear_text_placement();
            self.canvas.read(cx).focus_handle(cx).focus(window, cx);
        }
        if self.object_menu.take().is_some() {
            self.canvas.read(cx).focus_handle(cx).focus(window, cx);
        }
        if !matches!(&action, Action::CanvasAppearance(_)) && self.background_panel.is_some() {
            self.background_panel = None;
            self.canvas.read(cx).focus_handle(cx).focus(window, cx);
        }
        if !matches!(&action, Action::TextAppearance(_)) && self.typography.is_some() {
            self.typography = None;
            self.canvas.read(cx).focus_handle(cx).focus(window, cx);
        }
        if !matches!(&action, Action::ImageVersions(_)) {
            self.image_versions = None;
        }
        if !matches!(&action, Action::NodeName(_)) {
            self.close_node_name(window, cx);
        }
        if !matches!(&action, Action::Arrange(_)) {
            self.show_arrangement = false;
        }
        if action_flushes_form(&action) {
            self.apply_form_edit(cx);
        }
        if self.asset_picker.is_some()
            && matches!(
                &action,
                Action::Layers
                    | Action::ImportImage
                    | Action::ImportAt(_)
                    | Action::ImportModel
                    | Action::Add(_)
            )
        {
            // Toolbar buttons preserve focus. Return it before removing the
            // picker so cancelled imports and the next canvas shortcut work.
            self.asset_picker = None;
            self.canvas.read(cx).focus_handle(cx).focus(window, cx);
        }
        if !matches!(&action, Action::FrameEdit(_))
            && (self.frame_drawing || self.frame_drag.is_some())
        {
            self.clear_frame_tool();
            self.canvas.read(cx).focus_handle(cx).focus(window, cx);
        }
        if !matches!(&action, Action::Shape(_))
            && (self.shape_tool.is_some() || self.shape_drag.is_some() || self.show_shapes)
        {
            self.clear_shape_tool();
            self.canvas.read(cx).focus_handle(cx).focus(window, cx);
        }
        if !matches!(&action, Action::Pencil(_)) && self.pencil_active {
            self.clear_pencil_tool();
            self.sync_pencil_fields(window, cx);
            self.canvas.read(cx).focus_handle(cx).focus(window, cx);
        }
        if !matches!(&action, Action::ImageRotation(_)) && self.image_rotation.is_some() {
            self.clear_image_rotation();
            self.canvas.read(cx).focus_handle(cx).focus(window, cx);
        }
        if !matches!(&action, Action::ImageAdjust(_)) && self.image_adjustment.is_some() {
            self.clear_image_adjustment(cx);
            self.canvas.read(cx).focus_handle(cx).focus(window, cx);
        }
        if !matches!(
            &action,
            Action::ImageSizing(_)
                | Action::OriginalRatio
                | Action::Zoom(_)
                | Action::Fit
                | Action::PanTool(_)
        ) {
            self.close_image_sizing(window, cx);
        }
        let command = match action {
            Action::NodeName(action) => {
                self.handle_node_name(action, window, cx);
                return;
            }
            Action::FocusSelection => {
                self.focus_selected(window, cx);
                return;
            }
            Action::ImageVersions(action) => {
                self.handle_image_versions(action, window, cx);
                return;
            }
            Action::ImageSizing(action) => {
                self.handle_image_sizing(action, window, cx);
                return;
            }
            Action::TextPlacement(action) => {
                self.handle_text_placement(action, window, cx);
                return;
            }
            Action::Context(action) => {
                self.context_action(action, window, cx);
                return;
            }
            Action::CanvasAppearance(action) => {
                self.canvas_appearance_action(action, window, cx);
                return;
            }
            Action::TextAppearance(action) => {
                self.handle_typography(action, window, cx);
                return;
            }
            Action::Arrange(arrangement) => {
                self.handle_arrangement(arrangement, window, cx);
                return;
            }
            Action::ImageAdjust(action) => {
                self.handle_image_adjustment(action, window, cx);
                return;
            }
            Action::ImageRotation(action) => {
                self.handle_image_rotation(action, window, cx);
                return;
            }
            Action::Pencil(action) => {
                self.handle_pencil_action(action, window, cx);
                return;
            }
            Action::Shape(action) => {
                self.handle_shape_action(action, window, cx);
                return;
            }

            Action::FrameEdit(action) => {
                self.handle_frame_action(action, window, cx);
                return;
            }
            Action::ExportAsset(action) => {
                self.handle_asset_export(action, window, cx);
                return;
            }
            Action::Layer(action) => {
                self.layer_action(action, cx);
                return;
            }
            Action::ImageEdit(action) => {
                self.handle_image_edit(action, window, cx);
                return;
            }
            Action::NewCustom => {
                let graph = self.canvas.read(cx).editor.graph();
                let definitions =
                    crate::custom_nodes::custom_definitions_from_graph(graph).unwrap_or_default();
                let mut serial = 1;
                let kind = loop {
                    let candidate = format!("custom.node{serial}");
                    if !definitions.iter().any(|d| d.kind == candidate) {
                        break candidate;
                    }
                    serial += 1;
                };
                self.show_processing_menu = false;
                self.open_custom_designer(kind, window, cx);
                return;
            }
            Action::CustomDesigner(action) => {
                self.handle_custom_designer(action, window, cx);
                return;
            }
            Action::Custom(action) => {
                self.custom_action(action, cx);
                return;
            }
            Action::Processing(action) => {
                self.processing_action(action, cx);
                return;
            }
            Action::ProcessSelection(kind) => {
                self.create_processing_from_selection(kind, cx);
                return;
            }
            Action::ProcessingMenu => {
                self.show_processing_menu = !self.show_processing_menu;
                cx.notify();
                return;
            }
            Action::Mask(action) => {
                self.handle_mask_action(action, window, cx);
                return;
            }
            Action::BrowseAssets => {
                self.open_asset_picker(window, cx);
                return;
            }
            Action::CloseAssets => {
                self.asset_picker = None;
                self.canvas.read(cx).focus_handle(cx).focus(window, cx);
                cx.notify();
                return;
            }
            Action::AssetsPage(forward) => {
                if let Some(picker) = &mut self.asset_picker {
                    picker.state.page = if forward {
                        picker.state.page.saturating_add(1)
                    } else {
                        picker.state.page.saturating_sub(1)
                    };
                }
                cx.notify();
                return;
            }
            Action::FilterAssets(kind) => {
                if let Some(picker) = &mut self.asset_picker {
                    picker.state.kind = kind;
                    picker.state.page = 0;
                }
                cx.notify();
                return;
            }
            Action::PlaceAsset(key) => {
                self.place_project_asset(&key, cx);
                if self.asset_picker.is_none() {
                    self.canvas.read(cx).focus_handle(cx).focus(window, cx);
                }
                return;
            }
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
                if let Some(node) = self.selected(cx).filter(|n| n.kind == "text") {
                    let bold = crate::text_weight::read_weight(&node.data).unwrap_or(400) >= 600;
                    self.set_text_weight(if bold { 400 } else { 700 }, window, cx);
                }
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
                if self.show_layers {
                    self.layer_query.read(cx).focus_handle(cx).focus(window, cx);
                } else {
                    self.canvas.read(cx).focus_handle(cx).focus(window, cx);
                }
                cx.notify();
                return;
            }
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
                let size = match crate::image_sizing::size_from_height(
                    WorldSize::new(node.size.width, 1.),
                    node.size.width * asset.height as f32 / asset.width as f32,
                    false,
                ) {
                    Ok(size) => size,
                    Err(error) => {
                        self.notice = Some(error);
                        cx.notify();
                        return;
                    }
                };
                CanvasCommand::ResizeNode {
                    node: node.id,
                    size,
                }
            }
            Action::Home => {
                self.cancel_clipboard_paste();
                self.clear_asset_export();
                self.clear_image_edit();
                self.finish_mask_stroke(cx);
                self.clear_mask_edit();
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
                self.import_replacement = None;
                self.import_at = None;
                self.canvas.read(cx).focus_handle(cx).focus(window, cx);
                cx.notify();
                return;
            }
            Action::ImportImage => {
                self.import_model = false;
                self.import_image(false, cx);
                return;
            }
            Action::ImportAt(at) => {
                self.import_model = false;
                self.import_image_at(false, Some(at), cx);
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
                padding: 140.,
            },
            Action::Zoom(factor) => CanvasCommand::Zoom {
                position: WorldPoint::new(
                    self.canvas_size.width / 2.,
                    self.canvas_size.height / 2.,
                ),
                zoom: (self.canvas.read(cx).editor.viewport.zoom * factor).clamp(0.1, 3.),
            },
            Action::Add(kind) => {
                self.show_processing_menu = false;
                let v = self.canvas.read(cx);
                let id = v
                    .editor
                    .graph()
                    .fresh_id(&format!("creative-{}", self.next_id));
                self.next_id += 1;
                let node_size = self
                    .registry
                    .definition(&kind)
                    .map_or(WorldSize::new(320., 240.), |d| d.size);
                let node_height = node_size.height * v.editor.viewport.zoom;
                let composer_height = if GenerationKind::from_node_kind(&kind).is_some() {
                    170.
                } else {
                    104.
                };
                let at = v.editor.viewport.screen_to_world(WorldPoint::new(
                    (self.canvas_size.width - node_size.width * v.editor.viewport.zoom) / 2.,
                    ((self.canvas_size.height - node_height - composer_height - 24.) / 2.).max(80.),
                ));
                match self.registry.make_node(&kind, id, at, None) {
                    Ok(node) => {
                        self.add_object_in_free_space(node, cx);
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
    if node.locked || source != &form_identity(node) {
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
        "generate_image" | "generate_video" | "generate_audio" | "generate_model"
        | "edit_image" => "prompt",
        _ => return None,
    };
    node.data.get(field).and_then(Value::as_str).map(|_| field)
}

fn label(text: impl Into<SharedString>) -> Div {
    let text: SharedString = text.into();
    div().text_xs().text_color(rgb(theme().muted)).child(text)
}
mod arrangement_tools;
mod asset_browser;
mod asset_export_ui;
mod clipboard_images;
mod image_sizing_controls;
mod text_export;
pub use text_export::install_creative_text_export_backend;
mod asset_thumbnails;
mod canvas_tools;
mod chrome;
mod context_menu;
mod custom_designer;
mod custom_tasks;
mod frame_tools;
mod image_adjust_controls;
mod image_adjust_preview;
mod image_adjust_tasks;
mod image_edit_controls;
mod image_edit_preview;
mod image_edit_tasks;
mod image_rotation_controls;
mod image_rotation_preview;
mod image_rotation_tasks;
mod image_version_panel;
mod mask_editor;
mod mask_preview;
mod mask_tasks;
mod media_view;
mod model_view;
mod node_name;
mod object_creation;
mod pencil_preview;
mod pencil_tools;
mod processing_panel;
mod processing_tasks;
mod project_previews;
mod project_ui;
mod reference_ui;
mod selection_toolbar;
mod shape_preview;
mod shape_tools;
mod text_appearance_tools;
mod text_objects;
mod text_placement_tools;
mod tool_navigation;
mod workbench;
pub use workbench::CreativeWorkbench;

#[derive(Clone, Debug)]
pub enum CreativeEvent {
    BackToProjects,
    GenerationRequested(GenerationRequest),
}
impl EventEmitter<CreativeEvent> for CreativeCanvas {}

impl Render for CreativeCanvas {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // A completed path import removes its panel asynchronously. Return only
        // that now-hidden input's focus; never steal focus from another editor.
        if !self.show_import_path && self.local_path.read(cx).focus_handle(cx).is_focused(window) {
            self.canvas.read(cx).focus_handle(cx).focus(window, cx);
        }
        let view = self.canvas.read(cx);
        let t = FlowTheme {
            background: view.theme.background,
            ..theme()
        };
        let editable = view.is_editable() && !self.importing;
        let selection = view.editor.selection.clone();
        let count = view.editor.graph().nodes.len();
        let frame_count = view.editor.graph().groups.len();
        let zoom = view.editor.viewport.zoom;
        let viewport = view.editor.viewport;
        let node = self.selected(cx).filter(|n| {
            !self.pencil_active
                && (!matches!(n.kind.as_str(), "shape" | "pencil") || self.shape_drag.is_none())
        });
        let project_name = self
            .project
            .as_ref()
            .map(|p| p.metadata().name.clone())
            .unwrap_or_else(|| "Flow Creative".into());
        let dirty = self
            .last_saved_document
            .as_ref()
            .is_some_and(|d| d != &view.editor.document());
        let save_status = if self.export_cancel.is_some() {
            "正在导出素材…"
        } else if self.image_edit_cancel.is_some() {
            "正在应用图片编辑…"
        } else if self.custom_job.is_some() {
            "正在运行宿主接口…"
        } else if self.processing_job.is_some() {
            "正在本地处理…"
        } else if self.mask_cancel.is_some() {
            "正在应用遮罩…"
        } else if self.importing {
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
                                // A window move/resize invalidates the screen-space draft.
                                if this.text_placement_drag.is_some() {
                                    this.clear_text_placement();
                                    this.canvas.read(cx).focus_handle(cx).focus(window, cx);
                                }
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
            // The handle can start and release a drag before the next frame
            // mounts its overlay. Keep the finalizer on the persistent surface.
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, event: &MouseUpEvent, window, cx| {
                    if this.shape_drag.is_some() {
                        cx.stop_propagation();
                        this.finish_shape_drag(event.position, window, cx);
                    }
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, event: &MouseUpEvent, window, cx| {
                    if this.shape_drag.is_some() {
                        cx.stop_propagation();
                        this.finish_shape_drag(event.position, window, cx);
                    }
                }),
            )
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape"
                    && this.show_layers
                    && this
                        .layer_query
                        .read(cx)
                        .focus_handle(cx)
                        .is_focused(window)
                {
                    this.action(Action::Layers, window, cx);
                    cx.stop_propagation();
                    window.prevent_default();
                    return;
                }
                if !this.clipboard_key_down(event, window, cx) {
                    this.tool_key_down(event, window, cx);
                }
            }))
            .capture_key_up(cx.listener(|this, event: &KeyUpEvent, window, cx| {
                if matches!(event.keystroke.key.as_str(), "space" | " ")
                    && this.temporary_hand.is_some()
                {
                    cx.stop_propagation();
                    window.prevent_default();
                    this.finish_temporary_hand(true, window, cx);
                }
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" && this.node_name.is_some() {
                    this.close_node_name(window, cx);
                    cx.stop_propagation();
                    cx.notify();
                    return;
                }
                if event.keystroke.key == "escape" && this.image_versions.is_some() {
                    this.image_versions = None;
                    this.canvas.read(cx).focus_handle(cx).focus(window, cx);
                    cx.stop_propagation();
                    cx.notify();
                    return;
                }
                if event.keystroke.key == "escape" && this.image_sizing.is_some() {
                    this.close_image_sizing(window, cx);
                    this.canvas.read(cx).focus_handle(cx).focus(window, cx);
                    cx.stop_propagation();
                    cx.notify();
                    return;
                }
                if event.keystroke.key == "escape" && this.temporary_hand.is_some() {
                    this.finish_temporary_hand(false, window, cx);
                    cx.stop_propagation();
                    return;
                }
                if event.keystroke.key == "escape" && this.background_panel.is_some() {
                    cx.stop_propagation();
                    this.background_panel = None;
                    this.canvas.read(cx).focus_handle(cx).focus(window, cx);
                    cx.notify();
                    return;
                }
                if event.keystroke.key == "escape" && this.typography.is_some() {
                    cx.stop_propagation();
                    this.typography = None;
                    this.canvas.read(cx).focus_handle(cx).focus(window, cx);
                    cx.notify();
                    return;
                }
                if event.keystroke.key == "escape" && this.show_arrangement {
                    cx.stop_propagation();
                    this.show_arrangement = false;
                    cx.notify();
                    return;
                }
                if event.keystroke.key == "escape" && this.image_adjustment.is_some() {
                    cx.stop_propagation();
                    this.handle_image_adjustment(
                        image_adjust_controls::AdjustAction::Cancel,
                        window,
                        cx,
                    );
                    return;
                }
                if event.keystroke.key == "escape" && this.image_rotation.is_some() {
                    cx.stop_propagation();
                    this.handle_image_rotation(
                        image_rotation_controls::RotationAction::Cancel,
                        window,
                        cx,
                    );
                }
            }))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                let point = window.mouse_position();
                let screen =
                    WorldPoint::new(f32::from(point.x), f32::from(point.y)) - this.canvas_origin;
                let at = this.canvas.read(cx).editor.viewport.screen_to_world(screen);
                this.import_dropped_paths(paths.paths().to_vec(), at, cx);
            }));
        if let Some(overlay) = self.frame_draw_overlay(cx) {
            surface = surface.child(overlay);
        }
        if let Some(overlay) = self.text_placement_overlay(cx) {
            surface = surface.child(overlay);
        }
        if let Some(overlay) = self.shape_draw_overlay(cx) {
            surface = surface.child(overlay);
        }
        if let Some(overlay) = self.pencil_overlay(cx) {
            surface = surface.child(overlay);
        }
        let pan_mode = self.canvas.read(cx).pan_mode() || self.temporary_hand.is_some();
        let mut rail = div()
            .id("creative-tool-rail")
            .occlude()
            .absolute()
            .left(px(18.))
            .top(px(76.))
            .w(px(48.))
            .max_h(px((self.canvas_size.height - 132.).max(44.)))
            .overflow_y_scroll()
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
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
                self.rail_button(
                    "tool-select",
                    "mouse-pointer-2",
                    "选择对象",
                    Action::PanTool(false),
                    false,
                    cx,
                )
                .when(
                    !pan_mode
                        && !self.frame_drawing
                        && self.shape_tool.is_none()
                        && !self.pencil_active
                        && !self.text_placing,
                    |b| b.bg(rgb(0xeef1f6)),
                ),
            )
            .child(
                self.rail_button(
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
            .child(self.rail_button(
                "tool-import",
                "upload",
                "导入图片、视频或音频",
                Action::ImportImage,
                !editable || self.saving || count >= MAX_CARDS,
                cx,
            ))
            .child(self.rail_button(
                "tool-assets",
                "folder-open",
                "本项目素材",
                Action::BrowseAssets,
                self.project.is_none(),
                cx,
            ));
        for (kind, symbol, name) in [
            ("mask", "paintbrush", "绘制遮罩"),
            ("edit_image", "image-plus", "图片编辑 · Image/Mask 输入"),
            ("text", "type", "文字 · T"),
            ("generate_image", "image", "图片生成 · A"),
            ("generate_video", "video", "视频生成 · S"),
            ("generate_audio", "music", "音频生成"),
            ("generate_model", "box", "3D 模型生成"),
        ] {
            rail = rail.child(
                self.rail_button(
                    SharedString::from(format!("tool-{kind}")),
                    symbol,
                    name,
                    if kind == "text" {
                        Action::TextPlacement(text_placement_tools::TextPlacementAction::Toggle)
                    } else {
                        Action::Add(kind.into())
                    },
                    !editable || count >= MAX_CARDS,
                    cx,
                )
                .when(kind == "text" && self.text_placing, |button| {
                    button.bg(rgb(0xeef1f6))
                }),
            );
        }
        rail = rail.child(
            self.rail_button(
                "tool-pencil",
                "pencil",
                "自由绘制 · P",
                Action::Pencil(pencil_tools::PencilAction::Toggle),
                !editable || count >= MAX_CARDS,
                cx,
            )
            .when(self.pencil_active, |b| b.bg(rgb(0xeef1f6))),
        );
        rail = rail.child(
            self.rail_button(
                "tool-shape",
                "circle",
                "绘制图形",
                Action::Shape(shape_tools::ShapeAction::Menu),
                !editable || self.saving || count >= MAX_CARDS,
                cx,
            )
            .when(self.show_shapes || self.shape_tool.is_some(), |b| {
                b.bg(rgb(0xeef1f6))
            }),
        );
        rail = rail
            .child(
                self.rail_button(
                    "tool-frame",
                    "grid-2x2",
                    "画框 · F · 拖动或单击创建",
                    Action::FrameEdit(frame_tools::FrameAction::Draw),
                    !editable || self.saving || frame_count >= MAX_CARDS,
                    cx,
                )
                .when(self.frame_drawing, |b| b.bg(rgb(0xeef1f6))),
            )
            .child(self.rail_button(
                "tool-processing",
                "sliders-horizontal",
                "本地媒体处理",
                Action::ProcessingMenu,
                !editable,
                cx,
            ))
            .child(div().w(px(24.)).h(px(1.)).my_1().bg(rgb(0xe9eaed)))
            .child(self.rail_button("tool-layers", "layers", "图层", Action::Layers, false, cx));
        surface = surface.child(rail);
        if self.show_shapes {
            surface = surface.child(self.shape_menu(cx));
        }
        if self.show_processing_menu {
            let mut menu = div()
                .occlude()
                .absolute()
                .left(px(78.))
                .top(px(390.))
                .w(px(220.))
                .p_3()
                .rounded_xl()
                .bg(rgb(t.panel))
                .shadow_md()
                .flex()
                .flex_col()
                .gap_2()
                .child(label("本地处理节点"));
            for kind in [
                crate::processing::ProcessingKind::Image,
                crate::processing::ProcessingKind::Video,
                crate::processing::ProcessingKind::Audio,
            ] {
                menu = menu.child(self.button(
                    SharedString::from(format!("add-{}", kind.node_kind())),
                    kind.title(),
                    Action::Add(kind.node_kind().into()),
                    !editable || count >= MAX_CARDS,
                    cx,
                ));
            }
            menu = menu.child(self.button(
                "add-custom",
                "＋ 自定义节点",
                Action::NewCustom,
                !editable || count >= MAX_CARDS,
                cx,
            ));
            surface = surface.child(menu);
        }
        if count == 0
            && frame_count == 0
            && !self.frame_drawing
            && self.shape_tool.is_none()
            && !self.pencil_active
            && !self.text_placing
            && self.temporary_hand.is_none()
        {
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
            let editable = editable
                && !crate::layer_guard::node_is_locked(
                    self.canvas.read(cx).editor.graph(),
                    &node.id,
                );
            let r = viewport.world_rect_to_screen(node.bounds());
            if matches!(node.kind.as_str(), "shape" | "pencil")
                && let Some(handle) = self.shape_resize_handle(node, r, editable, cx)
            {
                surface = surface.child(handle);
            }
            if let Some(overlay) = self.image_edit_overlay(node, r, cx) {
                surface = surface.child(overlay);
            }
            if node.kind == "mask"
                && let Some(overlay) = self.mask_editor_overlay(node, r, cx)
            {
                surface = surface.child(overlay);
            }
            if node.kind == "text" && self.is_editing_text(&node.id) {
                surface = surface.child(self.text_editor(node, r, viewport.zoom, cx));
            }
            let export_width = if crate::asset_export::has_media_output(node)
                || matches!(node.kind.as_str(), "shape" | "pencil")
            {
                38.
            } else {
                0.
            };
            let toolbar_width = if self.is_image_editing(&node.id) {
                470.
            } else {
                (match node.kind.as_str() {
                    "image" => 562.,
                    "video" | "audio" | "process_image" | "process_video" | "process_audio" => 222.,
                    "shape" | "pencil" => 146.,
                    "text" => 506.,
                    _ => 184.,
                }) + export_width
            };
            let composer_size = self.composer_size(node, cx);
            let composer_bounds = gpui_flow_core::Rect::new(
                anchored_panel(r, self.canvas_size, composer_size),
                composer_size,
            );
            let toolbar_at = selection_toolbar::toolbar_avoiding_composer(
                r,
                self.canvas_size,
                toolbar_width,
                composer_bounds,
            );
            surface = surface
                .child(
                    self.object_toolbar(node, editable, cx)
                        .absolute()
                        .w(px(toolbar_width))
                        .left(px(toolbar_at.x))
                        .top(px(toolbar_at.y)),
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
            let panel_size = bounded_panel_size(self.canvas_size, WorldSize::new(294., 174.));
            let panel_at = anchored_panel(r, self.canvas_size, panel_size);
            let toolbar_at = if self.show_arrangement {
                selection_toolbar::toolbar_avoiding_composer(
                    r,
                    self.canvas_size,
                    selection_toolbar::SELECTION_TOOLBAR_WIDTH,
                    gpui_flow_core::Rect::new(panel_at, panel_size),
                )
            } else {
                selection_toolbar::toolbar_position(
                    r,
                    self.canvas_size,
                    selection_toolbar::SELECTION_TOOLBAR_WIDTH,
                )
            };
            surface = surface.child(
                self.selection_toolbar(
                    selection.nodes.len(),
                    editable
                        && selection
                            .expanded_nodes(self.canvas.read(cx).editor.graph())
                            .iter()
                            .all(|id| {
                                !crate::layer_guard::node_is_locked(
                                    self.canvas.read(cx).editor.graph(),
                                    id,
                                )
                            }),
                    cx,
                )
                .absolute()
                .left(px(toolbar_at.x))
                .top(px(toolbar_at.y)),
            );
            if self.show_arrangement {
                surface = surface.child(
                    self.arrangement_panel(panel_size, cx)
                        .absolute()
                        .left(px(panel_at.x))
                        .top(px(panel_at.y)),
                );
            }
        }
        if self.pencil_active {
            surface = surface.child(
                div()
                    .absolute()
                    .top(px(76.))
                    .left(px(90.))
                    .w(px(360.))
                    .p_3()
                    .rounded_lg()
                    .bg(rgb(0xffffff))
                    .shadow_sm()
                    .occlude()
                    .child(self.pencil_panel(editable, cx)),
            );
        }
        if let Some(controls) = self.frame_controls(cx) {
            surface = surface.child(controls);
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
                    .child(div().text_sm().child(if self.import_replacement.is_some() {
                        "替换已选择的图片 · 保留节点与连线"
                    } else if self.import_model {
                        "导入已选择的本地 GLB 模型"
                    } else if self.import_at.is_some() {
                        "导入到选定画布位置"
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
                    .px_1()
                    .rounded_lg()
                    .bg(rgb(t.background))
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
                    .h(px(38.))
                    .px_1()
                    .rounded_lg()
                    .bg(rgb(t.background))
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
                    .child(self.icon_button("fit", "focus", "适应画布", Action::Fit, false, cx))
                    .child(chrome::separator())
                    .child(
                        self.icon_button(
                            "toggle-minimap",
                            "map",
                            "显示或隐藏小地图",
                            Action::CanvasAppearance(
                                canvas_tools::CanvasAppearanceAction::ToggleMinimap,
                            ),
                            !editable || self.saving,
                            cx,
                        )
                        .when(self.canvas.read(cx).options.show_minimap, |v| {
                            v.bg(rgb(0xeff4ff))
                        }),
                    )
                    .child(
                        self.icon_button(
                            "canvas-background",
                            "palette",
                            "画布背景",
                            Action::CanvasAppearance(
                                canvas_tools::CanvasAppearanceAction::BackgroundPanel,
                            ),
                            false,
                            cx,
                        )
                        .when(self.background_panel.is_some(), |v| v.bg(rgb(0xeff4ff))),
                    ),
            )
            .when(selection.nodes.is_empty(), |surface| {
                surface.child(
                    div()
                        .absolute()
                        .right(px(22.))
                        .bottom(px(26.))
                        .text_size(px(10.))
                        .text_color(rgb(0xb2b7c0))
                        .child("V 选择 · H / Space 平移 · Shift 多选"),
                )
            });
        if self.background_panel.is_some() {
            surface = surface.child(self.background_panel(editable && !self.saving, cx));
        }
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
        if self.show_layers {
            surface = surface.child(self.layers_panel(cx));
        }
        if self.asset_picker.is_some() {
            surface = surface.child(self.asset_panel(cx));
        }
        if self.custom_designer.is_some() {
            surface = surface.child(self.custom_designer_panel(cx));
        }
        if self.export_dialog.is_some() {
            surface = surface.child(self.asset_export_panel(cx));
        }
        if self.object_menu.is_some() {
            surface = surface.child(self.object_context_menu(cx));
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

fn bounded_panel_size(canvas: WorldSize, preferred: WorldSize) -> WorldSize {
    WorldSize::new(
        preferred.width.min((canvas.width - 24.).max(1.)),
        preferred.height.min((canvas.height - 74.).max(1.)),
    )
}
fn panel_overlap(at: WorldPoint, panel: WorldSize, node: gpui_flow_core::Rect) -> f32 {
    ((at.x + panel.width).min(node.max_x()) - at.x.max(node.min_x())).max(0.)
        * ((at.y + panel.height).min(node.max_y()) - at.y.max(node.min_y())).max(0.)
}
fn anchored_panel(node: gpui_flow_core::Rect, canvas: WorldSize, panel: WorldSize) -> WorldPoint {
    let max_x = (canvas.width - panel.width - 12.).max(8.);
    let min_x = 90_f32.min(max_x);
    let max_y = (canvas.height - panel.height - 12.).max(8.);
    let min_y = 62_f32.min(max_y);
    let clamp = |p: WorldPoint| WorldPoint::new(p.x.clamp(min_x, max_x), p.y.clamp(min_y, max_y));
    let centered = (node.center().x - panel.width / 2.).clamp(min_x, max_x);
    let candidates = [
        WorldPoint::new(centered, node.max_y() + 12.),
        WorldPoint::new(node.max_x() + 12., node.origin.y.clamp(min_y, max_y)),
        WorldPoint::new(
            node.origin.x - panel.width - 12.,
            node.origin.y.clamp(min_y, max_y),
        ),
        WorldPoint::new(centered, node.origin.y - panel.height - 32.),
    ];
    for candidate in candidates {
        if candidate == clamp(candidate) && panel_overlap(candidate, panel, node) == 0. {
            return candidate;
        }
    }
    // With no free adjacent side, use the reachable viewport edge that covers
    // the least of the object, instead of always covering its upper half.
    candidates
        .into_iter()
        .map(clamp)
        .chain([
            WorldPoint::new(min_x, min_y),
            WorldPoint::new(max_x, min_y),
            WorldPoint::new(min_x, max_y),
            WorldPoint::new(max_x, max_y),
        ])
        .min_by(|a, b| panel_overlap(*a, panel, node).total_cmp(&panel_overlap(*b, panel, node)))
        .unwrap_or(WorldPoint::new(min_x, min_y))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;
    #[test]
    fn compact_panels_remain_reachable_and_choose_a_free_side_when_available() {
        use gpui_flow_core::Rect;
        for canvas in [
            WorldSize::new(1180., 812.),
            WorldSize::new(640., 480.),
            WorldSize::new(300., 200.),
        ] {
            let size = bounded_panel_size(canvas, WorldSize::new(286., 246.));
            for node in [
                Rect::new(WorldPoint::new(372., 158.), WorldSize::new(495., 438.)),
                Rect::new(WorldPoint::new(80., 80.), WorldSize::new(400., 300.)),
            ] {
                let at = anchored_panel(node, canvas, size);
                assert!(at.x >= 8. && at.y >= 8.);
                assert!(
                    at.x + size.width <= canvas.width - 12.
                        && at.y + size.height <= canvas.height - 12.
                );
            }
        }
        let node = Rect::new(WorldPoint::new(372., 158.), WorldSize::new(495., 438.));
        let size = WorldSize::new(286., 246.);
        assert_eq!(
            panel_overlap(
                anchored_panel(node, WorldSize::new(1180., 812.), size),
                size,
                node
            ),
            0.
        );
    }
    #[test]
    fn graph_switching_actions_flush_the_pending_form_first() {
        for action in [
            Action::Add("text".into()),
            Action::Duplicate,
            Action::Setting("aspect_ratio", serde_json::json!("16:9")),
            Action::ImportImage,
            Action::ImportAt(WorldPoint::new(100., 100.)),
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

mod focus_selection;
mod layers;
