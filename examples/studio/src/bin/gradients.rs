//! Native multi-stop gradient rendering, clipping, affine transforms and alpha.
use gpui::{prelude::*, *};
use gpui_flow::display::{gradient_cache_stats, gradient_render_issues};
use gpui_flow::scene::{
    self as scene, Affine, Color, DisplayObject, GradientStop, Paint, Scene, Stroke, Style,
    display as shapes,
};
use gpui_flow::{ControlButton, FlowTheme, SceneView, SceneViewEvent};
fn stops() -> Vec<GradientStop> {
    vec![
        GradientStop {
            offset: 0.0,
            color: Color::hex(0xff7664),
        },
        GradientStop {
            offset: 0.35,
            color: Color::hex(0xf6d365),
        },
        GradientStop {
            offset: 0.7,
            color: Color::hex(0x62e5c0),
        },
        GradientStop {
            offset: 1.0,
            color: Color::hex(0x8b7cff),
        },
    ]
}
fn linear(width: f32) -> Paint {
    Paint::LinearGradient {
        start: scene::Point::ZERO,
        end: scene::Point::new(width, 0.0),
        stops: stops(),
    }
}
fn radial(center: scene::Point, radius: f32) -> Paint {
    Paint::RadialGradient {
        center,
        radius,
        stops: vec![
            GradientStop {
                offset: 0.0,
                color: Color::WHITE,
            },
            GradientStop {
                offset: 0.35,
                color: Color::hex(0xb693ff),
            },
            GradientStop {
                offset: 0.72,
                color: Color::hex(0x6246c8),
            },
            GradientStop {
                offset: 1.0,
                color: Color::hex(0x14213d),
            },
        ],
    }
}
fn scene() -> Scene {
    let mut scene = Scene::new();
    let p = scene::Point::new;
    let label = |id: &str, title: &str, x: f32, y: f32| {
        DisplayObject::new(id, shapes::Text::new(title, scene::Size::new(300.0, 34.0)))
            .with_transform(Affine::translation(x, y))
            .with_style(Style::fill(Color::hex(0xc9d5e6)))
    };
    let mut add = |object| scene.insert(object, None, None).unwrap();
    add(label("linear-title", "MULTI-STOP LINEAR", 35.0, 25.0));
    add(
        DisplayObject::new("linear", shapes::Rect::new(270.0, 150.0).rounded(24.0))
            .with_transform(Affine::translation(35.0, 75.0))
            .with_style(Style {
                fill: Some(linear(270.0)),
                ..Default::default()
            }),
    );
    add(label("radial-title", "RADIAL · 4 STOPS", 360.0, 25.0));
    add(
        DisplayObject::new("radial", shapes::Ellipse::new(270.0, 150.0))
            .with_transform(Affine::translation(360.0, 75.0))
            .with_style(Style {
                fill: Some(radial(p(110.0, 55.0), 135.0)),
                ..Default::default()
            }),
    );
    add(label("hard-title", "HARD STOPS + ALPHA", 685.0, 25.0));
    add(
        DisplayObject::new("hard-stops", shapes::Rect::new(250.0, 150.0).rounded(20.0))
            .with_transform(Affine::translation(685.0, 75.0))
            .with_style(Style {
                fill: Some(Paint::LinearGradient {
                    start: p(0.0, 0.0),
                    end: p(250.0, 0.0),
                    stops: vec![
                        GradientStop {
                            offset: 0.0,
                            color: Color::rgba(0, 0, 255, 0),
                        },
                        GradientStop {
                            offset: 0.45,
                            color: Color::hex(0xff7664),
                        },
                        GradientStop {
                            offset: 0.45,
                            color: Color::hex(0x62e5c0),
                        },
                        GradientStop {
                            offset: 1.0,
                            color: Color::WHITE,
                        },
                    ],
                }),
                ..Default::default()
            }),
    );
    add(label("stroke-title", "GRADIENT STROKE", 35.0, 280.0));
    let curve = shapes::Pen::new().move_to(p(0.0, 100.0)).cubic_to(
        p(80.0, -65.0),
        p(155.0, 180.0),
        p(270.0, 0.0),
    );
    add(DisplayObject::new("gradient-stroke", curve)
        .with_transform(Affine::translation(45.0, 360.0))
        .with_style(Style {
            fill: None,
            stroke: Some(Stroke {
                paint: linear(270.0),
                width: 22.0,
            }),
            ..Default::default()
        }));
    add(label(
        "transform-title",
        "ROTATE + NONUNIFORM SCALE",
        360.0,
        280.0,
    ));
    add(DisplayObject::new(
        "transformed-radial",
        shapes::Rect::new(170.0, 130.0).rounded(18.0),
    )
    .with_transform(
        Affine::translation(390.0, 345.0) * Affine::rotation(18.0) * Affine::scale(1.3, 0.8),
    )
    .with_style(Style {
        fill: Some(radial(p(75.0, 60.0), 90.0)),
        ..Default::default()
    }));
    add(label("clip-title", "NESTED ROUNDED CLIP", 685.0, 280.0));
    add(
        DisplayObject::new("frame", shapes::Frame::new(250.0, 170.0).rounded(30.0))
            .with_transform(Affine::translation(685.0, 340.0))
            .with_style(Style::fill(Color::hex(0x1e293b))),
    );
    scene
        .insert(
            DisplayObject::new("clipped-gradient", shapes::Star::new(7, 130.0, 65.0))
                .with_transform(Affine::translation(-10.0, -50.0) * Affine::rotation(12.0))
                .with_style(Style {
                    fill: Some(radial(p(130.0, 130.0), 135.0)),
                    ..Default::default()
                }),
            Some("frame".into()),
            None,
        )
        .unwrap();
    scene
}
struct Gallery {
    view: Entity<SceneView>,
    errors: Vec<String>,
    _subscription: Subscription,
}
impl Gallery {
    fn new(_: &mut Window, cx: &mut Context<Self>) -> Self {
        let view = cx.new(|cx| SceneView::new(scene(), cx).unwrap());
        let subscription = cx.subscribe(&view, |_, _, _: &SceneViewEvent, cx| cx.notify());
        Self {
            view,
            errors: Vec::new(),
            _subscription: subscription,
        }
    }
}
impl Render for Gallery {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.errors = gradient_render_issues(window.window_handle().window_id().as_u64())
            .into_iter()
            .map(|issue| format!("{}: {}", issue.object, issue.message))
            .collect();
        if self.errors.len() > 4 {
            self.errors.drain(..self.errors.len() - 4);
        }
        let theme = FlowTheme::default();
        let stats = gradient_cache_stats();
        let controls = div()
            .flex()
            .gap_2()
            .child(
                ControlButton::new("fit", "Fit").on_click(cx.listener(|this, _, _, cx| {
                    this.view.update(cx, |view, cx| {
                        view.fit();
                        cx.notify();
                    });
                })),
            )
            .child(
                ControlButton::new("rotate", "Rotate +15°").on_click(cx.listener(
                    |this, _, _, cx| {
                        this.view.update(cx, |view, cx| {
                            let _ = view.execute_command(
                                gpui_flow::interaction::scene::SceneCommand::RotateSelection {
                                    degrees: 15.0,
                                },
                                cx,
                            );
                            cx.notify();
                        });
                    },
                )),
            )
            .child(
                ControlButton::new("reset", "Reset").on_click(cx.listener(|this, _, _, cx| {
                    this.errors.clear();
                    this.view.update(cx, |view, cx| {
                        let _ = view.set_scene(scene(), cx);
                    });
                })),
            );
        let toolbar = div()
            .h(px(66.0))
            .flex_shrink_0()
            .px_5()
            .flex()
            .items_center()
            .justify_between()
            .bg(rgb(theme.panel))
            .child("◈ Gradient rendering · real color sampling")
            .child(controls);
        let status = format!(
            "Drag / rotate shapes · Ctrl-wheel zoom · F fit  |  cache {} textures / {:.1} MiB · {} hits / {} misses\n{}",
            stats.entries,
            stats.retained_bytes as f32 / 1_048_576.0,
            stats.hits,
            stats.misses,
            self.errors.join(" | ")
        );
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(theme.background))
            .text_color(rgb(theme.foreground))
            .font_family("sans-serif")
            .child(toolbar)
            .child(div().flex_1().min_h_0().child(self.view.clone()))
            .child(
                div()
                    .h(px(50.0))
                    .flex_shrink_0()
                    .px_4()
                    .text_xs()
                    .text_color(rgb(theme.muted))
                    .child(status),
            )
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
                        size(px(1180.0), px(800.0)),
                        cx,
                    ))),
                    ..Default::default()
                },
                |window, cx| {
                    window.set_window_title("gpui-flow Gradients Gallery");
                    gpui_kit::component::Theme::change(
                        gpui_kit::component::ThemeMode::Dark,
                        Some(window),
                        cx,
                    );
                    cx.new(|cx| Gallery::new(window, cx))
                },
            )
            .unwrap();
            cx.activate(true);
        });
}
