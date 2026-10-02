//! Immutable Canvas pattern paints with actual controls at DPR 1 and 2.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, SceneView,
    composite::{
        CanvasCommand, CanvasPathTarget, CanvasPattern, CanvasPatternRepeat, CanvasStrokeOptions,
        CanvasSurface, CompositeError, ImageInputs, StaticImage,
    },
    scene::{
        Affine, Color, DisplayObject, FillRule, Point as ScenePoint, Scene, Shape,
        Size as SceneSize, Stroke, StrokeCap, StrokeJoin, Style, display,
    },
};
use std::sync::Arc;

fn path(svg: &str) -> display::Path {
    display::Path::from_svg(svg).unwrap()
}
fn point(x: f32, y: f32) -> ScenePoint {
    ScenePoint::new(x, y)
}

fn inputs(replacement: bool) -> ImageInputs {
    let mut rgba = Vec::new();
    for y in 0..12 {
        for x in 0..12 {
            // Already premultiplied: the lower-left quadrant is half opaque;
            // the lower-right is transparent, including zero RGB.
            let pixel = match (x < 6, y < 6, replacement) {
                (true, true, false) => [255, 103, 73, 255],
                (false, true, false) => [66, 220, 181, 255],
                (true, false, false) => [46, 75, 128, 128],
                (true, true, true) => [240, 199, 50, 255],
                (false, true, true) => [195, 95, 235, 255],
                (true, false, true) => [128, 45, 88, 128],
                _ => [0; 4],
            };
            rgba.extend_from_slice(&pixel);
        }
    }
    let tile = StaticImage::from_premultiplied_rgba(12, 12, rgba, Default::default()).unwrap();
    let mut images = ImageInputs::default();
    images.insert("tile".into(), Ok(Arc::new(tile)));
    images
}

#[derive(Clone)]
struct Patterns {
    repeat: CanvasPattern,
    once: CanvasPattern,
    reflected: CanvasPattern,
}
impl Patterns {
    fn capture(images: &ImageInputs) -> Result<Self, CompositeError> {
        // A factory surface is not the owner of the returned image snapshots.
        let factory = CanvasSurface::new(display::Canvas::new(1., 1.), Default::default())?;
        let mut repeat =
            factory.create_pattern(&"tile".into(), images, CanvasPatternRepeat::Repeat)?;
        repeat.set_transform(Affine::translation(0.35, 0.65) * Affine::scale(1.35, 1.35))?;
        let mut once =
            factory.create_pattern(&"tile".into(), images, CanvasPatternRepeat::NoRepeat)?;
        once.set_transform(Affine::translation(226., 30.) * Affine::scale(5., 5.))?;
        let mut reflected = repeat.clone();
        reflected.set_transform(
            Affine::translation(314., 153.) * Affine::rotation(18.) * Affine::scale(-2.3, 1.6),
        )?;
        Ok(Self {
            repeat,
            once,
            reflected,
        })
    }
}
fn fill(svg: &str, pattern: &CanvasPattern, rule: FillRule) -> CanvasCommand {
    CanvasCommand::FillPattern {
        path: CanvasPathTarget::Explicit(path(svg)),
        pattern: pattern.clone(),
        rule,
    }
}
fn solid(svg: &str, color: Color) -> CanvasCommand {
    CanvasCommand::FillPath {
        path: path(svg),
        paint: color.into(),
        rule: FillRule::NonZero,
    }
}
fn gallery(patterns: &Patterns) -> Vec<CanvasCommand> {
    vec![
        fill(
            "M 12 35 C 30 -5 165 0 185 38 L 176 117 Q 102 149 13 116 Z M 66 48 L 124 48 L 124 91 L 66 91 Z",
            &patterns.repeat,
            FillRule::EvenOdd,
        ),
        // Only one 60x60 tile appears inside this much larger path.
        fill(
            "M 206 10 L 312 10 L 312 132 L 206 132 Z",
            &patterns.once,
            FillRule::NonZero,
        ),
        CanvasCommand::StrokePath {
            path: path("M 206 10 L 312 10 L 312 132 L 206 132 Z"),
            stroke: Stroke::new(Color::hex(0x7c9ab9), 1.),
        },
        CanvasCommand::StrokePattern {
            path: CanvasPathTarget::Explicit(path(
                "M 14 167 C 42 130 74 209 110 170 S 167 137 185 186",
            )),
            pattern: patterns.repeat.clone(),
            options: CanvasStrokeOptions {
                width: 16.,
                cap: StrokeCap::Round,
                join: StrokeJoin::Round,
                dash_pattern: vec![18., 7., 5.],
                dash_offset: -5.,
                ..Default::default()
            },
        },
        fill(
            "M 211 151 L 286 145 L 315 181 L 290 230 L 221 225 L 202 192 Z",
            &patterns.reflected,
            FillRule::NonZero,
        ),
    ]
}
fn build_current() -> Vec<CanvasCommand> {
    vec![
        CanvasCommand::BeginPath,
        CanvasCommand::SetTransform(Affine::translation(12., 0.)),
        CanvasCommand::MoveTo {
            to: point(6., 220.),
        },
        CanvasCommand::QuadraticCurveTo {
            control: point(81., 202.),
            to: point(173., 221.),
        },
        CanvasCommand::LineTo {
            to: point(159., 247.),
        },
        CanvasCommand::LineTo {
            to: point(13., 244.),
        },
        CanvasCommand::ClosePath,
        // This batch's transform/opacity must not leak into the next draw.
        CanvasCommand::SetOpacity(0.13),
    ]
}
fn draw_current(patterns: &Patterns, transformed: bool) -> Vec<CanvasCommand> {
    vec![
        CanvasCommand::SetTransform(if transformed {
            Affine::translation(49., 3.) * Affine::scale(1.4, 0.8)
        } else {
            Affine::IDENTITY
        }),
        CanvasCommand::FillPattern {
            path: CanvasPathTarget::Current,
            pattern: patterns.repeat.clone(),
            rule: FillRule::NonZero,
        },
        CanvasCommand::StrokePattern {
            path: CanvasPathTarget::Current,
            pattern: patterns.reflected.clone(),
            options: CanvasStrokeOptions {
                width: 7.,
                cap: StrokeCap::Round,
                join: StrokeJoin::Bevel,
                dash_pattern: vec![12., 5.],
                dash_offset: 3.,
                ..Default::default()
            },
        },
        // A thin solid silhouette makes the frozen geometry easy to compare
        // while the current-path pattern stroke changes under the draw CTM.
        CanvasCommand::StrokeCurrentPath {
            stroke: Stroke::new(Color::hex(0xf6ecd0), 1.),
        },
    ]
}
fn clipping(patterns: &Patterns) -> Vec<CanvasCommand> {
    vec![
        CanvasCommand::Save,
        CanvasCommand::ClipPath {
            path: path(
                "M 16 124 C 16 4 303 4 303 124 C 303 252 16 252 16 124 Z M 119 83 L 204 83 L 204 168 L 119 168 Z",
            ),
            rule: FillRule::EvenOdd,
        },
        CanvasCommand::SetOpacity(0.72),
        CanvasCommand::Save,
        CanvasCommand::SetTransform(Affine::translation(34., 0.)),
        CanvasCommand::ClipPath {
            path: path("M 0 0 L 160 0 Q 108 118 166 260 L 0 260 Z"),
            rule: FillRule::NonZero,
        },
        // The second clip was frozen under translation. Resetting the CTM
        // changes the pattern placement, never either nested clip geometry.
        CanvasCommand::SetTransform(Affine::IDENTITY),
        fill(
            "M 0 0 L 320 0 L 320 260 L 0 260 Z",
            &patterns.reflected,
            FillRule::NonZero,
        ),
        CanvasCommand::Restore,
        // This right-side stripe is beyond the inner clip, but its far edge
        // still meets the outer ellipse. Only the inner clip was restored.
        fill(
            "M 220 111 L 320 111 L 320 137 L 220 137 Z",
            &patterns.repeat,
            FillRule::NonZero,
        ),
        CanvasCommand::Restore,
        // Full opacity and no clip after the outer Restore, outside the ellipse.
        fill(
            "M 2 2 L 26 2 L 26 26 L 2 26 Z",
            &patterns.repeat,
            FillRule::NonZero,
        ),
    ]
}
#[derive(Clone, Copy, Debug)]
enum Action {
    Gallery,
    Build,
    DrawCurrent,
    TransformCurrent,
    Clip,
    Reject,
    Clear,
    Begin,
}
impl Action {
    fn name(self) -> &'static str {
        match self {
            Self::Gallery => "gallery",
            Self::Build => "build_path",
            Self::DrawCurrent => "draw_current",
            Self::TransformCurrent => "transform_current",
            Self::Clip => "clip_restore",
            Self::Reject => "reject_late",
            Self::Clear => "clear",
            Self::Begin => "begin_path",
        }
    }
}
fn apply(
    surface: &mut CanvasSurface,
    patterns: &Patterns,
    action: Action,
) -> Result<serde_json::Value, CompositeError> {
    let generation = surface.generation();
    let pixels = surface.snapshot();
    let mut path_replay_unchanged = None;
    let published = match action {
        Action::Gallery => {
            surface.clear()?;
            surface.draw_commands(&build_current())?;
            surface.draw_commands(&gallery(patterns))?;
            surface.draw_commands(&draw_current(patterns, false))?;
            true
        }
        Action::Build => surface.draw_commands(&build_current())?,
        Action::DrawCurrent => surface.draw_commands(&draw_current(patterns, false))?,
        Action::TransformCurrent => surface.draw_commands(&draw_current(patterns, true))?,
        Action::Clip => {
            surface.clear()?;
            surface.draw_commands(&clipping(patterns))?;
            // The next batch independently starts without the prior clip.
            surface.draw_commands(&[solid(
                "M 300 240 L 316 240 L 316 256 L 300 256 Z",
                Color::hex(0xffc75d),
            )])?;
            true
        }
        Action::Clear => {
            surface.clear()?;
            true
        }
        Action::Begin => surface.draw_commands(&[CanvasCommand::BeginPath])?,
        Action::Reject => {
            let mut expected = surface.clone();
            let result = surface.draw_commands(&[
                CanvasCommand::BeginPath,
                CanvasCommand::MoveTo { to: point(5., 5.) },
                CanvasCommand::LineTo {
                    to: point(312., 5.),
                },
                CanvasCommand::LineTo {
                    to: point(312., 253.),
                },
                CanvasCommand::ClosePath,
                CanvasCommand::FillPattern {
                    path: CanvasPathTarget::Current,
                    pattern: patterns.repeat.clone(),
                    rule: FillRule::NonZero,
                },
                CanvasCommand::Restore,
            ]);
            let mut actual = surface.clone();
            expected.clear()?;
            actual.clear()?;
            expected.draw_commands(&draw_current(patterns, false))?;
            actual.draw_commands(&draw_current(patterns, false))?;
            let same_path =
                expected.snapshot().premultiplied_rgba() == actual.snapshot().premultiplied_rgba();
            if result.is_ok()
                || generation != surface.generation()
                || !Arc::ptr_eq(&pixels, &surface.snapshot())
                || !same_path
            {
                return Err(CompositeError::InvalidGeometry(
                    "late pattern rejection failed rollback checks".into(),
                ));
            }
            path_replay_unchanged = Some(same_path);
            false
        }
    };
    Ok(serde_json::json!({
        "published": published, "generation_before": generation, "generation_after": surface.generation(),
        "same_snapshot": Arc::ptr_eq(&pixels, &surface.snapshot()),
        "expected_batch_rejection": matches!(action, Action::Reject),
        "path_replay_unchanged": path_replay_unchanged,
        "current_path_bytes": surface.current_path_byte_len(),
    }))
}
fn document() -> Scene {
    let mut scene = Scene::new();
    for (id, x, dpr, label) in [
        ("one", 32., 1., "Pattern paints · starts at DPR 1"),
        ("two", 407., 2., "Pattern paints · starts at DPR 2"),
    ] {
        let mut canvas = display::Canvas::new(320., 260.).rounded(10.);
        canvas.pixel_ratio = dpr;
        canvas.safe_resize = true;
        scene
            .insert(
                DisplayObject::new(id, canvas)
                    .with_transform(Affine::translation(x, 54.))
                    .with_style(Style {
                        fill: Some(Color::hex(0x182b42).into()),
                        stroke: Some(Stroke::new(Color::hex(0x439cc3), 2.)),
                        ..Default::default()
                    }),
                None,
                None,
            )
            .unwrap();
        let mut label_text = display::Text::new(label, SceneSize::new(360., 30.));
        label_text.font_size = 16.;
        scene
            .insert(
                DisplayObject::new(format!("label-{id}"), label_text)
                    .with_transform(Affine::translation(x, 14.))
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
    images: ImageInputs,
    patterns: Patterns,
    source_replaced: bool,
    captured_replacement: bool,
    status: String,
    button_action_count: u64,
    action_origin: &'static str,
}
impl Lab {
    fn new(cx: &mut Context<Self>) -> Self {
        let view = cx.new(|cx| {
            let mut view = SceneView::new(document(), cx).unwrap();
            view.show_grid = false;
            view.set_editable(false, cx);
            view
        });
        let images = inputs(false);
        let patterns = Patterns::capture(&images).unwrap();
        let mut lab = Self {
            view,
            images,
            patterns,
            source_replaced: false,
            captured_replacement: false,
            status: String::new(),
            button_action_count: 0,
            action_origin: "startup",
        };
        lab.edit(
            Action::Gallery,
            "Repeat + hole; one tile; dashed curve; reflected fill; frozen bottom path.",
            cx,
        );
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
                Shape::Canvas(config) => serde_json::json!({"id": id, "config": config}),
                _ => serde_json::json!({"id": id, "config": null}),
            }
        });
        eprintln!(
            "{}",
            serde_json::json!({
                "fixture": "canvas_patterns", "action": action, "origin": self.action_origin,
                "button_action_count": self.button_action_count, "accepted": accepted,
                "revision": view.editor.engine.revision(), "canvases": configs,
                "source_replaced": self.source_replaced, "captured_replacement": self.captured_replacement,
                "status": self.status, "detail": detail,
            })
        );
    }
    fn edit(&mut self, action: Action, description: &str, cx: &mut Context<Self>) {
        self.begin_action();
        let patterns = self.patterns.clone();
        let (reports, outcomes, accepted) = self.view.update(cx, |view, cx| {
            let mut reports = Vec::new();
            let mut outcomes = Vec::new();
            let mut accepted = true;
            for (id, label) in [("one", "Left"), ("two", "Right")] {
                let handle = view.editor.engine.handle(&id.into()).unwrap();
                let mut report = serde_json::Value::Null;
                let result = view.edit_canvas(handle, |surface| {
                    report = apply(surface, &patterns, action)?;
                    Ok(())
                }, cx);
                accepted &= result.is_ok();
                let status = match &result {
                    Ok(()) => format!("{label}: generation {} -> {}; same snapshot {}; path bytes {}{}",
                        report["generation_before"], report["generation_after"], report["same_snapshot"], report["current_path_bytes"],
                        if matches!(action, Action::Reject) { "; late rejection preserved pixels and path" } else { "" }),
                    Err(error) => format!("{label}: failed: {error}"),
                };
                outcomes.push(serde_json::json!({"id": id, "accepted": result.is_ok(), "detail": report, "status": status}));
                reports.push(status);
            }
            (reports.join("\n"), outcomes, accepted)
        });
        self.status = format!("{description}\n{reports}");
        self.record(
            action.name(),
            accepted,
            serde_json::json!({"surfaces": outcomes}),
            cx,
        );
        cx.notify();
    }
    fn source(&mut self, recapture: bool, cx: &mut Context<Self>) {
        self.begin_action();
        if recapture {
            match Patterns::capture(&self.images) {
                Ok(patterns) => {
                    self.patterns = patterns;
                    self.captured_replacement = self.source_replaced;
                }
                Err(error) => {
                    self.status = format!("Pattern capture failed: {error}");
                    self.record("recapture_source", false, serde_json::json!({}), cx);
                    cx.notify();
                    return;
                }
            }
        } else {
            self.source_replaced = !self.source_replaced;
            self.images = inputs(self.source_replaced);
        }
        let origin = self.action_origin;
        self.action_origin = "source_substep";
        self.edit(Action::Gallery, if recapture {
            "New handles capture the current source. Previously created handles stay immutable."
        } else {
            "Source ID replaced. Gallery still uses captured pixels; Recapture source updates the handles."
        }, cx);
        self.action_origin = origin;
        self.record(
            if recapture {
                "recapture_source"
            } else {
                "replace_source"
            },
            true,
            serde_json::json!({}),
            cx,
        );
    }
    fn config(&mut self, action: &str, cx: &mut Context<Self>) {
        self.begin_action();
        let result = self.view.update(cx, |view, cx| {
            let result = view.editor.transaction(|scene| {
                for id in ["one", "two"] {
                    scene.update_object(&id.into(), |object| {
                        if let Shape::Canvas(config) = &mut object.shape {
                            match action {
                                "smooth" => config.smooth = !config.smooth,
                                "dpr" => {
                                    config.safe_resize = true;
                                    config.pixel_ratio =
                                        if config.pixel_ratio == 1. { 2. } else { 1. };
                                }
                                _ => {
                                    config.safe_resize = action == "safe_resize";
                                    config.size.width =
                                        if config.size.width > 200. { 180. } else { 320. };
                                }
                            }
                        }
                    })?;
                }
                Ok(())
            });
            cx.notify();
            result
        });
        self.status = match &result {
            Err(error) => format!("Configuration failed: {error}"),
            Ok(_) => match action {
                "smooth" => "Smoothing toggled. Regenerate to compare nearest/bilinear pattern edges; pixels/path remain until redrawn.",
                "dpr" => "Safe DPR swap copies physical pixels without scaling and resets the current path. Patterns survive; Regenerate redraws at the new DPR.",
                "safe_resize" => "Safe resize preserves only the physical intersection and resets the path. Growing cannot restore cropped pixels. Patterns survive.",
                _ => "Reset resize clears pixels and current geometry. Patterns survive outside the Canvas; Undo restores configuration only.",
            }.into(),
        };
        self.record(action, result.is_ok(), serde_json::json!({}), cx);
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
            "Configuration Undo: {changed}. Pixel edits and captured patterns never enter document history; resize/DPR boundaries reset geometry."
        );
        self.record("undo", true, serde_json::json!({"changed": changed}), cx);
        cx.notify();
    }
}
impl Render for Lab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let config = ["one", "two"].map(|id| {
            match &self
                .view
                .read(cx)
                .editor
                .scene()
                .get(&id.into())
                .unwrap()
                .object
                .shape
            {
                Shape::Canvas(config) => format!(
                    "{} x {}, DPR {}, {}",
                    config.size.width,
                    config.size.height,
                    config.pixel_ratio,
                    if config.smooth { "bilinear" } else { "nearest" }
                ),
                _ => String::new(),
            }
        });
        div().size_full().flex().flex_col().bg(rgb(0x0d1421)).text_color(rgb(0xe6edf5))
            .child(div().h(px(46.)).flex_shrink_0().px_4().flex().items_center().gap_2()
                .child(ControlButton::new("gallery", "Regenerate").on_click(cx.listener(|s,_,_,cx| s.edit(Action::Gallery,"Gallery regenerated with the captured pattern handles.",cx))))
                .child(ControlButton::new("build", "Build frozen path").on_click(cx.listener(|s,_,_,cx| s.edit(Action::Build,"Bottom geometry captured under translation; this path-only edit keeps existing pixels.",cx))))
                .child(ControlButton::new("current", "Paint current").on_click(cx.listener(|s,_,_,cx| s.edit(Action::DrawCurrent,"Fresh identity paints the retained path, without consuming it.",cx))))
                .child(ControlButton::new("transform", "Paint CTM").on_click(cx.listener(|s,_,_,cx| s.edit(Action::TransformCurrent,"Draw-time CTM moves the tile and scales the pen; frozen bottom geometry stays in place.",cx))))
                .child(ControlButton::new("clip", "Clip + restore").on_click(cx.listener(|s,_,_,cx| s.edit(Action::Clip,"Nested frozen clips: right stripe follows inner Restore; upper tile follows outer Restore; yellow square is a fresh batch.",cx))))
                .child(ControlButton::new("reject", "Reject late").on_click(cx.listener(|s,_,_,cx| s.edit(Action::Reject,"Invalid Restore follows replacement geometry and pattern fill; all pixels/path changes must roll back.",cx)))))
            .child(div().h(px(46.)).flex_shrink_0().px_4().flex().items_center().gap_2()
                .child(ControlButton::new("replace", "Replace source").on_click(cx.listener(|s,_,_,cx| s.source(false,cx))))
                .child(ControlButton::new("capture", "Recapture source").on_click(cx.listener(|s,_,_,cx| s.source(true,cx))))
                .child(ControlButton::new("smooth", "Toggle filter").on_click(cx.listener(|s,_,_,cx| s.config("smooth",cx))))
                .child(ControlButton::new("clear", "Clear pixels").on_click(cx.listener(|s,_,_,cx| s.edit(Action::Clear,"Pixels cleared; the retained path and immutable pattern handles survive.",cx))))
                .child(ControlButton::new("begin", "BeginPath").on_click(cx.listener(|s,_,_,cx| s.edit(Action::Begin,"Geometry cleared; existing pixels stay. Paint current now publishes no new snapshot.",cx)))))
            .child(div().h(px(46.)).flex_shrink_0().px_4().flex().items_center().gap_2()
                .child(ControlButton::new("safe", "Safe shrink/grow").on_click(cx.listener(|s,_,_,cx| s.config("safe_resize",cx))))
                .child(ControlButton::new("reset", "Reset shrink/grow").on_click(cx.listener(|s,_,_,cx| s.config("reset_resize",cx))))
                .child(ControlButton::new("dpr", "Swap DPR").on_click(cx.listener(|s,_,_,cx| s.config("dpr",cx))))
                .child(ControlButton::new("undo", "Undo config").on_click(cx.listener(|s,_,_,cx| s.undo(cx)))))
            .child(div().flex_1().min_h_0().overflow_hidden().child(self.view.clone()))
            .child(div().flex().flex_col().flex_shrink_0().gap_1().px_4().py_2().text_sm()
                .child(div().child(format!("Left: {} | Right: {}", config[0], config[1])))
                .child(div().child("Upper-left: repeat + even-odd hole. Upper-right: one tile. Middle: dashed open curve + reflected fill. Bottom: frozen current pattern fill + stroke."))
                .child(div().child("Source tile has opaque, translucent and transparent quadrants. Replace source -> unchanged; Recapture source -> new colors."))
                .child(div().child("Try Clear -> Paint current; Paint CTM; Reject late; Toggle filter -> Regenerate; resize/DPR -> Paint current -> Undo -> Regenerate."))
                .children(self.status.lines().map(|line| div().child(line.to_owned()))))
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
                        size(px(1230.), px(800.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                |window, cx| {
                    window.set_window_title("gpui-flow · Canvas patterns");
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
    fn pattern_controls_accept_the_dpr_filter_source_and_geometry_tour() {
        for dpr in [1., 2.] {
            for smooth in [false, true] {
                let mut config = display::Canvas::new(320., 260.);
                config.pixel_ratio = dpr;
                config.smooth = smooth;
                let mut surface = CanvasSurface::new(config.clone(), Default::default()).unwrap();
                let original = inputs(false);
                let captured = Patterns::capture(&original).unwrap();
                assert_eq!(captured.repeat.repetition(), CanvasPatternRepeat::Repeat);
                assert_eq!(captured.once.repetition(), CanvasPatternRepeat::NoRepeat);
                assert!(captured.reflected.transform().a < 0.);
                for action in [
                    Action::Gallery,
                    Action::Build,
                    Action::DrawCurrent,
                    Action::TransformCurrent,
                    Action::Clip,
                    Action::Reject,
                    Action::Clear,
                    Action::DrawCurrent,
                    Action::Begin,
                    Action::DrawCurrent,
                    Action::Build,
                    Action::DrawCurrent,
                    Action::Reject,
                    Action::Gallery,
                ] {
                    let report = apply(&mut surface, &captured, action).unwrap_or_else(|error| {
                        panic!("DPR {dpr}, smooth {smooth}, {action:?}: {error}")
                    });
                    if matches!(action, Action::Build | Action::Begin | Action::Reject) {
                        assert_eq!(report["same_snapshot"], true);
                    }
                    if matches!(action, Action::Reject) {
                        assert_eq!(report["path_replay_unchanged"], true);
                    }
                }
                let old_pixels = surface.snapshot();
                let replacement = inputs(true);
                // Replacing the inputs and dropping the original factory/source
                // cannot mutate a handle already captured by the UI.
                drop(original);
                apply(&mut surface, &captured, Action::Gallery).unwrap();
                assert_eq!(
                    surface.snapshot().premultiplied_rgba(),
                    old_pixels.premultiplied_rgba()
                );
                let recaptured = Patterns::capture(&replacement).unwrap();
                apply(&mut surface, &recaptured, Action::Gallery).unwrap();
                assert_ne!(
                    surface.snapshot().premultiplied_rgba(),
                    old_pixels.premultiplied_rgba()
                );
                for safe in [false, true] {
                    config.safe_resize = safe;
                    config.size.width = 180.;
                    surface.reconfigure(config.clone()).unwrap();
                    assert_eq!(surface.current_path_byte_len(), 0);
                    assert_eq!(
                        apply(&mut surface, &captured, Action::DrawCurrent).unwrap()["published"],
                        false
                    );
                    apply(&mut surface, &captured, Action::Gallery).unwrap();
                    config.size.width = 320.;
                    config.pixel_ratio = if config.pixel_ratio == 1. { 2. } else { 1. };
                    surface.reconfigure(config.clone()).unwrap();
                    assert_eq!(surface.current_path_byte_len(), 0);
                    apply(&mut surface, &captured, Action::Gallery).unwrap();
                }
                let mut editor = gpui_flow::editor::SceneEditor::new(document()).unwrap();
                let handle = editor.engine.handle(&"one".into()).unwrap();
                let before = editor.scene().clone();
                editor
                    .set_property(
                        handle,
                        gpui_flow::engine::PropertyKey::Shape,
                        serde_json::json!(Shape::from(display::Canvas::new(180., 260.))),
                    )
                    .unwrap();
                assert!(editor.undo());
                assert_eq!(editor.scene(), &before);
                assert!(editor.redo());
            }
        }
    }
}
