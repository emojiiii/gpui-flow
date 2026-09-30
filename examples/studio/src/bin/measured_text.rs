//! Text edits use actual platform fonts and ordinary reactive editor transactions.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, FlowTheme, GpuiTextMeasurer, SceneView,
    layout::*,
    scene::{self, Affine, Color, DisplayObject, Scene, Shape, Size, Style, display},
};
use std::sync::Arc;
struct Lab {
    view: Entity<SceneView>,
    measured: MeasuredFlowLayout,
    status: String,
}
impl Lab {
    fn new(cx: &mut Context<Self>) -> Self {
        let mut scene = Scene::new();
        scene
            .insert(
                DisplayObject::new("frame", display::Frame::new(500., 600.))
                    .with_transform(Affine::translation(40., 40.))
                    .with_style(Style::fill(Color::hex(0x253249))),
                None,
                None,
            )
            .unwrap();
        let mut config = FlowLayout::default();
        config.containers.insert(
            "frame".into(),
            FlowStyle {
                direction: FlowDirection::Column,
                align_items: gpui_flow::layout::AlignItems::Stretch,
                height_sizing: AxisSizing::content(),
                padding: Insets::all(20.),
                gap: 16.,
                ..Default::default()
            },
        );
        for (id, content) in [
            ("title", "Real font metrics / 真实文字测量"),
            (
                "body",
                "English and 中文 share the native GPUI shaper. 👩‍💻🌍\nChange width, weight or size: the text and frame resize in the same edit.\n",
            ),
            ("footer", "This final row follows measured text height."),
        ] {
            let mut text = display::Text::new(content, Size::new(460., 30.));
            text.font_size = 20.;
            scene
                .insert(
                    DisplayObject::new(id, text).with_style(Style::fill(Color::hex(0xe0e8f0))),
                    Some("frame".into()),
                    None,
                )
                .unwrap();
            config.texts.insert(id.into(), TextSizing::AutoHeight);
        }
        let measured =
            config.with_text_measurer(Arc::new(GpuiTextMeasurer::new(cx.text_system().clone())));
        measured.apply_to_scene(&mut scene).unwrap();
        let view = cx.new(|cx| {
            let mut view = SceneView::new(scene, cx).unwrap();
            view.set_editable(false, cx);
            measured.install_reactive(&mut view.editor.engine).unwrap();
            view
        });
        Self {
            view,
            measured,
            status: "Ordinary text/frame edits automatically measure and reflow".into(),
        }
    }
    fn edit(
        &mut self,
        edit: impl FnOnce(&mut Scene) -> Result<(), scene::SceneError>,
        cx: &mut Context<Self>,
    ) {
        let result = self.view.update(cx, |view, cx| {
            let result = view.editor.transaction(edit);
            cx.notify();
            result
        });
        self.status = result.map_or_else(
            |e| e.to_string(),
            |_| "Measured and committed as one Undo step".into(),
        );
        cx.notify();
    }
}
impl Render for Lab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = FlowTheme::default();
        let frame = self
            .view
            .read(cx)
            .editor
            .scene()
            .get(&"frame".into())
            .unwrap()
            .object
            .shape
            .local_bounds()
            .unwrap()
            .size;
        div().size_full().flex().flex_col().bg(rgb(theme.background)).text_color(rgb(theme.foreground))
            .child(div().h(px(65.)).px_4().flex().items_center().gap_2()
                .child(ControlButton::new("width", "Width").on_click(cx.listener(|this, _, _, cx| this.edit(|s| s.update_object(&"frame".into(), |o| { let Shape::Frame(f) = &mut o.shape else { unreachable!() }; f.size.width = if f.size.width > 400. { 320. } else { 700. }; }), cx))))
                .child(ControlButton::new("size", "Font size").on_click(cx.listener(|this, _, _, cx| this.edit(|s| s.update_object(&"body".into(), |o| { let Shape::Text(t) = &mut o.shape else { unreachable!() }; t.font_size = if t.font_size > 22. { 18. } else { 28. }; }), cx))))
                .child(ControlButton::new("weight", "Weight").on_click(cx.listener(|this, _, _, cx| this.edit(|s| s.update_object(&"body".into(), |o| { let Shape::Text(t) = &mut o.shape else { unreachable!() }; t.font_weight = if t.font_weight == 400 { 700 } else { 400 }; }), cx))))
                .child(ControlButton::new("content", "Content / empty").on_click(cx.listener(|this, _, _, cx| this.edit(|s| s.update_object(&"body".into(), |o| { let Shape::Text(t) = &mut o.shape else { unreachable!() }; t.content = if t.content.is_empty() { "中文与 English 👩‍💻\nTrailing newline keeps one empty line.\n".into() } else { String::new() }; }), cx))))
                .child(ControlButton::new("refresh", "Refresh fonts").on_click(cx.listener(|this, _, _, cx| { let result = this.view.update(cx, |view, cx| { let result = this.measured.refresh_text_metrics(&mut view.editor); cx.notify(); result }); this.status = result.map_or_else(|e| e.to_string(), |_| "Native metric cache cleared; scene reflowed".into()); cx.notify(); })))
                .child(ControlButton::new("undo", "Undo").on_click(cx.listener(|this, _, _, cx| { this.view.update(cx, |view, cx| { view.editor.undo(); cx.notify(); }); cx.notify(); })))
                .child(ControlButton::new("redo", "Redo").on_click(cx.listener(|this, _, _, cx| { this.view.update(cx, |view, cx| { view.editor.redo(); cx.notify(); }); cx.notify(); }))))
            .child(div().flex_1().min_h_0().child(self.view.clone()))
            .child(div().h(px(40.)).px_4().child(format!("Frame {:.1} × {:.1} | {}", frame.width, frame.height, self.status)))
    }
}
fn main() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(|cx| {
            gpui_kit::init(cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(1160.), px(780.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                |window, cx| {
                    window.set_window_title("gpui-flow · measured text");
                    cx.new(Lab::new)
                },
            )
            .unwrap();
            cx.activate(true);
        });
}
