//! Native/CPU comparison of the same bounded, closed-contour stroke meshes.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, SceneView,
    scene::{
        Affine, BlendMode, Color, DisplayObject, FillRule, GradientStop, Paint, Scene, Shape, Size,
        Stroke, StrokeAlignment, StrokeCap, StrokeJoin, Style, display,
    },
};
fn document() -> Scene {
    let mut scene = Scene::new();
    for (row, y, label) in [
        ("native", 85., "Native vector"),
        ("cpu", 310., "CPU compositing"),
    ] {
        for (col, x, alignment) in [
            ("center", 45., StrokeAlignment::Center),
            ("inside", 355., StrokeAlignment::Inside),
            ("outside", 665., StrokeAlignment::Outside),
        ] {
            let panel = format!("{row}-{col}");
            let mut frame = display::Frame::new(240., 165.).rounded(20.);
            frame.clip_children = false;
            scene
                .insert(
                    DisplayObject::new(panel.clone(), frame)
                        .with_transform(Affine::translation(x, y))
                        .with_style(Style {
                            fill: None,
                            blend_mode: if row == "cpu" {
                                BlendMode::Normal
                            } else {
                                BlendMode::PassThrough
                            },
                            ..Default::default()
                        }),
                    None,
                    None,
                )
                .unwrap();
            let path = display::Path::from_svg("M0 0H180V110H0Z M55 30H125V80H55Z").unwrap();
            let paint = Paint::LinearGradient {
                start: gpui_flow::scene::Point::ZERO,
                end: gpui_flow::scene::Point::new(180., 0.),
                stops: vec![
                    GradientStop {
                        offset: 0.,
                        color: Color::rgba(255, 70, 110, 190),
                    },
                    GradientStop {
                        offset: 1.,
                        color: Color::rgba(70, 180, 255, 190),
                    },
                ],
            };
            let mut stroke = Stroke::new(Color::WHITE, 12.)
                .with_join(StrokeJoin::Miter)
                .with_cap(StrokeCap::Butt)
                .with_alignment(alignment);
            stroke.paint = paint;
            scene
                .insert(
                    DisplayObject::new(format!("{panel}-shape"), path)
                        .with_transform(Affine::translation(30., 25.))
                        .with_style(Style {
                            fill: None,
                            fill_rule: FillRule::EvenOdd,
                            stroke: Some(stroke),
                            stroke_layers: vec![
                                Stroke::new(Color::rgba(255, 255, 255, 150), 2.)
                                    .with_join(StrokeJoin::Miter)
                                    .with_alignment(alignment),
                            ],
                            ..Default::default()
                        }),
                    Some(panel.into()),
                    None,
                )
                .unwrap();
            let mut text = display::Text::new(format!("{label} · {col}"), Size::new(290., 30.));
            text.font_size = 18.;
            scene
                .insert(
                    DisplayObject::new(format!("{row}-{col}-label"), text)
                        .with_transform(Affine::translation(x, y - 38.))
                        .with_style(Style::fill(Color::WHITE)),
                    None,
                    None,
                )
                .unwrap();
        }
    }
    scene
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
    fn edit(&mut self, operation: &str, cx: &mut Context<Self>) {
        self.view.update(cx, |v, cx| {
            v.editor
                .transaction(|scene| {
                    for row in ["native", "cpu"] {
                        for col in ["center", "inside", "outside"] {
                            let panel = format!("{row}-{col}");
                            if operation == "clip" {
                                scene.update_object(&panel.into(), |o| {
                                    if let Shape::Frame(f) = &mut o.shape {
                                        f.clip_children = !f.clip_children;
                                    }
                                })?;
                            } else {
                                scene.update_object(&format!("{panel}-shape").into(), |o| {
                                    match operation {
                                        "rule" => {
                                            o.style.fill_rule =
                                                if o.style.fill_rule == FillRule::EvenOdd {
                                                    FillRule::NonZero
                                                } else {
                                                    FillRule::EvenOdd
                                                }
                                        }
                                        "dash" => {
                                            let s = o.style.stroke.as_mut().unwrap();
                                            s.dash_pattern = if s.dash_pattern.is_empty() {
                                                vec![16., 10.]
                                            } else {
                                                vec![]
                                            };
                                        }
                                        "transform" => {
                                            o.transform = if o.transform.a > 0. {
                                                Affine::translation(212., 12.)
                                                    * Affine::rotation(8.)
                                                    * Affine::scale(-0.95, 1.2)
                                            } else {
                                                Affine::translation(30., 25.)
                                            }
                                        }
                                        _ => unreachable!(),
                                    }
                                })?;
                            }
                        }
                    }
                    Ok(())
                })
                .unwrap();
            cx.notify();
        });
        cx.notify();
    }
}
impl Render for Lab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().flex().flex_col().bg(rgb(0x161b25)).text_color(rgb(0xffffff))
            .child(div().h(px(58.)).px_4().flex().items_center().gap_2().child("Stroke alignment")
                .child(ControlButton::new("rule","EvenOdd / NonZero").on_click(cx.listener(|this,_,_,cx|this.edit("rule",cx))))
                .child(ControlButton::new("dash","Dashes").on_click(cx.listener(|this,_,_,cx|this.edit("dash",cx))))
                .child(ControlButton::new("transform","Reflect / transform").on_click(cx.listener(|this,_,_,cx|this.edit("transform",cx))))
                .child(ControlButton::new("clip","Rounded clip").on_click(cx.listener(|this,_,_,cx|this.edit("clip",cx))))
                .child(ControlButton::new("undo","Undo").on_click(cx.listener(|this,_,_,cx|{this.view.update(cx,|v,cx|{v.editor.undo();cx.notify();});cx.notify();}))))
            .child(div().flex_1().min_h_0().overflow_hidden().child(self.view.clone()))
            .child(div().h(px(62.)).px_4().text_sm().child("Same final mesh in both rows. No fill paint; holes still control Inside / Outside.\nToggle winding, dash, affine and clip; Undo restores one edit. Pan/zoom to compare boundaries."))
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
                        size(px(1060.), px(720.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                |window, cx| {
                    window.set_window_title("gpui-flow · Stroke alignment");
                    cx.new(Lab::new)
                },
            )
            .unwrap();
            cx.activate(true);
        });
}
