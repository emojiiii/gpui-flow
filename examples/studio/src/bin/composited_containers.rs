//! Explicit native/CPU container paint phases using the existing SceneView consumer.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, SceneView,
    scene::{
        Affine, BlendMode, Color, ContainerPaintOrder, DisplayObject, Scene, Shape, Size, Stroke,
        Style, display,
    },
};
fn document() -> Scene {
    let mut scene = Scene::new();
    for (id, label, x, shape) in [
        (
            "box",
            "Native Box: visible overflow",
            40.,
            Shape::Box(
                display::Box::new(240., 230.)
                    .rounded(28.)
                    .with_paint_order(ContainerPaintOrder::StrokeAfterChildren),
            ),
        ),
        (
            "frame",
            "CPU Frame: rounded clip + alpha",
            360.,
            Shape::Frame(
                display::Frame::new(240., 230.)
                    .rounded(48.)
                    .with_paint_order(ContainerPaintOrder::StrokeAfterChildren),
            ),
        ),
    ] {
        scene
            .insert(
                DisplayObject::new(id, shape)
                    .with_transform(Affine::translation(x, 85.))
                    .with_style(Style {
                        fill: Some(Color::hex(0xffcc66).into()),
                        stroke: Some(Stroke::new(Color::hex(0x50dfaf), 14.)),
                        opacity: if id == "box" { 1. } else { 0.5 },
                        blend_mode: if id == "box" {
                            BlendMode::PassThrough
                        } else {
                            BlendMode::Normal
                        },
                        ..Default::default()
                    }),
                None,
                None,
            )
            .unwrap();
        for (suffix, x, y, color) in [("red", -25., 30., 0xff4050), ("blue", 100., 125., 0x4060ff)]
        {
            scene
                .insert(
                    DisplayObject::new(format!("{id}-{suffix}"), display::Rect::new(195., 150.))
                        .with_transform(Affine::translation(x, y))
                        .with_style(Style::fill(Color::hex(color))),
                    Some(id.into()),
                    None,
                )
                .unwrap();
        }
        label_at(&mut scene, id, label, x);
    }
    scene
        .insert(
            DisplayObject::new("blend", display::Group::new())
                .with_transform(Affine::translation(680., 85.))
                .with_style(Style {
                    blend_mode: BlendMode::Normal,
                    ..Default::default()
                }),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("yellow", display::Rect::new(275., 275.))
                .with_style(Style::fill(Color::hex(0xffdd33))),
            Some("blend".into()),
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new(
                "clip",
                display::Frame::new(230., 230.)
                    .rounded(65.)
                    .with_paint_order(ContainerPaintOrder::StrokeAfterChildren),
            )
            .with_transform(Affine::translation(20., 20.))
            .with_style(Style {
                fill: None,
                stroke: Some(Stroke::new(Color::hex(0x50dfaf), 14.)),
                ..Default::default()
            }),
            Some("blend".into()),
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("blend-blue", display::Rect::new(300., 300.))
                .with_transform(Affine::translation(-20., -20.))
                .with_style(Style {
                    fill: Some(Color::hex(0x4060ff).into()),
                    blend_mode: BlendMode::Multiply,
                    ..Default::default()
                }),
            Some("clip".into()),
            None,
        )
        .unwrap();
    label_at(&mut scene, "blend", "Clipping keeps parent backdrop", 680.);
    scene
}
fn label_at(scene: &mut Scene, id: &str, label: &str, x: f32) {
    let mut text = display::Text::new(label, Size::new(310., 40.));
    text.font_size = 17.;
    scene
        .insert(
            DisplayObject::new(format!("{id}-label"), text)
                .with_transform(Affine::translation(x, 30.))
                .with_style(Style::fill(Color::WHITE)),
            None,
            None,
        )
        .unwrap();
}
struct Lab {
    view: Entity<SceneView>,
}
impl Lab {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            view: cx.new(|cx| {
                let mut v = SceneView::new(document(), cx).unwrap();
                v.show_grid = false;
                v.set_editable(false, cx);
                v
            }),
        }
    }
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
        let frame = self
            .view
            .read(cx)
            .editor
            .scene()
            .get(&"frame".into())
            .unwrap();
        let Shape::Frame(frame) = &frame.object.shape else {
            unreachable!()
        };
        let order = match frame.paint_order {
            ContainerPaintOrder::BeforeChildren => "fill → stroke → children (legacy)",
            ContainerPaintOrder::StrokeAfterChildren => "fill → children → stroke",
        };
        div().size_full().flex().flex_col().bg(rgb(0x161b25)).text_color(rgb(0xffffff))
            .child(div().h(px(96.)).px_4().py_2().flex().flex_col().gap_2()
                .child(format!("Container paint phases: {order}"))
                .child(div().flex().items_center().gap_2()
                    .child(ControlButton::new("order", "Swap paint order").on_click(cx.listener(|this, _, _, cx| {
                        this.view.update(cx, |v, cx| {
                            v.editor.transaction(|s| {
                                for id in ["box", "frame", "clip"] {
                                    s.update_object(&id.into(), |o| {
                                        let order = match &mut o.shape {
                                            Shape::Box(b) => &mut b.paint_order,
                                            Shape::Frame(f) => &mut f.paint_order,
                                            _ => unreachable!(),
                                        };
                                        *order = match *order {
                                            ContainerPaintOrder::BeforeChildren => ContainerPaintOrder::StrokeAfterChildren,
                                            ContainerPaintOrder::StrokeAfterChildren => ContainerPaintOrder::BeforeChildren,
                                        };
                                    })?;
                                }
                                Ok(())
                            }).unwrap();
                            cx.notify();
                        });
                        cx.notify();
                    })))
                    .child(ControlButton::new("clip", "Frame clip").on_click(cx.listener(|this, _, _, cx| this.edit("frame", |o| if let Shape::Frame(f) = &mut o.shape { f.clip_children = !f.clip_children; }, cx))))
                    .child(ControlButton::new("native", "Box native / CPU").on_click(cx.listener(|this, _, _, cx| this.edit("box", |o| o.style.opacity = if o.style.opacity == 1. { 0.5 } else { 1. }, cx))))
                    .child(ControlButton::new("opacity", "Frame alpha").on_click(cx.listener(|this, _, _, cx| this.edit("frame", |o| o.style.opacity = if o.style.opacity == 0.5 { 1. } else { 0.5 }, cx))))
                    .child(ControlButton::new("blend", "Multiply / Screen").on_click(cx.listener(|this, _, _, cx| this.edit("blend-blue", |o| o.style.blend_mode = if o.style.blend_mode == BlendMode::Multiply { BlendMode::Screen } else { BlendMode::Multiply }, cx))))
                    .child(ControlButton::new("undo", "Undo").on_click(cx.listener(|this, _, _, cx| {
                        this.view.update(cx, |v, cx| { v.editor.undo(); cx.notify(); });
                        cx.notify();
                    }))))
            )
            .child(div().flex_1().min_h_0().overflow_hidden().child(self.view.clone()))
            .child(div().h(px(70.)).px_4().text_sm().child("Explicit after-fill order uses the same native/CPU scene pipeline. Swap order to reveal children over the inner stroke.\nOutside strokes survive child clips; ancestor clips still apply. Box switches native/CPU, Frame keeps whole alpha. Pan/zoom and Undo."))
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
                        size(px(1060.), px(660.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                |window, cx| {
                    window.set_window_title("gpui-flow · Container paint phases");
                    cx.new(Lab::new)
                },
            )
            .unwrap();
            cx.activate(true);
        });
}
