//! Composed subtree shadows, using the same SceneView worker and Gaussian.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, GpuiTextRasterizer, SceneView,
    scene::{
        Affine, BlendMode, Color, ContainerPaintOrder, DisplayObject, Scene, Shadow, Shape, Size,
        Stroke, StrokeAlignment, Style, display,
    },
};
fn effect() -> Option<Shadow> {
    Some(Shadow::new(35., 35., 5., Color::rgba(255, 65, 120, 220)))
}
fn document() -> Scene {
    let mut s = Scene::new();
    for (id, x, shape) in [
        ("group", 65., Shape::Group(display::Group::new())),
        (
            "box",
            340.,
            Shape::Box(
                display::Box::new(130., 100.)
                    .rounded(20.)
                    .with_paint_order(ContainerPaintOrder::StrokeAfterChildren),
            ),
        ),
        (
            "frame",
            640.,
            Shape::Frame(
                display::Frame::new(130., 100.)
                    .rounded(20.)
                    .with_paint_order(ContainerPaintOrder::StrokeAfterChildren),
            ),
        ),
    ] {
        s.insert(
            DisplayObject::new(id, shape)
                .with_transform(Affine::translation(x, 100.))
                .with_style(Style {
                    fill: None,
                    stroke: (id != "group").then(|| {
                        Stroke::new(Color::hex(0x40c8ff), 8.)
                            .with_alignment(StrokeAlignment::Outside)
                    }),
                    shadow: effect(),
                    ..Default::default()
                }),
            None,
            None,
        )
        .unwrap();
        for (suffix, x, y) in [("a", 15., 15.), ("b", 85., 65.)] {
            s.insert(
                DisplayObject::new(format!("{id}-{suffix}"), display::Ellipse::circle(30.))
                    .with_transform(Affine::translation(x, y))
                    .with_style(Style {
                        fill: Some(Color::rgba(90, 230, 175, 220).into()),
                        shadow: (id == "group")
                            .then(|| Shadow::new(-14., 10., 2., Color::rgba(45, 150, 240, 160))),
                        ..Default::default()
                    }),
                Some(id.into()),
                None,
            )
            .unwrap();
        }
    }
    for (id, x, label) in [
        ("g-label", 45., "Group: child shadows + empty gaps"),
        ("b-label", 340., "Box: visible overflow + stroke"),
        ("f-label", 625., "Frame: rounded child clip"),
    ] {
        let mut t = display::Text::new(label, Size::new(280., 32.));
        t.font_size = 15.;
        s.insert(
            DisplayObject::new(id, t)
                .with_transform(Affine::translation(x, 35.))
                .with_style(Style::fill(Color::WHITE)),
            None,
            None,
        )
        .unwrap();
    }
    s
}
struct Lab {
    view: Entity<SceneView>,
}
impl Lab {
    fn edit(&mut self, id: &str, edit: impl FnOnce(&mut DisplayObject), cx: &mut Context<Self>) {
        self.view.update(cx, |v, cx| {
            v.editor
                .transaction(|s| s.update_object(&id.into(), edit))
                .unwrap();
            cx.notify();
        });
        cx.notify();
    }
}
impl Render for Lab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().flex().flex_col().bg(rgb(0x171c27)).text_color(rgb(0xffffff))
            .child(div().h(px(58.)).px_4().flex().items_center().gap_2().child("Subtree shadows")
                .child(ControlButton::new("shadow","Group shadow").on_click(cx.listener(|this,_,_,cx|this.edit("group",|o|o.style.shadow=if o.style.shadow.is_some(){None}else{effect()},cx))))
                .child(ControlButton::new("blur","Blur 5 / 0").on_click(cx.listener(|this,_,_,cx|this.edit("box",|o|{let s=o.style.shadow.as_mut().unwrap();s.blur=if s.blur==0.{5.}else{0.};},cx))))
                .child(ControlButton::new("opacity","Group alpha").on_click(cx.listener(|this,_,_,cx|this.edit("group",|o|o.style.opacity=if o.style.opacity==1.{0.5}else{1.},cx))))
                .child(ControlButton::new("clip","Frame clip").on_click(cx.listener(|this,_,_,cx|this.edit("frame",|o|if let Shape::Frame(f)=&mut o.shape {f.clip_children = !f.clip_children;},cx))))
                .child(ControlButton::new("phase","Box order").on_click(cx.listener(|this,_,_,cx|this.edit("box",|o|if let Shape::Box(b)=&mut o.shape {b.paint_order=if b.paint_order==ContainerPaintOrder::BeforeChildren {ContainerPaintOrder::StrokeAfterChildren}else{ContainerPaintOrder::BeforeChildren};},cx))))
                .child(ControlButton::new("unsafe","Child blend").on_click(cx.listener(|this,_,_,cx|this.edit("group-a",|o|o.style.blend_mode=if o.style.blend_mode==BlendMode::Multiply {BlendMode::PassThrough}else{BlendMode::Multiply},cx))))
                .child(ControlButton::new("isolate","Group isolation").on_click(cx.listener(|this,_,_,cx|this.edit("group",|o|o.style.blend_mode=if o.style.blend_mode==BlendMode::Normal {BlendMode::PassThrough}else{BlendMode::Normal},cx))))
                .child(ControlButton::new("undo","Undo").on_click(cx.listener(|this,_,_,cx|this.view.update(cx,|v,cx|{v.editor.undo();cx.notify();})))))
            .child(div().flex_1().min_h_0().overflow_hidden().child(self.view.clone()))
            .child(div().h(px(85.)).px_4().text_sm().child("Pink shadows use composed subtree alpha, including blue child shadows; gaps remain transparent.\nChild blend rejects a full-opacity pass-through Group. Group isolation explicitly restores a safe Normal surface.\nClick real shapes to select; shadow-only pixels do not expand hits or selection. Drag, pan/zoom and Undo use the same Scene."))
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
                    size(px(1150.), px(510.)),
                    cx,
                ))),
                ..Default::default()
            },
            move |window, cx| {
                window.set_window_title("gpui-flow · Composed subtree shadows");
                cx.new(|cx| Lab {
                    view: cx.new(|cx| {
                        let mut v = SceneView::new(document(), cx).unwrap();
                        v.set_text_rasterizer(Some(bridge.clone()), cx).unwrap();
                        v
                    }),
                })
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
