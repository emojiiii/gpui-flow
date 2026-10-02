//! Opt-in intrinsic Text-font resize, preserving affine linear pose. Optional --trace prints
//! accepted selected-target matrices, never text contents or image bytes.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, FlowTheme, GpuiTextRasterizer, SceneView, SceneViewEvent,
    interaction::{
        SelectionMode,
        scene::{
            SceneCommand,
            plugins::{SceneResizePlugin, TextResizeMode},
        },
    },
    scene::{
        Affine, Color, DisplayObject, Point as ScenePoint, Scene, Shape, Size, Style, display,
    },
};

use std::sync::Arc;
fn text(label: &str) -> display::Text {
    let mut t = display::Text::new(label, Size::new(220., 100.));
    t.font_family = "DejaVu Sans".into();
    t.font_size = 23.;
    t.padding = [8.; 4];
    t.overflow = display::TextOverflow::Ellipsis;
    t.decoration = display::TextDecoration::Underline;
    t
}
fn scene() -> Scene {
    let mut scene = Scene::new();
    let image = format!("{}/assets/checker.png", env!("CARGO_MANIFEST_DIR"));
    scene
        .insert(
            DisplayObject::new("text", text("Text can resize\nand rotate · ffi café"))
                .with_transform(Affine::translation(80., 90.))
                .with_style(Style::fill(Color::hex(0xf4dc94))),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("rect", display::Rect::new(100., 80.))
                .with_transform(Affine::translation(320., 100.))
                .with_style(Style::fill(Color::hex(0x55a7ed))),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("affine-parent", display::Group::new()).with_transform(
                Affine::translation(560., 170.) * Affine::rotation(-12.) * Affine::scale(1.2, 0.8),
            ),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("affine-text", text("Affine parent\nSame glyph backend"))
                .with_transform(Affine::rotation(30.))
                .with_style(Style::fill(Color::hex(0x8bd7ff))),
            Some("affine-parent".into()),
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new(
                "image",
                display::Image::new(image, Size::new(160., 100.)).with_corners([28., 0., 18., 0.]),
            )
            .with_transform(Affine::translation(80., 340.)),
            None,
            None,
        )
        .unwrap();
    scene
}
struct Lab {
    view: Entity<SceneView>,
    _subscription: Subscription,
    bridge: Arc<GpuiTextRasterizer>,
    enabled: bool,
    box_mode: bool,
}
impl Lab {
    fn new(bridge: Arc<GpuiTextRasterizer>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let view = cx.new(|cx| {
            let mut view = SceneView::new(scene(), cx).unwrap();
            view.set_text_editing_enabled(true, window, cx);
            view.set_text_rasterizer(Some(bridge.clone()), cx).unwrap();
            view.replace_plugin(
                SceneResizePlugin::default().with_text_resize_mode(TextResizeMode::FontSize),
                cx,
            )
            .unwrap();
            view.editor.selection.select("text".into(), false);
            view
        });
        let trace = std::env::args().any(|arg| arg == "--trace");
        let subscription = cx.subscribe(&view, move |_, view, event, cx| {
            if trace && matches!(event, SceneViewEvent::Changed) {
                let view = view.read(cx);
                for id in &view.editor.selection.ids {
                    if let Some(node) = view.editor.scene().get(id) {
                        eprintln!("transform: {id} {:?}", node.object.transform);
                        if let Shape::Text(t) = &node.object.shape {
                            eprintln!("box: {id} {:?} font {}", t.size, t.font_size);
                        }
                    }
                }
            }
            cx.notify();
        });
        Self {
            view,
            _subscription: subscription,
            bridge,
            enabled: true,
            box_mode: true,
        }
    }
    fn command(&mut self, command: SceneCommand, window: &mut Window, cx: &mut Context<Self>) {
        self.view.update(cx, |view, cx| {
            view.finish_text_editor(window, cx);
            let _ = view.execute_command(command, cx);
        });
    }
}
impl Render for Lab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = FlowTheme::default();
        let view = self.view.read(cx);
        let status = view
            .editor
            .selection
            .ids
            .iter()
            .next()
            .and_then(|id| {
                view.editor.scene().get(id).map(|node| {
                    let t = node.object.transform;
                    let detail = if let Shape::Text(text) = &node.object.shape {
                        format!(
                            " · box {:.1}×{:.1} · font {:.1}",
                            text.size.width, text.size.height, text.font_size
                        )
                    } else {
                        String::new()
                    };
                    format!(
                        "{id}: [{:.3}, {:.3}, {:.3}, {:.3}] + ({:.2}, {:.2}){}{}",
                        t.a,
                        t.b,
                        t.c,
                        t.d,
                        t.tx,
                        t.ty,
                        if view.is_editable() {
                            ""
                        } else {
                            " · read-only"
                        },
                        detail,
                    )
                })
            })
            .unwrap_or_else(|| "Select objects".into());
        div().size_full().flex().flex_col().bg(rgb(theme.background)).text_color(rgb(theme.foreground))
            .child(div().flex().items_center().gap_2().p_3()
                .children([
                    ("text", "Text", vec!["text"]),
                    ("mixed", "Mixed", vec!["text", "rect"]),
                    ("affine", "Affine Text", vec!["affine-text"]),
                    ("roots", "Shared roots", vec!["rect", "affine-parent", "affine-text"]),
                    ("mixed-image", "Text + Image", vec!["image", "text"]),
                ].into_iter().map(|(id, label, ids)| ControlButton::new(id, label).on_click(cx.listener(move |this, _, window, cx| {
                    for (i, id) in ids.iter().enumerate() {
                        this.command(SceneCommand::Select { id: (*id).into(), mode: if i == 0 { SelectionMode::Replace } else { SelectionMode::Add } }, window, cx);
                    }
                }))))
                .child(ControlButton::new("undo", "Undo").on_click(cx.listener(|this, _, window, cx| this.command(SceneCommand::Undo, window, cx))))
                .child(ControlButton::new("redo", "Redo").on_click(cx.listener(|this, _, window, cx| this.command(SceneCommand::Redo, window, cx))))
                .child(ControlButton::new("readonly", "Read-only").on_click(cx.listener(|this, _, window, cx| {
                    this.view.update(cx, |view, cx| { view.finish_text_editor(window, cx); view.set_editable(!view.is_editable(), cx); });
                })))
                .child(ControlButton::new("zoom", "Zoom").on_click(cx.listener(|this, _, window, cx| {
                    let zoom = if this.view.read(cx).editor.viewport.zoom < 1.25 { 1.5 } else { 1. };
                    this.command(SceneCommand::Zoom { position: ScenePoint::new(350., 220.), zoom }, window, cx);
                })))
                .child(ControlButton::new("box-mode", if self.box_mode { "Font: on" } else { "Font: off" }).on_click(cx.listener(|this, _, window, cx| {
                    this.box_mode = !this.box_mode;
                    this.view.update(cx, |v,cx| { v.finish_text_editor(window,cx); v.replace_plugin(SceneResizePlugin::default().with_text_resize_mode(if this.box_mode { TextResizeMode::FontSize } else { TextResizeMode::Transform }),cx).unwrap(); });
                })))
                .child(ControlButton::new("bridge", "Font bridge").on_click(cx.listener(|this, _, window, cx| { this.enabled = !this.enabled; this.view.update(cx,|v,cx|{v.finish_text_editor(window,cx);v.set_text_rasterizer(this.enabled.then(||this.bridge.clone()),cx).unwrap();});})))
                .child(ControlButton::new("reset", "Reset").on_click(cx.listener(|this, _, window, cx| this.command(SceneCommand::LoadScene(scene()), window, cx)))))
            .child(div().px_3().text_sm().child("Font mode: single Text scales font, padding and fixed box. Top/bottom use Y; other handles use X. Linear pose stays fixed."))
            .child(div().flex_1().min_h_0().child(self.view.clone()))
            .child(div().p_3().text_sm().text_color(rgb(theme.muted)).child(format!("{} selected · zoom {:.2} · {}", view.editor.selection.ids.len(), view.editor.viewport.zoom, status)))
    }
}
fn main() {
    let (app, bridge) =
        GpuiTextRasterizer::application(gpui_kit::platform::current_platform(false));
    app.with_assets(gpui_kit::assets::Assets).run(move |cx| {
        gpui_kit::init(cx);
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1180.), px(740.)),
                    cx,
                ))),
                ..Default::default()
            },
            cx,
            move |window, cx| {
                window.set_window_title("gpui-flow · Text font resize");
                cx.new(|cx| Lab::new(bridge, window, cx))
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
