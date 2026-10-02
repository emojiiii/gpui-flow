//! Affine Text uses the same native font bridge and existing image compositor.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, GpuiTextRasterizer, SceneView,
    scene::{
        Affine, Color, DisplayObject, Point as ScenePoint, Scene, Shape, Size as SceneSize, Stroke,
        Style, display,
    },
};
use std::sync::Arc;
fn text() -> display::Text {
    let mut t = display::Text::new(
        "Affine ffi café · 漢字\nA long fixed-box title with actual glyphs and ellipsis.",
        SceneSize::new(280., 130.),
    );
    t.font_family = "DejaVu Sans".into();
    t.font_size = 23.;
    t.italic = true;
    t.padding = [12.; 4];
    t.overflow = display::TextOverflow::Ellipsis;
    t.decoration = display::TextDecoration::Underline;
    t
}
fn document() -> Scene {
    let mut s = Scene::new();
    for (id, x, label) in [
        ("reference", 35., "Identity reference"),
        ("affine", 395., "Affine Text + actual vector bounds"),
    ] {
        let mut t = display::Text::new(label, SceneSize::new(370., 35.));
        t.font_size = 18.;
        s.insert(
            DisplayObject::new(format!("{id}-label"), t)
                .with_transform(Affine::translation(x, 25.))
                .with_style(Style::fill(Color::WHITE)),
            None,
            None,
        )
        .unwrap();
    }
    s.insert(
        DisplayObject::new("parent", display::Box::new(320., 230.).rounded(45.))
            .with_transform(Affine::translation(395., 75.))
            .with_style(Style {
                fill: Some(Color::hex(0x102333).into()),
                stroke: Some(Stroke::new(Color::hex(0x205775), 1.)),
                ..Default::default()
            }),
        None,
        None,
    )
    .unwrap();
    for (id, parent, m) in [
        ("reference", None, Affine::translation(35., 125.)),
        (
            "affine",
            Some("parent".into()),
            Affine::translation(20., 50.),
        ),
    ] {
        s.insert(
            DisplayObject::new(format!("{id}-box"), display::Rect::new(280., 130.))
                .with_transform(m)
                .with_style(Style {
                    fill: None,
                    stroke: Some(Stroke::new(Color::hex(0x5d85a0), 1.)),
                    ..Default::default()
                }),
            parent.clone(),
            None,
        )
        .unwrap();
        s.insert(
            DisplayObject::new(id, text())
                .with_transform(m)
                .with_style(Style::fill(Color::hex(0xf4d493))),
            parent,
            None,
        )
        .unwrap();
    }
    s
}
struct Lab {
    view: Entity<SceneView>,
    bridge: Arc<GpuiTextRasterizer>,
    enabled: bool,
}
impl Lab {
    fn new(bridge: Arc<GpuiTextRasterizer>, cx: &mut Context<Self>) -> Self {
        let view = cx.new(|cx| {
            let mut v = SceneView::new(document(), cx).unwrap();
            v.set_editable(false, cx);
            v.show_grid = false;
            v.set_text_rasterizer(Some(bridge.clone()), cx).unwrap();
            v
        });
        Self {
            view,
            bridge,
            enabled: true,
        }
    }
    fn edit(&mut self, action: u8, cx: &mut Context<Self>) {
        self.view.update(cx, |v, cx| {
            v.editor
                .transaction(|s| {
                    if action == 0 {
                        let pose = s
                            .get(&"affine".into())
                            .unwrap()
                            .object
                            .data
                            .as_u64()
                            .unwrap_or(0);
                        let next = (pose + 1) % 6;
                        let linear = match next {
                            1 => Affine::rotation(30.),
                            2 => Affine::rotation(90.),
                            3 => Affine::scale(-1., 1.),
                            4 => Affine::scale(1.3, 0.6),
                            5 => Affine::skew(20., 10.),
                            _ => Affine::IDENTITY,
                        };
                        for id in ["affine", "affine-box"] {
                            s.update_object(&id.into(), |o| {
                                o.transform = Affine::translation(20., 50.)
                                    * linear.around(ScenePoint::new(140., 65.));
                                o.data = next.into();
                            })?;
                        }
                    } else if action == 4 {
                        s.update_object(&"parent".into(), |o| {
                            if let Shape::Box(b) = &mut o.shape {
                                b.clip_children = !b.clip_children;
                            }
                        })?;
                    } else {
                        for id in ["reference", "affine"] {
                            s.update_object(&id.into(), |o| {
                                if let Shape::Text(t) = &mut o.shape {
                                    match action {
                                        1 => t.wrap = !t.wrap,
                                        2 => {
                                            t.decoration =
                                                if t.decoration == display::TextDecoration::None {
                                                    display::TextDecoration::UnderlineStrikethrough
                                                } else {
                                                    display::TextDecoration::None
                                                }
                                        }
                                        3 => {
                                            t.overflow =
                                                if t.overflow == display::TextOverflow::Clip {
                                                    display::TextOverflow::Ellipsis
                                                } else {
                                                    display::TextOverflow::Clip
                                                }
                                        }
                                        _ => {
                                            o.style.opacity =
                                                if o.style.opacity == 1. { 0.5 } else { 1. }
                                        }
                                    }
                                }
                            })?;
                        }
                    }
                    Ok(())
                })
                .unwrap();
            cx.notify();
        });
    }
    fn backend(&mut self, cx: &mut Context<Self>) {
        self.enabled = !self.enabled;
        self.view.update(cx, |v, cx| {
            v.set_text_rasterizer(self.enabled.then(|| self.bridge.clone()), cx)
                .unwrap()
        });
    }
}
impl Render for Lab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().flex().flex_col().bg(rgb(0x0d1421)).text_color(rgb(0xe6edf5))
.child(div().h(px(56.)).px_4().flex().items_center().gap_3()
.child(ControlButton::new("pose","Cycle affine pose").on_click(cx.listener(|s,_,_,cx|s.edit(0,cx))))
.child(ControlButton::new("wrap","Wrap / nowrap").on_click(cx.listener(|s,_,_,cx|s.edit(1,cx))))
.child(ControlButton::new("decor","Decoration").on_click(cx.listener(|s,_,_,cx|s.edit(2,cx))))
.child(ControlButton::new("ellipsis","Ellipsis / clip").on_click(cx.listener(|s,_,_,cx|s.edit(3,cx))))
.child(ControlButton::new("ancestor","Ancestor clip").on_click(cx.listener(|s,_,_,cx|s.edit(4,cx))))
.child(ControlButton::new("alpha","Opacity 1 / 0.5").on_click(cx.listener(|s,_,_,cx|s.edit(5,cx))))
.child(ControlButton::new("bridge","Font bridge").on_click(cx.listener(|s,_,_,cx|s.backend(cx))))
.child(ControlButton::new("undo","Undo").on_click(cx.listener(|s,_,_,cx|s.view.update(cx,|v,cx|{v.editor.undo();cx.notify();})))))
.child(div().flex_1().min_h_0().overflow_hidden().child(self.view.clone()))
.child(div().h(px(95.)).px_4().text_sm().child("Cycle: identity → rotate 30° → rotate 90° → reflect → nonuniform → shear. The outline uses the same retained transform.\nGlyphs, padding, ellipsis and decorations stay in local space. Affine pixels require the explicit same-App font bridge.\nUndo restores document properties; transform handles remain outside this rendering slice."))
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
                    size(px(1120.), px(620.)),
                    cx,
                ))),
                ..Default::default()
            },
            move |window, cx| {
                window.set_window_title("gpui-flow · Affine Text");
                cx.new(|cx| Lab::new(bridge, cx))
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
