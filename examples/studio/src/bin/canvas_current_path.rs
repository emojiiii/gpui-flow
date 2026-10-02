//! Persistent Canvas geometry with batch-local drawing state, at DPR 1 and 2.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, SceneView,
    composite::{CanvasCommand, CanvasSurface, CompositeError},
    scene::{
        Affine, Color, DisplayObject, FillRule, GradientStop, Paint, Point as ScenePoint, Scene,
        Shape, Size as SceneSize, Stroke, Style, display,
    },
};
use std::sync::Arc;

fn point(x: f32, y: f32) -> ScenePoint {
    ScenePoint::new(x, y)
}

fn outline() -> Vec<CanvasCommand> {
    vec![
        CanvasCommand::BeginPath,
        CanvasCommand::SetTransform(Affine::translation(24., 22.)),
        CanvasCommand::MoveTo {
            to: point(14., 44.),
        },
        CanvasCommand::QuadraticCurveTo {
            control: point(62., -10.),
            to: point(120., 22.),
        },
        CanvasCommand::BezierCurveTo {
            control1: point(218., -12.),
            control2: point(276., 112.),
            to: point(199., 151.),
        },
        CanvasCommand::QuadraticCurveTo {
            control: point(100., 196.),
            to: point(14., 123.),
        },
        CanvasCommand::ClosePath,
        // Neither this opacity nor the translation survives this path-only batch.
        CanvasCommand::SetOpacity(0.17),
    ]
}

fn hole() -> Vec<CanvasCommand> {
    vec![
        CanvasCommand::Save,
        CanvasCommand::Transform(Affine::translation(107., 91.)),
        CanvasCommand::MoveTo { to: point(0., 0.) },
        CanvasCommand::LineTo { to: point(60., 0.) },
        CanvasCommand::LineTo {
            to: point(60., 44.),
        },
        CanvasCommand::LineTo { to: point(0., 44.) },
        CanvasCommand::ClosePath,
        // Restore removes the translation, but deliberately keeps the new subpath.
        CanvasCommand::Restore,
    ]
}

fn drawing(transformed: bool) -> Vec<CanvasCommand> {
    vec![
        CanvasCommand::SetTransform(if transformed {
            Affine::translation(75., 0.) * Affine::scale(0.65, 1.8)
        } else {
            Affine::IDENTITY
        }),
        CanvasCommand::FillCurrentPath {
            paint: Paint::LinearGradient {
                start: point(24., 0.),
                end: point(270., 0.),
                stops: vec![
                    GradientStop {
                        offset: 0.,
                        color: Color::hex(0xff805c),
                    },
                    GradientStop {
                        offset: 0.5,
                        color: Color::hex(0xffd471),
                    },
                    GradientStop {
                        offset: 1.,
                        color: Color::hex(0x65d7e5),
                    },
                ],
            },
            rule: FillRule::EvenOdd,
        },
        CanvasCommand::StrokeCurrentPath {
            stroke: Stroke::new(Color::hex(0xf6f1d7), 3.),
        },
    ]
}

fn fill_path(svg: &str, color: Color) -> CanvasCommand {
    CanvasCommand::FillPath {
        path: display::Path::from_svg(svg).unwrap(),
        paint: color.into(),
        rule: FillRule::NonZero,
    }
}

fn clipping() -> Vec<CanvasCommand> {
    vec![
        CanvasCommand::Save,
        // Current geometry is already frozen: this must not move the clip.
        CanvasCommand::SetTransform(Affine::translation(45., 10.)),
        CanvasCommand::ClipCurrentPath { rule: FillRule::EvenOdd },
        CanvasCommand::SetTransform(Affine::IDENTITY),
        fill_path("M 0 0 L 300 0 L 300 210 L 0 210 Z", Color::hex(0x2759a3)),
        CanvasCommand::StrokePath {
            path: display::Path::from_svg(
                "M -30 0 L 140 210 M 25 0 L 195 210 M 80 0 L 250 210 M 135 0 L 305 210 M 190 0 L 360 210 M 245 0 L 415 210",
            ).unwrap(),
            stroke: Stroke::new(Color::hex(0x78e0cf), 18.),
        },
        CanvasCommand::Restore,
        // The corner square proves Restore removed the batch clip.
        fill_path("M 6 6 L 18 6 L 18 18 L 6 18 Z", Color::hex(0xffd471)),
    ]
}

fn rejected() -> Vec<CanvasCommand> {
    vec![
        CanvasCommand::BeginPath,
        CanvasCommand::MoveTo { to: point(8., 8.) },
        CanvasCommand::LineTo {
            to: point(292., 8.),
        },
        CanvasCommand::LineTo {
            to: point(292., 202.),
        },
        CanvasCommand::ClosePath,
        CanvasCommand::FillCurrentPath {
            paint: Color::WHITE.into(),
            rule: FillRule::NonZero,
        },
        // Fail after replacing geometry and requesting white pixels.
        CanvasCommand::Restore,
    ]
}

#[derive(Clone, Copy)]
enum Action {
    Build,
    Append,
    Draw,
    TransformDraw,
    Clip,
    Reject,
    Clear,
    Begin,
}

fn apply(surface: &mut CanvasSurface, action: Action) -> Result<String, CompositeError> {
    let before_generation = surface.generation();
    let before_pixels = surface.snapshot();
    if matches!(action, Action::Reject) {
        let mut expected = surface.clone();
        let result = surface.draw_commands(&rejected());
        if result.is_ok() {
            return Ok("FAIL: invalid Restore was accepted".into());
        }
        let unchanged = before_generation == surface.generation()
            && Arc::ptr_eq(&before_pixels, &surface.snapshot());
        // Replay on temporary clones checks rollback of the hidden geometry as
        // well as live snapshot identity. Neither replay edits the live Canvas.
        let mut actual = surface.clone();
        expected.clear()?;
        actual.clear()?;
        expected.draw_commands(&drawing(false))?;
        actual.draw_commands(&drawing(false))?;
        let path_unchanged =
            expected.snapshot().premultiplied_rgba() == actual.snapshot().premultiplied_rgba();
        return Ok(format!(
            "rejected; pixels/generation intact: {unchanged}; path replay intact: {path_unchanged}"
        ));
    }
    let published = match action {
        Action::Build => surface.draw_commands(&outline())?,
        Action::Append => surface.draw_commands(&hole())?,
        Action::Draw => surface.draw_commands(&drawing(false))?,
        Action::TransformDraw => surface.draw_commands(&drawing(true))?,
        Action::Clip => {
            let clipped = surface.draw_commands(&clipping())?;
            // A separate batch also starts unclipped, with the same current path.
            let next = surface.draw_commands(&[fill_path(
                "M 278 188 L 292 188 L 292 202 L 278 202 Z",
                Color::hex(0xff805c),
            )])?;
            clipped || next
        }
        Action::Clear => {
            surface.clear()?;
            true
        }
        Action::Begin => surface.draw_commands(&[CanvasCommand::BeginPath])?,
        Action::Reject => unreachable!(),
    };
    Ok(format!(
        "published {published}; generation {before_generation} -> {}; same snapshot {}",
        surface.generation(),
        Arc::ptr_eq(&before_pixels, &surface.snapshot()),
    ))
}

fn document() -> Scene {
    let mut scene = Scene::new();
    for (id, x, dpr, label) in [
        ("one", 38., 1., "Left surface · starts at DPR 1"),
        ("two", 395., 2., "Right surface · starts at DPR 2"),
    ] {
        let mut canvas = display::Canvas::new(300., 210.).rounded(12.);
        canvas.pixel_ratio = dpr;
        canvas.safe_resize = true;
        scene
            .insert(
                DisplayObject::new(id, canvas)
                    .with_transform(Affine::translation(x, 62.))
                    .with_style(Style {
                        fill: Some(Color::hex(0x182b42).into()),
                        stroke: Some(Stroke::new(Color::hex(0x439cc3), 2.)),
                        ..Default::default()
                    }),
                None,
                None,
            )
            .unwrap();
        let mut text = display::Text::new(label, SceneSize::new(350., 35.));
        text.font_size = 17.;
        scene
            .insert(
                DisplayObject::new(format!("label-{id}"), text)
                    .with_transform(Affine::translation(x, 18.))
                    .with_style(Style::fill(Color::WHITE)),
                None,
                None,
            )
            .unwrap();
    }
    scene
}

struct Lab {
    view: Entity<SceneView>,
    status: String,
    button_action_count: u64,
    action_origin: &'static str,
    last_edit_accepted: bool,
}

impl Lab {
    fn new(cx: &mut Context<Self>) -> Self {
        let view = cx.new(|cx| {
            let mut view = SceneView::new(document(), cx).unwrap();
            view.show_grid = false;
            view.set_editable(false, cx);
            view
        });
        let mut lab = Self {
            view,
            status: String::new(),
            button_action_count: 0,
            action_origin: "startup",
            last_edit_accepted: true,
        };
        lab.regenerate(cx);
        lab.action_origin = "button";
        lab
    }

    fn begin_action(&mut self) {
        if self.action_origin == "button" {
            self.button_action_count += 1;
        }
    }

    fn record(&self, action: &str, accepted: bool, detail: serde_json::Value, cx: &Context<Self>) {
        let view = self.view.read(cx);
        let configs = ["one", "two"].map(|id| {
            let object = &view.editor.scene().get(&id.into()).unwrap().object;
            match &object.shape {
                Shape::Canvas(canvas) => serde_json::json!({"id": id, "config": canvas}),
                _ => serde_json::json!({"id": id, "config": null}),
            }
        });
        eprintln!(
            "{}",
            serde_json::json!({
                "fixture": "canvas_current_path",
                "action": action,
                "origin": self.action_origin,
                "button_action_count": self.button_action_count,
                "accepted": accepted,
                "status": self.status,
                "revision": view.editor.engine.revision(),
                "canvases": configs,
                "detail": detail,
            })
        );
    }

    fn edit(&mut self, action: Action, description: &str, cx: &mut Context<Self>) {
        self.begin_action();
        let (reports, outcomes, accepted) = self.view.update(cx, |view, cx| {
            let mut reports = Vec::new();
            let mut outcomes = Vec::new();
            let mut accepted = true;
            for (id, label) in [("one", "Left"), ("two", "Right")] {
                let handle = view.editor.engine.handle(&id.into()).unwrap();
                let mut report = String::new();
                let result = view.edit_canvas(
                    handle,
                    |surface| {
                        report = apply(surface, action)?;
                        Ok(())
                    },
                    cx,
                );
                let surface_accepted = result.is_ok();
                accepted &= surface_accepted;
                let status = match result {
                    Ok(()) => format!("{label}: {report}"),
                    Err(error) => format!("{label}: edit failed: {error}"),
                };
                outcomes.push(serde_json::json!({
                    "id": id,
                    "accepted": surface_accepted,
                    "status": status,
                }));
                reports.push(status);
            }
            (reports.join("\n"), outcomes, accepted)
        });
        self.status = format!("{description}\n{reports}");
        self.last_edit_accepted = accepted;
        let action_name = match action {
            Action::Build => "build",
            Action::Append => "append",
            Action::Draw => "draw",
            Action::TransformDraw => "transform_draw",
            Action::Clip => "clip",
            Action::Reject => "reject",
            Action::Clear => "clear",
            Action::Begin => "begin_path",
        };
        self.record(
            action_name,
            accepted,
            serde_json::json!({
                "description": description,
                "expected_batch_rejection": matches!(action, Action::Reject),
                "surfaces": outcomes,
            }),
            cx,
        );
        cx.notify();
    }

    fn regenerate(&mut self, cx: &mut Context<Self>) {
        self.begin_action();
        let origin = self.action_origin;
        self.action_origin = if origin == "startup" {
            "startup_substep"
        } else {
            "regenerate_substep"
        };
        // Separate adapter edits deliberately test path-only state retention.
        self.edit(Action::Clear, "Clear", cx);
        let mut accepted = self.last_edit_accepted;
        self.edit(Action::Build, "Build", cx);
        accepted &= self.last_edit_accepted;
        self.edit(Action::Append, "Append", cx);
        accepted &= self.last_edit_accepted;
        self.edit(
            Action::Draw,
            "Regenerated: outline, hole and draw were separate Canvas edits.",
            cx,
        );
        accepted &= self.last_edit_accepted;
        self.action_origin = origin;
        self.record(
            "regenerate",
            accepted,
            serde_json::json!({"edit_substeps": 4}),
            cx,
        );
    }

    fn resize(&mut self, safe: bool, cx: &mut Context<Self>) {
        self.begin_action();
        let result = self.view.update(cx, |view, cx| {
            let result = view.editor.transaction(|scene| {
                for id in ["one", "two"] {
                    scene.update_object(&id.into(), |object| {
                        if let Shape::Canvas(canvas) = &mut object.shape {
                            canvas.safe_resize = safe;
                            canvas.size.width = if canvas.size.width > 200. { 180. } else { 300. };
                        }
                    })?;
                }
                Ok(())
            });
            cx.notify();
            result
        });
        self.status = match &result {
            Ok(_) if safe => "Safe resize preserves the physical-pixel intersection but resets the current path. Fill + stroke should publish false; growing cannot restore cropped pixels.".into(),
            Ok(_) => "Reset resize clears pixels and the current path. Undo config restores dimensions, without replaying geometry or drawing commands.".into(),
            Err(error) => format!("Resize failed: {error}"),
        };
        self.record(
            "resize",
            result.is_ok(),
            serde_json::json!({"safe": safe}),
            cx,
        );
        cx.notify();
    }

    fn swap_dpr(&mut self, cx: &mut Context<Self>) {
        self.begin_action();
        let result = self.view.update(cx, |view, cx| {
            let result = view.editor.transaction(|scene| {
                for id in ["one", "two"] {
                    scene.update_object(&id.into(), |object| {
                        if let Shape::Canvas(canvas) = &mut object.shape {
                            canvas.safe_resize = true;
                            canvas.pixel_ratio = if canvas.pixel_ratio == 1. { 2. } else { 1. };
                        }
                    })?;
                }
                Ok(())
            });
            cx.notify();
            result
        });
        self.status = match &result {
            Ok(_) => "DPR swapped with safe-resize: physical pixels are copied without scaling; current paths reset. Regenerate restores logical placement at the new DPR.".into(),
            Err(error) => format!("DPR swap failed: {error}"),
        };
        self.record(
            "swap_dpr",
            result.is_ok(),
            serde_json::json!({"safe": true}),
            cx,
        );
        cx.notify();
    }

    fn undo(&mut self, cx: &mut Context<Self>) {
        self.begin_action();
        let changed = self.view.update(cx, |view, cx| {
            let changed = view.editor.undo();
            cx.notify();
            changed
        });
        self.status = format!(
            "Configuration Undo: {changed}. Size/DPR changes reset the current path; history never replays runtime pixels or path commands."
        );
        self.record("undo", true, serde_json::json!({"changed": changed}), cx);
        cx.notify();
    }
}

impl Render for Lab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let config = ["one", "two"].map(|id| {
            let object = &self
                .view
                .read(cx)
                .editor
                .scene()
                .get(&id.into())
                .unwrap()
                .object;
            if let Shape::Canvas(canvas) = &object.shape {
                format!(
                    "{} x {} logical, DPR {}",
                    canvas.size.width, canvas.size.height, canvas.pixel_ratio
                )
            } else {
                String::new()
            }
        });
        div().size_full().flex().flex_col().bg(rgb(0x0d1421)).text_color(rgb(0xe6edf5))
            .child(div().h(px(48.)).px_4().flex().items_center().gap_2()
                .child(ControlButton::new("build", "Build outline").on_click(cx.listener(|s,_,_,cx| s.edit(Action::Build,"BeginPath + Q/C outline: geometry only; previous pixels stay.",cx))))
                .child(ControlButton::new("append", "Append hole").on_click(cx.listener(|s,_,_,cx| s.edit(Action::Append,"Append a hole across calls. Save/Restore does not remove its geometry.",cx))))
                .child(ControlButton::new("draw", "Fill + stroke").on_click(cx.listener(|s,_,_,cx| s.edit(Action::Draw,"Draw the retained path with fresh identity/opacity 1. Drawing keeps the path.",cx))))
                .child(ControlButton::new("clear", "Clear pixels").on_click(cx.listener(|s,_,_,cx| s.edit(Action::Clear,"Pixels cleared; the current path remains available for Fill + stroke.",cx))))
                .child(ControlButton::new("begin", "BeginPath").on_click(cx.listener(|s,_,_,cx| s.edit(Action::Begin,"BeginPath removes geometry only. Existing pixels remain; later Fill + stroke is empty.",cx))))
                .child(ControlButton::new("reject", "Reject late").on_click(cx.listener(|s,_,_,cx| s.edit(Action::Reject,"Late invalid Restore rolls back both replacement geometry and requested white fill.",cx)))))
            .child(div().h(px(48.)).px_4().flex().items_center().gap_2()
                .child(ControlButton::new("transform", "Paint transform").on_click(cx.listener(|s,_,_,cx| s.edit(Action::TransformDraw,"New draw transform moves the gradient and scales the pen, while the path stays in place.",cx))))
                .child(ControlButton::new("clip", "Clip + stripes").on_click(cx.listener(|s,_,_,cx| s.edit(Action::Clip,"Frozen current path clips stripes. Yellow corner follows Restore; orange corner is a new unclipped batch.",cx))))
                .child(ControlButton::new("safe", "Safe shrink/grow").on_click(cx.listener(|s,_,_,cx| s.resize(true,cx))))
                .child(ControlButton::new("reset", "Reset shrink/grow").on_click(cx.listener(|s,_,_,cx| s.resize(false,cx))))
                .child(ControlButton::new("dpr", "Swap DPR").on_click(cx.listener(|s,_,_,cx| s.swap_dpr(cx))))
                .child(ControlButton::new("undo", "Undo config").on_click(cx.listener(|s,_,_,cx| s.undo(cx))))
                .child(ControlButton::new("regenerate", "Regenerate").on_click(cx.listener(|s,_,_,cx| s.regenerate(cx)))))
            .child(div().flex_1().min_h_0().overflow_hidden().child(self.view.clone()))
            .child(div().h(px(156.)).px_4().text_sm()
                .child(format!("Left: {}  |  Right: {}\n", config[0], config[1]))
                .child("Try Clear pixels -> Fill + stroke; Clear pixels -> BeginPath -> Fill; Build -> Append -> Fill.\nPath-only edits retain snapshot identity and generation. Each surface is an independent transaction.\n")
                .child(self.status.clone()))
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
                        size(px(1090.), px(670.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                |window, cx| {
                    window.set_window_title("gpui-flow · Canvas current path");
                    cx.new(Lab::new)
                },
            )
            .unwrap();
            cx.activate(true);
        });
}
