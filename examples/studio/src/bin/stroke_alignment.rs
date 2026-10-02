//! Native/CPU comparison of bounded open- and closed-contour stroke meshes.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, SceneView,
    scene::{
        Affine, BlendMode, Color, DisplayObject, FillRule, GradientStop, Paint, Point, Scene,
        SceneError, Shape, Size, Stroke, StrokeAlignment, StrokeCap, StrokeJoin, Style, display,
    },
};

const SCENARIOS: [&str; 7] = [
    "Open L",
    "Open Q/C curve",
    "Straight Line",
    "Closed outer + open inner",
    "SVG open arc",
    "Ellipse open arc",
    "Closed baseline",
];
const HINTS: [&str; 7] = [
    "The end-to-start diagonal closes the fill mask only: it must never be stroked.",
    "Quadratic and cubic segments remain open; caps belong only at the real endpoints.",
    "Zero-area Line: Inside is empty; Outside is twice Center's width, including caps.",
    "Same-winding contours: switch EvenOdd / NonZero; the open inner edge stays unstroked.",
    "SVG A command becomes an open curve; the implicit chord must never be stroked.",
    "Ellipse::arc stays open; compare its real endpoints and the unpainted closing chord.",
    "Original closed rectangle and hole: compare the established closed-contour baseline.",
];

// These two values belong to the document, rather than parallel UI state, so
// history restores the selected scenario and its edited geometry together.
fn fixture_state(scene: &Scene) -> (usize, bool) {
    (
        scene.data["scenario"].as_u64().unwrap_or(0) as usize % SCENARIOS.len(),
        scene.data["geometry_edited"].as_bool().unwrap_or(false),
    )
}
fn geometry(scenario: usize, edited: bool) -> Shape {
    let svg = match (scenario, edited) {
        (0, false) => "M0 0H180V110",
        (0, true) => "M15 15H155V105L105 75",
        (1, false) => "M0 100Q35 -10 85 45C120 105 145 -15 180 30",
        (1, true) => "M5 90Q65 -25 95 65C115 115 155 0 180 70",
        (2, _) => {
            return display::Line::new(
                Point::new(5., if edited { 95. } else { 55. }),
                Point::new(175., if edited { 20. } else { 55. }),
            )
            .into();
        }
        (3, false) => "M0 0H180V110H0Z M55 30H125V80H55",
        (3, true) => "M0 0H175V110H0Z M35 20H130V90H70",
        (4, false) => "M10 90A85 55 0 0 1 170 90",
        (4, true) => "M5 85A90 60 12 0 1 170 105",
        (5, _) => {
            return display::Ellipse::new(180., 110.)
                .arc(
                    if edited { 65. } else { 25. },
                    if edited { 275. } else { 300. },
                )
                .into();
        }
        (6, false) => "M0 0H180V110H0Z M55 30H125V80H55Z",
        (6, true) => "M10 5H170V105H10Z M40 25H135V85H40Z",
        _ => unreachable!(),
    };
    display::Path::from_svg(svg).unwrap().into()
}
fn base_transform() -> Affine {
    Affine::translation(15., 15.)
}
fn representative(scene: &Scene) -> &DisplayObject {
    &scene.get(&"native-center-shape".into()).unwrap().object
}
fn clipped(scene: &Scene) -> bool {
    matches!(
        &scene.get(&"native-center".into()).unwrap().object.shape,
        Shape::Frame(frame) if frame.clip_children
    )
}
fn dash_label(stroke: &Stroke) -> &'static str {
    match stroke.dash_pattern.len() {
        0 => "solid",
        2 => "36/24",
        _ => "36/24/12 @ -13",
    }
}
fn document() -> Scene {
    let mut scene = Scene::new();
    scene.data = serde_json::json!({ "scenario": 0, "geometry_edited": false });
    for (row, y, label) in [
        ("native", 55., "Native vector"),
        ("cpu", 290., "CPU compositing"),
    ] {
        for (col, x, alignment) in [
            ("center", 45., StrokeAlignment::Center),
            ("inside", 355., StrokeAlignment::Inside),
            ("outside", 665., StrokeAlignment::Outside),
        ] {
            let panel = format!("{row}-{col}");
            // The rounded boundary intersects the outer stroke at the corners
            // when enabled; no fill paint is needed to define either mask.
            let mut frame = display::Frame::new(210., 135.).rounded(22.);
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
            let paint = Paint::LinearGradient {
                start: Point::ZERO,
                end: Point::new(180., 0.),
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
                    DisplayObject::new(format!("{panel}-shape"), geometry(0, false))
                        .with_transform(base_transform())
                        .with_style(Style {
                            fill: None,
                            fill_rule: FillRule::EvenOdd,
                            stroke: Some(stroke),
                            stroke_layers: vec![
                                Stroke::new(Color::rgba(255, 255, 255, 150), 2.)
                                    .with_cap(StrokeCap::Butt)
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

fn edit_scene(scene: &mut Scene, operation: &str) -> Result<(), SceneError> {
    let (mut scenario, mut edited) = fixture_state(scene);
    match operation {
        "scenario_next" => {
            scenario = (scenario + 1) % SCENARIOS.len();
            edited = false;
        }
        "scenario_previous" => {
            scenario = (scenario + SCENARIOS.len() - 1) % SCENARIOS.len();
            edited = false;
        }
        "geometry" => edited = !edited,
        _ => {}
    }
    for row in ["native", "cpu"] {
        for col in ["center", "inside", "outside"] {
            let panel = format!("{row}-{col}");
            if operation == "clip" {
                scene.update_object(&panel.into(), |object| {
                    if let Shape::Frame(frame) = &mut object.shape {
                        frame.clip_children = !frame.clip_children;
                    }
                })?;
            } else {
                scene.update_object(
                    &format!("{panel}-shape").into(),
                    |object| match operation {
                        "scenario_next" | "scenario_previous" | "geometry" => {
                            object.shape = geometry(scenario, edited);
                        }
                        "rule" => {
                            object.style.fill_rule = if object.style.fill_rule == FillRule::EvenOdd
                            {
                                FillRule::NonZero
                            } else {
                                FillRule::EvenOdd
                            };
                        }
                        "transform" => {
                            object.transform = if object.transform.a > 0. {
                                Affine::translation(200., 6.)
                                    * Affine::rotation(8.)
                                    * Affine::scale(-0.95, 1.2)
                            } else {
                                base_transform()
                            };
                        }
                        "cap" | "join" | "dash" => {
                            for stroke in object
                                .style
                                .stroke
                                .iter_mut()
                                .chain(object.style.stroke_layers.iter_mut())
                            {
                                match operation {
                                    "cap" => {
                                        stroke.cap = match stroke.cap {
                                            StrokeCap::Butt => StrokeCap::Round,
                                            StrokeCap::Round => StrokeCap::Square,
                                            StrokeCap::Square => StrokeCap::Butt,
                                        };
                                    }
                                    "join" => {
                                        stroke.join = match stroke.join {
                                            StrokeJoin::Miter => StrokeJoin::Round,
                                            StrokeJoin::Round => StrokeJoin::Bevel,
                                            StrokeJoin::Bevel => StrokeJoin::Miter,
                                        };
                                    }
                                    "dash" => {
                                        let (pattern, offset) = match stroke.dash_pattern.len() {
                                            0 => (vec![36., 24.], 0.),
                                            2 => (vec![36., 24., 12.], -13.),
                                            _ => (vec![], 0.),
                                        };
                                        stroke.dash_pattern = pattern;
                                        stroke.dash_offset = offset;
                                    }
                                    _ => unreachable!(),
                                }
                            }
                        }
                        _ => unreachable!(),
                    },
                )?;
            }
        }
    }
    scene.data = serde_json::json!({ "scenario": scenario, "geometry_edited": edited });
    Ok(())
}

// One record per user action (plus startup), never from the render loop. The
// representative style/geometry is shared by all six panels except alignment.
fn diagnostic(view: &SceneView, action: &str, changed: bool, error: Option<String>) {
    let scene = view.editor.scene();
    let (scenario, edited) = fixture_state(scene);
    let object = representative(scene);
    let panels: Vec<_> = ["native", "cpu"]
        .into_iter()
        .flat_map(|backend| {
            ["center", "inside", "outside"].map(move |alignment| {
                let id = format!("{backend}-{alignment}-shape");
                let object = &scene.get(&id.as_str().into()).unwrap().object;
                serde_json::json!({
                    "id": id,
                    "backend": backend,
                    "alignment": object.style.stroke.as_ref().unwrap().alignment,
                    "shape_kind": object.shape.tag(),
                    "fill_rule": object.style.fill_rule,
                })
            })
        })
        .collect();
    eprintln!(
        "{}",
        serde_json::json!({
            "fixture": "stroke_alignment",
            "action": action,
            "changed": changed,
            "revision": view.editor.engine.revision(),
            "scenario_index": scenario + 1,
            "scenario": SCENARIOS[scenario],
            "geometry_edited": edited,
            "geometry": object.shape,
            "style": object.style,
            "fill_rule": object.style.fill_rule,
            "transform": object.transform,
            "rounded_clip": clipped(scene),
            "alignments": ["center", "inside", "outside"],
            "backends": ["native", "cpu"],
            "panels": panels,
            "error": error,
        })
    );
}

struct Lab {
    view: Entity<SceneView>,
}
impl Lab {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            view: cx.new(|cx| {
                let mut view = SceneView::new(document(), cx).unwrap();
                view.show_grid = false;
                view.set_editable(false, cx);
                diagnostic(&view, "startup", true, None);
                view
            }),
        }
    }
    fn edit(&mut self, operation: &str, cx: &mut Context<Self>) {
        self.view.update(cx, |view, cx| {
            let (changed, error) = match operation {
                "undo" => (view.editor.undo(), None),
                "redo" => (view.editor.redo(), None),
                _ => match view
                    .editor
                    .transaction(|scene| edit_scene(scene, operation))
                {
                    Ok(()) => (true, None),
                    Err(error) => (false, Some(error.to_string())),
                },
            };
            diagnostic(view, operation, changed, error);
            cx.notify();
        });
        cx.notify();
    }
}
impl Render for Lab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = self.view.read(cx);
        let scene = view.editor.scene();
        let (scenario, edited) = fixture_state(scene);
        let object = representative(scene);
        let stroke = object.style.stroke.as_ref().unwrap();
        let cap = format!("Cap: {:?}", stroke.cap);
        let join = format!("Join: {:?}", stroke.join);
        let dash = format!("Dash: {}", dash_label(stroke));
        let rule = format!("Rule: {:?}", object.style.fill_rule);
        let affine = format!(
            "Affine: {}",
            if object.transform.a < 0. { "on" } else { "off" }
        );
        let clip = format!("Clip: {}", if clipped(scene) { "on" } else { "off" });
        let status = format!(
            "{} / {} · {} · geometry {} · revision {}",
            scenario + 1,
            SCENARIOS.len(),
            SCENARIOS[scenario],
            if edited { "edited" } else { "original" },
            view.editor.engine.revision(),
        );
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0x161b25))
            .text_color(rgb(0xffffff))
            .child(
                div()
                    .h(px(48.))
                    .flex_shrink_0()
                    .px_4()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child("Stroke alignment")
                    .child(ControlButton::new("previous", "Previous scenario").on_click(
                        cx.listener(|this, _, _, cx| this.edit("scenario_previous", cx)),
                    ))
                    .child(ControlButton::new("next", "Next scenario").on_click(
                        cx.listener(|this, _, _, cx| this.edit("scenario_next", cx)),
                    ))
                    .child(ControlButton::new("geometry", "Edit geometry").on_click(
                        cx.listener(|this, _, _, cx| this.edit("geometry", cx)),
                    ))
                    .child(ControlButton::new("undo", "Undo").on_click(
                        cx.listener(|this, _, _, cx| this.edit("undo", cx)),
                    ))
                    .child(ControlButton::new("redo", "Redo").on_click(
                        cx.listener(|this, _, _, cx| this.edit("redo", cx)),
                    )),
            )
            .child(
                div()
                    .h(px(48.))
                    .flex_shrink_0()
                    .px_4()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(ControlButton::new("cap", cap).on_click(
                        cx.listener(|this, _, _, cx| this.edit("cap", cx)),
                    ))
                    .child(ControlButton::new("join", join).on_click(
                        cx.listener(|this, _, _, cx| this.edit("join", cx)),
                    ))
                    .child(ControlButton::new("dash", dash).on_click(
                        cx.listener(|this, _, _, cx| this.edit("dash", cx)),
                    ))
                    .child(ControlButton::new("rule", rule).on_click(
                        cx.listener(|this, _, _, cx| this.edit("rule", cx)),
                    ))
                    .child(ControlButton::new("transform", affine).on_click(
                        cx.listener(|this, _, _, cx| this.edit("transform", cx)),
                    ))
                    .child(ControlButton::new("clip", clip).on_click(
                        cx.listener(|this, _, _, cx| this.edit("clip", cx)),
                    )),
            )
            .child(div().h(px(28.)).flex_shrink_0().px_4().text_sm().child(status))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(self.view.clone()),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .px_4()
                    .py_2()
                    .text_sm()
                    .child(div().child(HINTS[scenario]))
                    .child(div().child("No fill paint; translucent gradient + white stroke layer. Affine = reflection, 0.95×/1.2× scale, 8° rotation."))
                    .child(div().child("Every action edits all six panels in one transaction. Undo / Redo restore scene and controls; pan / zoom to inspect.")),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn every_scenario_and_geometry_variant_accepts_the_control_tour() {
        let mut scene = document();
        // Exercise the odd, negative-phase dash with round, square, and butt
        // caps; both rules, clips, and affine states occur during this tour.
        // All controls return to their initial values before changing geometry.
        let tour = [
            "cap",
            "join",
            "dash",
            "rule",
            "transform",
            "clip",
            "dash",
            "cap",
            "join",
            "rule",
            "cap",
            "join",
            "dash",
            "transform",
            "clip",
        ];
        for (scenario, name) in SCENARIOS.iter().enumerate() {
            let before = scene.clone();
            for edited in [false, true] {
                assert_eq!(fixture_state(&scene), (scenario, edited));
                for action in tour {
                    edit_scene(&mut scene, action).unwrap_or_else(|error| {
                        panic!("{name} edited={edited}, {action}: {error}")
                    });
                    let reference = representative(&scene);
                    for row in ["native", "cpu"] {
                        for col in ["center", "inside", "outside"] {
                            let id = format!("{row}-{col}-shape");
                            let object = &scene.get(&id.into()).unwrap().object;
                            assert_eq!(object.shape, reference.shape);
                            assert_eq!(object.transform, reference.transform);
                            let mut style = object.style.clone();
                            for stroke in style
                                .stroke
                                .iter_mut()
                                .chain(style.stroke_layers.iter_mut())
                            {
                                stroke.alignment = StrokeAlignment::Center;
                            }
                            assert_eq!(style, reference.style);
                        }
                    }
                }
                scene.validate().unwrap();
                edit_scene(&mut scene, "geometry").unwrap();
            }
            assert_eq!(
                scene, before,
                "the control/geometry tour must be reversible"
            );
            edit_scene(&mut scene, "scenario_next").unwrap();
        }
        assert_eq!(
            scene,
            document(),
            "scenario cycling must return to the original document"
        );
        edit_scene(&mut scene, "scenario_previous").unwrap();
        assert_eq!(fixture_state(&scene), (SCENARIOS.len() - 1, false));
    }
}
