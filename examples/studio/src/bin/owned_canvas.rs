//! Explicit, bounded backdrop mixing inside the canvas owned by SceneView.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, GpuiTextRasterizer, SceneCompositing, SceneView,
    scene::{
        Affine, BlendMode, Bounds as SceneBounds, Color, DisplayObject, Point as ScenePoint, Scene,
        Size, Style, display,
    },
};
use std::sync::Arc;
fn document() -> Scene {
    let mut scene = Scene::new();
    for (id, x, y, color, blend) in [
        ("red", 65., 85., 0xf54257, BlendMode::PassThrough),
        ("blue", 195., 145., 0x367aff, BlendMode::Multiply),
        ("green", 340., 80., 0x4eca72, BlendMode::Screen),
    ] {
        scene
            .insert(
                DisplayObject::new(id, display::Rect::new(220., 190.).rounded(24.))
                    .with_transform(Affine::translation(x, y))
                    .with_style(Style {
                        fill: Some(Color::hex(color).into()),
                        blend_mode: blend,
                        ..Default::default()
                    }),
                None,
                None,
            )
            .unwrap();
    }
    let mut text = display::Text::new(
        "Real roots blend with earlier roots, background and grid",
        Size::new(850., 45.),
    );
    text.font_family = "DejaVu Sans".into();
    text.font_size = 22.;
    scene
        .insert(
            DisplayObject::new("title", text)
                .with_transform(Affine::translation(45., 20.))
                .with_style(Style::fill(Color::hex(0x1a2940))),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new(
                "animation",
                display::Image::new(
                    concat!(
                        env!("CARGO_MANIFEST_DIR"),
                        "/assets/composite-animation.gif"
                    ),
                    Size::new(205., 205.),
                ),
            )
            .with_transform(Affine::translation(640., 90.))
            .with_style(Style {
                blend_mode: BlendMode::Multiply,
                ..Default::default()
            }),
            None,
            None,
        )
        .unwrap();
    let mut bad = DisplayObject::new(
        "unsupported",
        display::Custom::new(
            "host-native-widget",
            SceneBounds::new(ScenePoint::ZERO, Size::new(30., 30.)),
            vec![],
        ),
    );
    bad.visible = false;
    scene.insert(bad, None, None).unwrap();
    scene
}
struct Lab {
    view: Entity<SceneView>,
}
impl Lab {
    fn new(bridge: Arc<GpuiTextRasterizer>, cx: &mut Context<Self>) -> Self {
        Self {
            view: cx.new(|cx| {
                let mut view = SceneView::new(document(), cx).unwrap();
                view.compositing_mode = SceneCompositing::OwnedCanvas;
                view.theme.background = 0xf3dd94;
                view.theme.grid = 0x748398;
                view.set_text_rasterizer(Some(bridge), cx).unwrap();
                view
            }),
        }
    }
    fn edit(&mut self, id: &str, edit: impl FnOnce(&mut DisplayObject), cx: &mut Context<Self>) {
        self.view.update(cx, |view, cx| {
            view.editor
                .transaction(|scene| scene.update_object(&id.into(), edit))
                .unwrap();
            cx.notify();
        });
    }
}
impl Render for Lab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().flex().flex_col().bg(rgb(0x161b25)).text_color(rgb(0xffffff))
            .child(div().h(px(54.)).px_3().flex().items_center().gap_2().child("Owned canvas")
                .child(ControlButton::new("mode", "Owned / domains").on_click(cx.listener(|this,_,_,cx| this.view.update(cx, |v,cx| { v.compositing_mode=if v.compositing_mode==SceneCompositing::OwnedCanvas {SceneCompositing::Domains} else {SceneCompositing::OwnedCanvas};cx.notify(); }))))
                .child(ControlButton::new("theme", "Yellow / white").on_click(cx.listener(|this,_,_,cx| this.view.update(cx, |v,cx| {v.theme.background=if v.theme.background==0xf3dd94 {0xffffff} else {0xf3dd94};cx.notify();}))))
                .child(ControlButton::new("grid", "Grid").on_click(cx.listener(|this,_,_,cx| this.view.update(cx, |v,cx| {v.show_grid = !v.show_grid;cx.notify();}))))
                .child(ControlButton::new("blend", "Multiply / Screen").on_click(cx.listener(|this,_,_,cx| this.edit("blue", |o| {o.style.blend_mode=if o.style.blend_mode==BlendMode::Multiply {BlendMode::Screen} else {BlendMode::Multiply};}, cx))))
                .child(ControlButton::new("reject", "Custom reject / restore").on_click(cx.listener(|this,_,_,cx| this.edit("unsupported", |o| o.visible = !o.visible, cx))))
                .child(ControlButton::new("undo", "Undo").on_click(cx.listener(|this,_,_,cx| this.view.update(cx, |v,cx| {v.editor.undo();cx.notify();})))))
            .child(div().flex_1().min_h_0().overflow_hidden().child(self.view.clone()))
            .child(div().h(px(64.)).px_4().text_sm().child("Only the owned SceneView canvas is a backdrop. Host controls are outside.\nDrag, pan, zoom and Undo; overlays stay above the complete canvas. Visible Custom rejects the whole domain. GIF and native-backend text use their existing producers."))
    }
}
fn main() {
    let (app, bridge) =
        GpuiTextRasterizer::application(gpui_kit::platform::current_platform(false));
    app.with_assets(gpui_kit::assets::Assets).run(move |cx| {
        gpui_kit::init(cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1060.), px(630.)),
                    cx,
                ))),
                ..Default::default()
            },
            move |window, cx| {
                window.set_window_title("gpui-flow · Owned canvas backdrop");
                cx.new(|cx| Lab::new(bridge.clone(), cx))
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
