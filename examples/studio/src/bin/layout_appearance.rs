//! Explicit visual layout plus layered vector paints, independent of graph workflows.
use gpui::{prelude::*, *};
use gpui_flow::layout::{
    AlignItems, FlowDirection, FlowItem, FlowLayout, FlowStyle, Insets, JustifyContent,
};
use gpui_flow::scene::{
    Affine, Color, DisplayObject, GradientStop, Paint, Scene, Stroke, Style, display,
};
use gpui_flow::{ControlButton, FlowTheme, SceneView, SceneViewEvent};

fn card_style(color: u32) -> Style {
    Style {
        fill: Some(Color::hex(color).into()),
        fill_layers: vec![Paint::LinearGradient {
            start: gpui_flow::scene::Point::ZERO,
            end: gpui_flow::scene::Point::new(0., 75.),
            stops: vec![
                GradientStop {
                    offset: 0.,
                    color: Color::rgba(255, 255, 255, 145),
                },
                GradientStop {
                    offset: 1.,
                    color: Color::TRANSPARENT,
                },
            ],
        }],
        stroke: Some(Stroke::new(Color::rgba(255, 255, 255, 45), 8.)),
        stroke_layers: vec![Stroke::new(Color::rgba(255, 255, 255, 210), 1.5)],
        ..Default::default()
    }
}
fn document() -> (Scene, FlowLayout) {
    let mut scene = Scene::new();
    let mut layout = FlowLayout::default();
    for (id, title, x, y, w, h) in [
        ("wrap", "ROW / WRAP / GAP", 35., 55., 830., 225.),
        ("column", "COLUMN / SPACE BETWEEN", 35., 340., 250., 275.),
        ("grow", "GROW / MIN / MAX", 320., 340., 545., 275.),
    ] {
        scene
            .insert(
                DisplayObject::new(
                    format!("{id}-title"),
                    display::Text::new(title, gpui_flow::scene::Size::new(w, 28.)),
                )
                .with_transform(Affine::translation(x, y - 33.))
                .with_style(Style::fill(Color::hex(0xbccade))),
                None,
                None,
            )
            .unwrap();
        scene
            .insert(
                DisplayObject::new(id, display::Frame::new(w, h).rounded(16.))
                    .with_transform(Affine::translation(x, y))
                    .with_style(Style::fill(Color::hex(0x172235))),
                None,
                None,
            )
            .unwrap();
    }
    layout.containers.insert(
        "wrap".into(),
        FlowStyle {
            padding: Insets::all(22.),
            gap: 16.,
            line_gap: 18.,
            wrap: true,
            align_items: AlignItems::Center,
            ..Default::default()
        },
    );
    layout.containers.insert(
        "column".into(),
        FlowStyle {
            direction: FlowDirection::Column,
            padding: Insets::all(20.),
            gap: 12.,
            justify: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            ..Default::default()
        },
    );
    layout.containers.insert(
        "grow".into(),
        FlowStyle {
            padding: Insets::all(24.),
            gap: 18.,
            align_items: AlignItems::Stretch,
            ..Default::default()
        },
    );
    let colors = [
        0x36a99e, 0x7376de, 0xc67855, 0x5099c3, 0xbb66a0, 0x879e4b, 0xc7a04e,
    ];
    for (i, color) in colors.into_iter().enumerate() {
        scene
            .insert(
                DisplayObject::new(
                    format!("wrap-{i}"),
                    display::Rect::new(140. + (i % 3) as f32 * 18., 65.).rounded(12.),
                )
                .with_style(card_style(color)),
                Some("wrap".into()),
                None,
            )
            .unwrap();
    }
    for i in 0..3 {
        scene
            .insert(
                DisplayObject::new(
                    format!("column-{i}"),
                    display::Rect::new(175., 55.).rounded(10.),
                )
                .with_style(card_style(colors[i])),
                Some("column".into()),
                None,
            )
            .unwrap();
        let id = format!("grow-{i}");
        scene
            .insert(
                DisplayObject::new(id.clone(), display::Rect::new(40., 65.).rounded(12.))
                    .with_style(card_style(colors[i + 3])),
                Some("grow".into()),
                None,
            )
            .unwrap();
        layout.items.insert(
            id.into(),
            FlowItem {
                grow: (i + 1) as f32,
                min_width: Some(45.),
                max_width: if i == 0 { Some(80.) } else { None },
                ..Default::default()
            },
        );
    }
    layout
        .apply_to_scene(&mut scene)
        .expect("valid gallery layout");
    (scene, layout)
}
struct Gallery {
    view: Entity<SceneView>,
    layout: FlowLayout,
    result: String,
    _subscription: Subscription,
}
impl Gallery {
    fn new(cx: &mut Context<Self>) -> Self {
        let (scene, layout) = document();
        let view = cx.new(|cx| SceneView::new(scene, cx).unwrap());
        let subscription = cx.subscribe(&view, |_, _, _: &SceneViewEvent, cx| cx.notify());
        Self {
            view,
            layout,
            result: "Drag a card or resize a frame, then Reflow. Undo restores the complete edit."
                .into(),
            _subscription: subscription,
        }
    }
}
impl Render for Gallery {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = FlowTheme::default();
        let status = format!(
            "{} | revision {} | explicit flow pass, flat alpha compositing",
            self.result,
            self.view.read(cx).editor.engine.revision()
        );
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(theme.background))
            .text_color(rgb(theme.foreground))
            .font_family("sans-serif")
            .child(
                div()
                    .h(px(60.))
                    .px_5()
                    .flex()
                    .items_center()
                    .justify_between()
                    .bg(rgb(theme.panel))
                    .child("Layout + layered appearance")
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(ControlButton::new("reflow", "Reflow").on_click(cx.listener(
                                |this, _, _, cx| {
                                    let result = this.view.update(cx, |view, cx| {
                                        let result = this.layout.apply_to_editor(&mut view.editor);
                                        cx.notify();
                                        result
                                    });
                                    this.result = match result {
                                        Ok(report) => format!(
                                            "Reflow: {} containers, {} changed elements",
                                            report.containers,
                                            report.changed.len()
                                        ),
                                        Err(error) => error.to_string(),
                                    };
                                    cx.notify();
                                },
                            )))
                            .child(ControlButton::new("undo", "Undo").on_click(cx.listener(
                                |this, _, _, cx| {
                                    this.view.update(cx, |view, cx| {
                                        view.editor.undo();
                                        cx.notify();
                                    });
                                },
                            )))
                            .child(ControlButton::new("redo", "Redo").on_click(cx.listener(
                                |this, _, _, cx| {
                                    this.view.update(cx, |view, cx| {
                                        view.editor.redo();
                                        cx.notify();
                                    });
                                },
                            )))
                            .child(ControlButton::new("fit", "Fit").on_click(cx.listener(
                                |this, _, _, cx| {
                                    this.view.update(cx, |view, cx| {
                                        view.fit();
                                        cx.notify();
                                    });
                                },
                            )))
                            .child(ControlButton::new("reset", "Reset").on_click(cx.listener(
                                |this, _, _, cx| {
                                    let (scene, layout) = document();
                                    this.layout = layout;
                                    this.view.update(cx, |view, cx| {
                                        view.set_scene(scene, cx).unwrap();
                                    });
                                    this.result = "Reset gallery".into();
                                    cx.notify();
                                },
                            ))),
                    ),
            )
            .child(div().flex_1().min_h_0().child(self.view.clone()))
            .child(
                div()
                    .min_h(px(42.))
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
                        size(px(1100.), px(820.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                |window, cx| {
                    window.set_window_title("gpui-flow · layout and appearance");
                    gpui_kit::component::Theme::change(
                        gpui_kit::component::ThemeMode::Dark,
                        Some(window),
                        cx,
                    );
                    cx.new(Gallery::new)
                },
            )
            .expect("open gallery");
            cx.activate(true);
        });
}
