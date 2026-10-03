//! Native SceneView acceptance fixture for the bounded scene-eraser increment.
//! CPU export files are independent export results, never native framebuffer captures.
//! No remote resources, font bridge, GPU readback, new dependencies, or auto-upload.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, SceneCompositing, SceneInputOperation, SceneView, SceneViewEvent,
    composite::{
        CompositeError, CompositeLimits, ImageInputs, SceneExport, SceneExportFormat,
        SceneExportPlan, SceneExportPreparation, SceneExportRequest, TextInputs,
    },
    engine::SceneEngine,
    scene::{
        Affine, BlendMode, Bounds as SceneBounds, Color, DisplayObject, EraserMode, EventKind,
        HitPolicy, HitTestOptions, MaskMode, Point as ScenePoint, Scene, SceneError,
        Size as SceneSize, Stroke, Style, display,
    },
};
use serde_json::{Value, json};
use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
    time::{SystemTime, UNIX_EPOCH},
};

const WIDTH: f32 = 1000.;
const HEIGHT: f32 = 430.;
const BACKGROUND: Color = Color::rgb(243, 221, 148);
const RED: Color = Color::rgb(255, 0, 0);
const BLUE: Color = Color::rgb(0, 0, 255);
const GREEN: Color = Color::rgb(0, 220, 90);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Case {
    Order,
    Isolation,
    Masks,
    Root,
}
impl Case {
    fn label(self) -> &'static str {
        match self {
            Self::Order => "A: order + transformed clip",
            Self::Isolation => "B: hidden-marker isolation",
            Self::Masks => "C: mask object opacity",
            Self::Root => "D: owned root + rejection",
        }
    }
    fn erasers(self) -> &'static [&'static str] {
        match self {
            Self::Order => &["cut", "clip-cut"],
            Self::Isolation => &["marker"],
            Self::Masks => &["full-cut", "half-cut", "paint-cut"],
            Self::Root => &["root-cut"],
        }
    }
    fn probes(self) -> &'static [(&'static str, f32, f32)] {
        match self {
            Self::Order => &[
                ("hole", 150., 140.),
                ("later", 200., 190.),
                ("outside-parent", 410., 150.),
                ("clip-interior", 715., 235.),
                ("clip-outside", 530., 345.),
            ],
            Self::Isolation => &[("test", 250., 200.), ("control", 730., 200.)],
            Self::Masks => &[
                ("mask-1", 155., 200.),
                ("mask-half", 480., 200.),
                ("paint-half", 805., 200.),
                ("later", 155., 305.),
            ],
            Self::Root => &[
                ("hole", 250., 160.),
                ("later", 330., 210.),
                ("positive", 130., 150.),
                ("background", 40., 40.),
            ],
        }
    }
}
fn add(s: &mut Scene, object: DisplayObject, parent: Option<&str>) {
    s.insert(object, parent.map(Into::into), None).unwrap();
}
fn rect(id: &str, x: f32, y: f32, w: f32, h: f32, color: Color) -> DisplayObject {
    DisplayObject::new(id, display::Rect::new(w, h))
        .with_transform(Affine::translation(x, y))
        .with_style(Style::fill(color))
}
fn group(id: &str, x: f32, y: f32) -> DisplayObject {
    let mut object =
        DisplayObject::new(id, display::Group::new()).with_transform(Affine::translation(x, y));
    object.hit_policy = HitPolicy::None;
    object
}
fn cut(mut object: DisplayObject) -> DisplayObject {
    object.eraser = EraserMode::Pixel;
    // Picking is geometric even when the Path control removes all paint.
    object.hit_policy = HitPolicy::Geometry;
    object
}
fn document(case: Case) -> Scene {
    let mut s = Scene::new();
    match case {
        Case::Order => {
            add(&mut s, rect("external", 30., 60., 410., 280., BLUE), None);
            add(&mut s, group("parent", 60., 80.), None);
            add(
                &mut s,
                rect("earlier", 0., 0., 280., 200., RED),
                Some("parent"),
            );
            let mut e = cut(DisplayObject::new("cut", display::Ellipse::new(140., 140.))
                .with_transform(Affine::translation(60., 20.))
                .with_style(Style::fill(Color::WHITE)));
            e.style.stroke = Some(Stroke::new(Color::WHITE, 18.));
            add(&mut s, e, Some("parent"));
            add(
                &mut s,
                rect("later", 110., 80., 60., 50., GREEN),
                Some("parent"),
            );
            // The eraser's own geometry stays outside its parent's aggregate bounds.
            add(
                &mut s,
                cut(rect("reach", 330., 50., 40., 40., Color::WHITE)),
                Some("parent"),
            );
            add(
                &mut s,
                rect("clip-external", 510., 50., 420., 310., BLUE),
                None,
            );
            add(
                &mut s,
                DisplayObject::new("clip", display::Frame::new(280., 220.).rounded(48.))
                    .with_transform(Affine::translation(570., 85.) * Affine::rotation(8.))
                    .with_style(Style::fill(RED)),
                None,
            );
            add(
                &mut s,
                cut(
                    rect("clip-cut", 0., 0., 340., 140., Color::WHITE).with_transform(
                        Affine::translation(10., 90.)
                            * Affine::rotation(-22.)
                            * Affine::skew(16., 0.),
                    ),
                ),
                Some("clip"),
            );
        }
        Case::Isolation => {
            for (prefix, x, isolated) in [("test", 80., false), ("control", 560., true)] {
                add(
                    &mut s,
                    rect(&format!("{prefix}-external"), x, 90., 340., 230., RED),
                    None,
                );
                let mut p = group(prefix, x, 90.);
                if isolated {
                    p.style.blend_mode = BlendMode::Normal;
                }
                add(&mut s, p, None);
                let mut blue = rect(&format!("{prefix}-blue"), 0., 0., 340., 230., BLUE);
                blue.style.blend_mode = BlendMode::Multiply;
                add(&mut s, blue, Some(prefix));
            }
            let mut marker = cut(rect("marker", 95., 65., 140., 100., Color::WHITE));
            marker.visible = false;
            add(&mut s, marker, Some("test"));
        }
        Case::Masks => {
            for (prefix, x, opacity, alpha) in [
                ("full", 30., 1., 255),
                ("half", 355., 0.5, 255),
                ("paint", 680., 1., 128),
            ] {
                add(
                    &mut s,
                    rect(&format!("{prefix}-external"), x, 70., 270., 290., BLUE),
                    None,
                );
                add(&mut s, group(prefix, x, 90.), None);
                let mut mask =
                    DisplayObject::new(format!("{prefix}-mask"), display::Ellipse::new(250., 250.))
                        .with_mask(MaskMode::Alpha)
                        .with_style(Style::fill(Color::rgba(255, 255, 255, alpha)));
                mask.style.opacity = opacity;
                mask.hit_policy = HitPolicy::None;
                add(&mut s, mask, Some(prefix));
                add(
                    &mut s,
                    rect(&format!("{prefix}-earlier"), 0., 0., 250., 250., RED),
                    Some(prefix),
                );
                add(
                    &mut s,
                    cut(rect(
                        &format!("{prefix}-cut"),
                        75.,
                        60.,
                        100.,
                        150.,
                        Color::WHITE,
                    )),
                    Some(prefix),
                );
                add(
                    &mut s,
                    rect(&format!("{prefix}-later"), 95., 195., 60., 35., GREEN),
                    Some(prefix),
                );
            }
        }
        Case::Root => {
            add(
                &mut s,
                rect("root-earlier", 80., 70., 700., 260., RED),
                None,
            );
            add(
                &mut s,
                cut(
                    DisplayObject::new("root-cut", display::Ellipse::new(220., 220.))
                        .with_transform(Affine::translation(200., 80.))
                        .with_style(Style::fill(Color::WHITE)),
                ),
                None,
            );
            add(
                &mut s,
                rect("root-later", 280., 150., 120., 120., GREEN),
                None,
            );
        }
    }
    s.validate().unwrap();
    s
}
fn fingerprint(bytes: &[u8]) -> String {
    let hash = bytes.iter().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    });
    format!("{hash:016x}")
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.write_all(bytes).map_err(|e| e.to_string())
}
fn request(format: SceneExportFormat) -> SceneExportRequest {
    SceneExportRequest {
        world_bounds: SceneBounds::new(ScenePoint::ZERO, SceneSize::new(WIDTH, HEIGHT)),
        scale: 1.,
        pixel_ratio: 1.,
        background: BACKGROUND,
        format,
    }
}
fn hit_ids(s: &Scene, point: ScenePoint) -> Value {
    match s.hit_test_all(point, HitTestOptions::default()) {
        Ok(hits) => json!(
            hits.iter()
                .map(|hit| hit.id.to_string())
                .collect::<Vec<_>>()
        ),
        Err(error) => json!({"query_error": error.to_string()}),
    }
}
fn expects_external_error(view: &SceneView, case: Case) -> bool {
    case == Case::Root
        && view.compositing_mode == SceneCompositing::Domains
        && view
            .editor
            .scene()
            .get(&"root-cut".into())
            .is_some_and(|node| {
                node.object.eraser != EraserMode::None
                    && node.object.visible
                    && node.object.style.opacity > 0.
            })
}
fn snapshot(view: &SceneView, case: Case) -> Value {
    let canonical = view.editor.scene();
    let presented = view.presented_geometry();
    let probes: Vec<Value> = case
        .probes()
        .iter()
        .map(|(name, x, y)| {
            let point = ScenePoint::new(*x, *y);
            let engine = match view
                .editor
                .engine
                .hit_test_all(point, HitTestOptions::default())
            {
                Ok(hits) => json!(hits.iter().map(|h| h.id.to_string()).collect::<Vec<_>>()),
                Err(error) => json!({"query_error":error.to_string()}),
            };
            json!({"name":name,"world":[x,y],"canonical_scene_ids":hit_ids(canonical,point),
            "canonical_engine_ids":engine,"presented_scene_ids":presented.map(|s|hit_ids(s,point))})
        })
        .collect();
    let canonical_json = canonical.to_json().unwrap();
    let presented_json = presented.map(|s| s.to_json().unwrap());
    let expected_error = expects_external_error(view, case);
    json!({"evidence_kind":"public live SceneView state; no GPU framebuffer readback",
        "case":case.label(),"readonly":!view.is_editable(),"revision":view.editor.engine.revision(),
        "mode":format!("{:?}",view.compositing_mode),"pending":view.compositing_pending,
        "composite_issues":view.composite_diagnostics.iter().map(|issue|json!({
            "object":issue.object,"typed_error":format!("{:?}",issue.error)})).collect::<Vec<_>>(),
        "render_issues":format!("{:?}",view.diagnostics),
        "expected_typed_error":if expected_error {Some("UnsupportedExternalBackdrop")}else{None},
        "expected_diagnostics_observed":if expected_error {
            view.composite_diagnostics.iter().any(|issue|issue.error==CompositeError::UnsupportedExternalBackdrop)
        }else{view.composite_diagnostics.is_empty()},
        "presented_geometry_available":presented.is_some(),
        "presented_equals_canonical":presented_json.as_ref().map(|s|s==&canonical_json),
        "canonical_json_fnv1a64":fingerprint(canonical_json.as_bytes()),
        "presented_json_fnv1a64":presented_json.as_ref().map(|s|fingerprint(s.as_bytes())),
        "erasers":case.erasers().iter().map(|id|&canonical.get(&(*id).into()).unwrap().object).collect::<Vec<_>>(),
        "bounds":if case==Case::Order {json!({"parent":view.editor.engine.world_bounds(&"parent".into()).unwrap(),
            "reach":view.editor.engine.world_bounds(&"reach".into()).unwrap()})}else{Value::Null},
        "geometry_probes":probes,"viewport":view.editor.viewport})
}

struct Lab {
    view: Entity<SceneView>,
    case: Case,
    status: String,
    last_pointer: String,
    session: PathBuf,
    sequence: u32,
    baseline: Vec<String>,
    native_inputs: Vec<Value>,
    burst_left: u8,
    burst_queued: bool,
    burst_token: u64,
    _observe: Subscription,
    _input: Subscription,
}
impl Lab {
    fn new_view(case: Case, cx: &mut Context<Self>) -> Result<Entity<SceneView>, SceneError> {
        let engine = SceneEngine::new(document(case))?;
        Ok(cx.new(|cx| {
            let mut view = SceneView::from_engine(engine, cx);
            view.compositing_mode = SceneCompositing::OwnedCanvas;
            view.theme.background = 0xf3dd94;
            view.theme.grid = 0x748398;
            view.show_grid = true;
            view.set_editable(false, cx);
            view.set_input_tracing(true);
            view
        }))
    }
    fn subscriptions(
        view: &Entity<SceneView>,
        cx: &mut Context<Self>,
    ) -> (Subscription, Subscription) {
        let observe = cx.observe(view, |_, _, cx| cx.notify());
        let input = cx.subscribe(view, |this: &mut Self, entity, event, cx| {
            if let SceneViewEvent::InputTrace(trace) = event
                && let SceneInputOperation::Canvas(gpui_flow::interaction::CanvasEvent::PointerDown(pointer)) = &trace.operation {
                    let view = entity.read(cx);
                    let world = view.editor.viewport.screen_to_world(pointer.position);
                    let targets = trace.events.iter().filter(|e|e.kind==EventKind::PointerDown)
                        .map(|e|e.target.to_string()).collect::<Vec<_>>();
                    this.last_pointer = format!("({:.1},{:.1}) routed={:?}, presented IDs={}",world.x,world.y,targets,
                        view.presented_geometry().map(|s|hit_ids(s,world)).unwrap_or(Value::Null));
                    let event = json!({"kind":"native_pointer_down","world":world,"targets":targets,
                        "presented_hit_ids":view.presented_geometry().map(|s|hit_ids(s,world)),
                        "canonical_hit_ids":hit_ids(view.editor.scene(),world),"router_errors":trace.errors});
                    println!("{event}");
                    if this.native_inputs.len()==64 { this.native_inputs.remove(0); }
                    this.native_inputs.push(event); cx.notify();
            }
        });
        (observe, input)
    }
    fn new(cx: &mut Context<Self>) -> Self {
        let view = Self::new_view(Case::Order, cx).expect("validated initial fixture scene");
        let (observe, input) = Self::subscriptions(&view, cx);
        let session = evidence_root().join(format!(
            "gpui-scene-erasers-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let status = match std::fs::create_dir(&session) {
            Ok(()) => format!("Receipts: {}", session.display()),
            Err(error) => format!("Receipt directory failed: {error}; live fixture remains usable"),
        };
        println!(
            "{}",
            json!({"fixture":"scene-erasers","session":session,"state":"native app initialized; pixels still require observation"})
        );
        Self {
            view,
            case: Case::Order,
            status,
            last_pointer: "no native click yet".into(),
            session,
            sequence: 0,
            baseline: vec![],
            native_inputs: vec![],
            burst_left: 0,
            burst_queued: false,
            burst_token: 0,
            _observe: observe,
            _input: input,
        }
    }
    fn preset(&mut self, case: Case, cx: &mut Context<Self>) {
        self.burst_token += 1;
        self.burst_left = 0;
        self.burst_queued = false;
        // Presets start a fresh host-owned session. LoadScene on the existing
        // readonly session must continue to return CapabilityDenied(Edit).
        let old_view = self.view.entity_id();
        let view = match Self::new_view(case, cx) {
            Ok(view) => view,
            Err(error) => {
                self.status = format!("Preset creation failed: {error}");
                println!(
                    "{}",
                    json!({"kind":"preset_reset_failed","case":case.label(),"error":error.to_string()})
                );
                cx.notify();
                return;
            }
        };
        let (observe, input) = Self::subscriptions(&view, cx);
        self._observe = observe;
        self._input = input;
        self.view = view;
        println!(
            "{}",
            json!({"kind":"preset_reset","case":case.label(),
            "old_view":format!("{old_view:?}"),"new_view":format!("{:?}",self.view.entity_id()),
            "readonly":!self.view.read(cx).is_editable(),"burst_token":self.burst_token,
            "scope":"new host-owned SceneView session; no Edit capability override"})
        );
        self.case = case;
        self.baseline.clear();
        self.native_inputs.clear();
        self.last_pointer = "no native click in this preset".into();
        self.status="Preset reset. Wait for pending=false, then inspect the actual canvas and click probes.".into();
        cx.notify();
    }
    fn edit(
        &mut self,
        edit: impl FnOnce(&mut Scene) -> Result<(), SceneError>,
        cx: &mut Context<Self>,
    ) {
        let before = self.view.read(cx).editor.scene().to_json().unwrap();
        self.view.update(cx, |v, cx| {
            v.editor.transaction(edit).unwrap();
            cx.notify();
        });
        if self.view.read(cx).editor.scene().to_json().unwrap() != before {
            self.baseline.push(before);
        }
        self.status =
            "Canonical edit applied; wait for replacement before recording live state.".into();
        cx.notify();
    }
    fn erasers(&mut self, edit: impl Fn(&mut DisplayObject), cx: &mut Context<Self>) {
        let ids = self.case.erasers();
        self.edit(
            |s| {
                for id in ids {
                    s.update_object(&(*id).into(), |o| edit(o))?;
                }
                Ok(())
            },
            cx,
        );
    }
    fn mode(&mut self, mode: EraserMode, cx: &mut Context<Self>) {
        self.erasers(|o| o.eraser = mode, cx);
    }
    fn opacity(&mut self, cx: &mut Context<Self>) {
        let old = self
            .view
            .read(cx)
            .editor
            .scene()
            .get(&self.case.erasers()[0].into())
            .unwrap()
            .object
            .style
            .opacity;
        let next = if old == 1. {
            0.5
        } else if old == 0.5 {
            0.01
        } else if old == 0.01 {
            0.
        } else {
            1.
        };
        self.erasers(|o| o.style.opacity = next, cx);
    }
    fn paint(&mut self, cx: &mut Context<Self>) {
        let has_fill = self
            .view
            .read(cx)
            .editor
            .scene()
            .get(&self.case.erasers()[0].into())
            .unwrap()
            .object
            .style
            .fill
            .is_some();
        self.erasers(
            |o| {
                o.style.fill = (!has_fill).then(|| Color::WHITE.into());
                o.style.stroke = None;
            },
            cx,
        );
    }
    fn variant(&mut self, cx: &mut Context<Self>) {
        match self.case {
            Case::Order => self.edit(
                |s| {
                    let before = s
                        .get(&"parent".into())
                        .unwrap()
                        .children
                        .iter()
                        .position(|id| id.as_str() == "later")
                        .unwrap()
                        == 1;
                    s.reparent(
                        &"later".into(),
                        Some("parent".into()),
                        Some(if before { 2 } else { 1 }),
                        false,
                    )
                },
                cx,
            ),
            Case::Isolation => self.erasers(
                |o| {
                    if !o.visible {
                        o.visible = true;
                        o.style.opacity = 0.;
                    } else if o.style.opacity == 0. {
                        o.style.opacity = 1.;
                    } else {
                        o.visible = false;
                        o.style.opacity = 1.;
                    }
                },
                cx,
            ),
            Case::Masks => self.erasers(
                |o| {
                    o.style.blend_mode = if o.style.blend_mode == BlendMode::PassThrough {
                        BlendMode::Multiply
                    } else {
                        BlendMode::PassThrough
                    }
                },
                cx,
            ),
            Case::Root => {
                self.view.update(cx, |v, cx| {
                    v.compositing_mode = if v.compositing_mode == SceneCompositing::OwnedCanvas {
                        SceneCompositing::Domains
                    } else {
                        SceneCompositing::OwnedCanvas
                    };
                    cx.notify();
                });
                self.status="View mode changed (not an Undo edit). Domains + exposed root eraser must report UnsupportedExternalBackdrop.".into();
                cx.notify();
            }
        }
    }
    fn undo(&mut self, cx: &mut Context<Self>) {
        self.burst_token += 1;
        self.burst_left = 0;
        self.burst_queued = false;
        let Some(expected) = self.baseline.pop() else {
            self.status = "No fixture edit to undo in this preset".into();
            cx.notify();
            return;
        };
        let restored = self.view.update(cx, |v, cx| {
            let undone = v.editor.undo();
            cx.notify();
            undone && v.editor.scene().to_json().unwrap() == expected
        });
        self.status = format!(
            "Undo restored exact canonical JSON: {restored}; verify replacement and hit IDs."
        );
        println!(
            "{}",
            json!({"kind":"undo","exact_canonical_baseline_restored":restored})
        );
        cx.notify();
    }
    fn advance_burst(&mut self, token: u64, cx: &mut Context<Self>) -> bool {
        if token != self.burst_token {
            return false;
        }
        self.burst_queued = false;
        if self.burst_left == 0 {
            return false;
        }
        self.burst_left -= 1;
        let mode = if self.burst_left.is_multiple_of(2) {
            EraserMode::Pixel
        } else {
            EraserMode::Path
        };
        self.erasers(
            |o| {
                o.eraser = mode;
                o.style.opacity = 1.;
                o.visible = true;
                o.style.fill = Some(Color::WHITE.into());
            },
            cx,
        );
        true
    }
    fn start_burst(&mut self, cx: &mut Context<Self>) {
        self.burst_token += 1;
        self.burst_left = 8;
        self.burst_queued = false;
        self.status="Eight frame-spaced mode changes; final state Pixel, opacity 1, visible, white. Native cancellation timing is not asserted.".into();
        cx.notify();
    }
    fn expect(&self, cx: &App) -> String {
        let view = self.view.read(cx);
        let o = &view
            .editor
            .scene()
            .get(&self.case.erasers()[0].into())
            .unwrap()
            .object;
        let detail = match self.case {
            Case::Order => {
                "Earlier red can erase to external blue; green survives only when later. Right cut stays inside rotated rounded clip. Reach remains a hit outside parent bounds."
            }
            Case::Isolation => {
                "Left hidden/zero marker still isolates: blue matches right Normal control. None removes marker isolation: left Multiply sees red (black). Visible marker reveals red."
            }
            Case::Masks => {
                "Columns: mask opacity 1 / 0.5 / opacity 1 with fill alpha 128. Pixel: first+third retain earlier content; middle clears. Path+PassThrough clears all; explicit blend restores opacity-1 no-op."
            }
            Case::Root => {
                if expects_external_error(view, self.case) {
                    "Expected typed UnsupportedExternalBackdrop. Return Owned for recovery; host chrome must stay intact. CPU export still owns its separate backdrop."
                } else {
                    "Owned root erase reveals yellow/grid; later green survives. Domains is unsupported while root eraser is exposed. None is ordinary paint."
                }
            }
        };
        format!(
            "{:?}, opacity {}, visible {}, fill {}, blend {:?}. {detail}",
            o.eraser,
            o.style.opacity,
            o.visible,
            o.style.fill.is_some(),
            o.style.blend_mode
        )
    }
    fn record(&mut self, export: bool, cx: &mut Context<Self>) {
        self.sequence += 1;
        let stem = format!("{:03}", self.sequence);
        let view = self.view.read(cx);
        let before = view.editor.scene().to_json().unwrap();
        let revision = view.editor.engine.revision();
        let selection = view.editor.selection.clone();
        let viewport = view.editor.viewport;
        let mut receipt = json!({"fixture":"scene-erasers","session":self.session,"sequence":self.sequence,
            "expectation":self.expect(cx),"live":snapshot(view,self.case),"native_inputs":self.native_inputs,
            "canonical_scene":serde_json::from_str::<Value>(&before).unwrap(),
            "native_framebuffer_captured":false,"native_acceptance":"requires original physical screenshot + human visual check",
            "export":Value::Null});
        let outcome = (|| -> Result<(), String> {
            if export {
                receipt["export"] = json!({"evidence_kind":"independent canonical CPU export; not accepted native pixels",
                    "world_bounds":[0,0,WIDTH,HEIGHT],"scale":1,"pixel_ratio":1,
                    "max_surface_writes":CompositeLimits::default().max_surface_writes,
                    "background":"explicit opaque yellow, without view grid","stage":"png_encode"});
                let png = view
                    .export_scene(request(SceneExportFormat::Png), CompositeLimits::default())
                    .map_err(|e| format!("{e:?}"))?;
                let png_path = self.session.join(format!("{stem}-cpu-export.png"));
                receipt["export"]["stage"] = json!("png_write");
                write_new(&png_path, &png.bytes)?;
                receipt["export"]["png"] = json!(png_path);
                receipt["export"]["png_bytes"] = json!(png.bytes.len());
                receipt["export"]["stage"] = json!("png_decode");
                let p = image::load_from_memory(&png.bytes)
                    .map_err(|e| e.to_string())?
                    .to_rgba8();
                receipt["export"]["png_decoded_size"] = json!(p.dimensions());
                if p.dimensions() != (WIDTH as u32, HEIGHT as u32) {
                    return Err("CPU PNG export extent differs from fixed request".into());
                }
                receipt["export"]["rgba_fnv1a64"] = json!(fingerprint(p.as_raw()));
                receipt["export"]["probes"] = json!(
                    self.case
                        .probes()
                        .iter()
                        .map(|(name, x, y)| json!({"name":name,"world":[x,y],
                    "export_rgba":p.get_pixel(*x as u32,*y as u32).0}))
                        .collect::<Vec<_>>()
                );
                receipt["export"]["stage"] = json!("webp_encode");
                let webp = view
                    .export_scene(
                        request(SceneExportFormat::WebPLossless),
                        CompositeLimits::default(),
                    )
                    .map_err(|e| format!("{e:?}"))?;
                let webp_path = self.session.join(format!("{stem}-cpu-export.webp"));
                receipt["export"]["stage"] = json!("webp_write");
                write_new(&webp_path, &webp.bytes)?;
                receipt["export"]["webp"] = json!(webp_path);
                receipt["export"]["webp_bytes"] = json!(webp.bytes.len());
                receipt["export"]["stage"] = json!("webp_decode");
                let w = image::load_from_memory(&webp.bytes)
                    .map_err(|e| e.to_string())?
                    .to_rgba8();
                receipt["export"]["webp_decoded_size"] = json!(w.dimensions());
                receipt["export"]["stage"] = json!("compare_decoded_exports");
                receipt["export"]["lossless_decoded_equal"] = json!(p == w);
                if p != w {
                    return Err("CPU PNG/WebP decoded equality failed".into());
                }
                receipt["export"]["stage"] = json!("complete");
            }
            Ok(())
        })();
        receipt["source_unchanged"] = json!(
            before == view.editor.scene().to_json().unwrap()
                && revision == view.editor.engine.revision()
                && selection == view.editor.selection
                && viewport == view.editor.viewport
        );
        if let Err(error) = &outcome {
            receipt["operation_error"] = json!(error);
        }
        let path = self.session.join(format!("{stem}-receipt.json"));
        let saved = write_new(&path, &serde_json::to_vec_pretty(&receipt).unwrap());
        println!("{receipt}");
        self.status = match saved {
            Ok(()) => format!(
                "Saved {}{}",
                path.display(),
                outcome.err().map(|e| format!("; {e}")).unwrap_or_default()
            ),
            Err(error) => format!("Receipt write failed: {error}"),
        };
        cx.notify();
    }
    fn failures(&mut self, cx: &mut Context<Self>) {
        self.sequence += 1;
        let view = self.view.read(cx);
        let before = view.editor.scene().to_json().unwrap();
        let revision = view.editor.engine.revision();
        let selection = view.editor.selection.clone();
        let viewport = view.editor.viewport;
        let cancelled = view.export_scene_cancellable(
            request(SceneExportFormat::Png),
            CompositeLimits::default(),
            &AtomicBool::new(true),
        );
        let tight = view.export_scene(
            request(SceneExportFormat::Png),
            CompositeLimits {
                max_surface_bytes: 1,
                ..Default::default()
            },
        );
        let cancelled_ok = matches!(cancelled, Err(CompositeError::Superseded));
        let budget_ok = matches!(tight, Err(CompositeError::WorkingSetLimit));
        let unchanged = before == view.editor.scene().to_json().unwrap()
            && revision == view.editor.engine.revision()
            && selection == view.editor.selection
            && viewport == view.editor.viewport;
        self.status = format!(
            "CPU export only: Superseded={cancelled_ok}, WorkingSetLimit={budget_ok}, source unchanged={unchanged}. No live job/publication claim."
        );
        let no_output = cancelled.is_err() && tight.is_err();
        let receipt = json!({"kind":"cpu_export_failure_checks","session":self.session,"sequence":self.sequence,
            "live":snapshot(view,self.case),"cancel_error":cancelled.err().map(|e|format!("{e:?}")),
            "budget_error":tight.err().map(|e|format!("{e:?}")),"source_unchanged":unchanged,"no_successful_export":no_output,
            "expected_cancel_error_observed":cancelled_ok,"expected_budget_error_observed":budget_ok,
            "scope":"Does not expose allocator ledger, native worker cancellation, or GPU publication."});
        let path = self
            .session
            .join(format!("{:03}-failure-receipt.json", self.sequence));
        if let Err(error) = write_new(&path, &serde_json::to_vec_pretty(&receipt).unwrap()) {
            self.status
                .push_str(&format!(" Receipt write failed: {error}"));
        }
        println!("{receipt}");
        cx.notify();
    }
}
impl Render for Lab {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.burst_left > 0 && !self.burst_queued {
            self.burst_queued = true;
            let token = self.burst_token;
            let weak = cx.entity().downgrade();
            window.on_next_frame(move |_, cx| {
                let _ = weak.update(cx, |this, cx| {
                    this.advance_burst(token, cx);
                });
            });
            window.request_animation_frame();
        }
        let view = self.view.read(cx);
        let live = format!(
            "pending={} · presented={} · errors={:?}",
            view.compositing_pending,
            view.presented_geometry().is_some(),
            view.composite_diagnostics
        );
        let variant = match self.case {
            Case::Order => "Sibling order",
            Case::Isolation => "Hidden / zero / visible",
            Case::Masks => "Path explicit blend",
            Case::Root => "Owned / Domains",
        };
        let expectation = self.expect(cx);
        div().size_full().flex().flex_col().bg(rgb(0x151d2b)).text_color(rgb(0xffffff)).text_sm()
            .child(div().h(px(43.)).px_3().flex().items_center().gap_2()
                .child(ControlButton::new("case-a","A Order + clip").on_click(cx.listener(|s,_,_,cx|s.preset(Case::Order,cx))))
                .child(ControlButton::new("case-b","B Isolation").on_click(cx.listener(|s,_,_,cx|s.preset(Case::Isolation,cx))))
                .child(ControlButton::new("case-c","C Masks").on_click(cx.listener(|s,_,_,cx|s.preset(Case::Masks,cx))))
                .child(ControlButton::new("case-d","D Root").on_click(cx.listener(|s,_,_,cx|s.preset(Case::Root,cx))))
                .child(ControlButton::new("record","Live receipt").on_click(cx.listener(|s,_,_,cx|s.record(false,cx))))
                .child(ControlButton::new("export","CPU PNG + WebP").on_click(cx.listener(|s,_,_,cx|s.record(true,cx)))))
            .child(div().h(px(43.)).px_3().flex().items_center().gap_2()
                .child(ControlButton::new("pixel","Pixel").on_click(cx.listener(|s,_,_,cx|s.mode(EraserMode::Pixel,cx))))
                .child(ControlButton::new("path","Path").on_click(cx.listener(|s,_,_,cx|s.mode(EraserMode::Path,cx))))
                .child(ControlButton::new("none","None").on_click(cx.listener(|s,_,_,cx|s.mode(EraserMode::None,cx))))
                .child(ControlButton::new("opacity","Opacity 1/.5/.01/0").on_click(cx.listener(|s,_,_,cx|s.opacity(cx))))
                .child(ControlButton::new("paint","Fill / no paint").on_click(cx.listener(|s,_,_,cx|s.paint(cx))))
                .child(ControlButton::new("variant",variant).on_click(cx.listener(|s,_,_,cx|s.variant(cx))))
                .child(ControlButton::new("undo","Undo").on_click(cx.listener(|s,_,_,cx|s.undo(cx)))))
            .child(div().h(px(40.)).px_3().flex().items_center().gap_2()
                .child(ControlButton::new("burst","8 frame changes").on_click(cx.listener(|s,_,_,cx|s.start_burst(cx))))
                .child(ControlButton::new("failures","CPU failure checks").on_click(cx.listener(|s,_,_,cx|s.failures(cx))))
                .child(format!("{} · {} burst steps left",self.case.label(),self.burst_left)))
            .child(div().flex_1().min_h_0().overflow_hidden().child(self.view.clone()))
            .child(div().h(px(205.)).flex_shrink_0().px_3().py_2().flex().flex_col().gap_1()
                .child(expectation).child(live).child(format!("Native click: {}",self.last_pointer))
                .child(self.status.clone())
                .child("Actual SceneView above. Receipts are observations, not automatic acceptance. Text/Image special producers, path containers/Text/Custom and external host erase remain outside this native fixture."))
    }
}
// This branch exercises these exact fixture builders before any GPUI Application
// exists. It is a public CPU export tour, never a fake SceneView/native acceptance.
fn cpu_encode(
    scene: &Scene,
    format: SceneExportFormat,
    limits: CompositeLimits,
    cancelled: bool,
) -> Result<SceneExport, CompositeError> {
    SceneExportPlan::new(scene, request(format), limits)?.encode_with_inputs(
        &ImageInputs::default(),
        &TextInputs::default(),
        SceneExportPreparation::default(),
        &AtomicBool::new(cancelled),
    )
}
fn cpu_expected(case: Case) -> (Vec<Option<&'static str>>, Vec<[u8; 4]>) {
    let red = [255, 0, 0, 255];
    let blue = [0, 0, 255, 255];
    let green = [0, 220, 90, 255];
    let yellow = [243, 221, 148, 255];
    match case {
        Case::Order => (
            vec![
                Some("cut"),
                Some("later"),
                Some("reach"),
                Some("clip-cut"),
                Some("clip-external"),
            ],
            vec![blue, green, blue, blue, blue],
        ),
        Case::Isolation => (
            vec![Some("test-blue"), Some("control-blue")],
            vec![blue, blue],
        ),
        Case::Masks => (
            vec![
                Some("full-cut"),
                Some("half-cut"),
                Some("paint-cut"),
                Some("full-later"),
            ],
            vec![red, blue, [128, 0, 127, 255], green],
        ),
        Case::Root => (
            vec![
                Some("root-cut"),
                Some("root-later"),
                Some("root-earlier"),
                None,
            ],
            vec![yellow, green, red, yellow],
        ),
    }
}
fn cpu_fixture_record(
    scene: &Scene,
    case: Case,
    tag: &str,
    expected_pixels: &[[u8; 4]],
    check_webp: bool,
    session: &Path,
) -> Value {
    let before = scene.to_json().unwrap();
    let mut receipt = json!({"kind":"headless_fixture_cpu_export","case":case.label(),"variant":tag,
        "native_application_created":false,"native_framebuffer_captured":false,
        "world_bounds":[0,0,WIDTH,HEIGHT],"scale":1,"pixel_ratio":1,
        "max_surface_writes":CompositeLimits::default().max_surface_writes,
        "canonical_json_fnv1a64":fingerprint(before.as_bytes()),"stage":"validate_scene"});
    let result = (|| -> Result<(), String> {
        scene.validate().map_err(|e| e.to_string())?;
        let engine = SceneEngine::new(scene.clone()).map_err(|e| e.to_string())?;
        let expected_heads = cpu_expected(case).0;
        receipt["stage"] = json!("geometry_probes");
        let mut probes = Vec::new();
        let mut geometry_ok = true;
        for ((name, x, y), expected_head) in case.probes().iter().zip(expected_heads) {
            let point = ScenePoint::new(*x, *y);
            let scene_hits = scene
                .hit_test_all(point, HitTestOptions::default())
                .map_err(|e| e.to_string())?;
            let engine_hits = engine
                .hit_test_all(point, HitTestOptions::default())
                .map_err(|e| e.to_string())?;
            let scene_ids: Vec<_> = scene_hits.iter().map(|h| h.id.as_str()).collect();
            let engine_ids: Vec<_> = engine_hits.iter().map(|h| h.id.as_str()).collect();
            let expected_underlying = match (case, *name) {
                (Case::Order, "hole") => Some("earlier"),
                (Case::Root, "hole") => Some("root-earlier"),
                _ => None,
            };
            let pass = scene_ids == engine_ids
                && scene_ids.first().copied() == expected_head
                && expected_underlying.is_none_or(|id| scene_ids.contains(&id));
            geometry_ok &= pass;
            probes.push(json!({"name":name,"world":[x,y],"scene_ids":scene_ids,"engine_ids":engine_ids,
                "expected_head":expected_head,"expected_erased_candidate":expected_underlying,"pass":pass}));
        }
        receipt["geometry_probes"] = json!(probes);
        if !geometry_ok {
            return Err("Fixture Scene/Engine probe IDs differ from the declared contract".into());
        }
        if case == Case::Order {
            let parent = engine
                .world_bounds(&"parent".into())
                .map_err(|e| e.to_string())?
                .ok_or("parent bounds missing")?;
            let reach = engine
                .world_bounds(&"reach".into())
                .map_err(|e| e.to_string())?
                .ok_or("reach bounds missing")?;
            receipt["bounds"] = json!({"parent":parent,"reach":reach});
            if parent != SceneBounds::new(ScenePoint::new(60., 80.), SceneSize::new(280., 200.))
                || reach != SceneBounds::new(ScenePoint::new(390., 130.), SceneSize::new(40., 40.))
            {
                return Err(
                    "Fixture eraser/parent bounds differ from the declared contract".into(),
                );
            }
        }
        receipt["stage"] = json!("png_encode");
        let png = cpu_encode(
            scene,
            SceneExportFormat::Png,
            CompositeLimits::default(),
            false,
        )
        .map_err(|e| format!("{e:?}"))?;
        let png_path = session.join(format!("{tag}-cpu-export.png"));
        receipt["stage"] = json!("png_write");
        write_new(&png_path, &png.bytes)?;
        receipt["png"] = json!(png_path);
        receipt["stage"] = json!("png_decode");
        let decoded = image::load_from_memory(&png.bytes)
            .map_err(|e| e.to_string())?
            .to_rgba8();
        receipt["png_decoded_size"] = json!(decoded.dimensions());
        if decoded.dimensions() != (WIDTH as u32, HEIGHT as u32) {
            return Err("CPU PNG extent mismatch".into());
        }
        receipt["rgba_fnv1a64"] = json!(fingerprint(decoded.as_raw()));
        let samples: Vec<_> = case
            .probes()
            .iter()
            .map(|(_, x, y)| decoded.get_pixel(*x as u32, *y as u32).0)
            .collect();
        receipt["pixel_probes"] = json!({"actual_rgba":samples,"expected_rgba":expected_pixels,
            "all_export_pixels_opaque":decoded.pixels().all(|p|p.0[3]==255)});
        let pixels_ok = samples == expected_pixels && decoded.pixels().all(|p| p.0[3] == 255);
        if check_webp {
            receipt["stage"] = json!("webp_encode");
            let webp = cpu_encode(
                scene,
                SceneExportFormat::WebPLossless,
                CompositeLimits::default(),
                false,
            )
            .map_err(|e| format!("{e:?}"))?;
            let webp_path = session.join(format!("{tag}-cpu-export.webp"));
            receipt["stage"] = json!("webp_write");
            write_new(&webp_path, &webp.bytes)?;
            receipt["webp"] = json!(webp_path);
            receipt["stage"] = json!("webp_decode");
            let other = image::load_from_memory(&webp.bytes)
                .map_err(|e| e.to_string())?
                .to_rgba8();
            receipt["webp_decoded_size"] = json!(other.dimensions());
            receipt["lossless_decoded_equal"] = json!(decoded == other);
            if decoded != other {
                return Err("CPU PNG/WebP decoded equality mismatch".into());
            }
        }
        receipt["stage"] = json!("typed_failure_checks");
        let svg = cpu_encode(
            scene,
            SceneExportFormat::Svg,
            CompositeLimits::default(),
            false,
        )
        .err();
        let expected_svg = CompositeError::UnsupportedShape(
            match case {
                Case::Order | Case::Root => "SVG erasers",
                Case::Isolation => "SVG blend mode",
                Case::Masks => "SVG alpha masks",
            }
            .into(),
        );
        let cancelled = cpu_encode(
            scene,
            SceneExportFormat::Png,
            CompositeLimits::default(),
            true,
        )
        .err();
        let tight = cpu_encode(
            scene,
            SceneExportFormat::Png,
            CompositeLimits {
                max_surface_bytes: 1,
                ..Default::default()
            },
            false,
        )
        .err();
        let failures_ok = svg.as_ref() == Some(&expected_svg)
            && cancelled == Some(CompositeError::Superseded)
            && tight == Some(CompositeError::WorkingSetLimit);
        receipt["typed_failures"] = json!({"svg":svg.map(|e|format!("{e:?}")),"expected_svg":format!("{expected_svg:?}"),
            "cancelled":cancelled.map(|e|format!("{e:?}")),"low_surface_budget":tight.map(|e|format!("{e:?}")),"pass":failures_ok});
        if !pixels_ok || !failures_ok {
            return Err(
                "Fixture CPU pixel or typed-error expectation failed; inspect receipt".into(),
            );
        }
        receipt["stage"] = json!("complete");
        Ok(())
    })();
    receipt["source_unchanged"] = json!(scene.to_json().unwrap() == before);
    receipt["pass"] = json!(result.is_ok() && receipt["source_unchanged"] == true);
    if let Err(error) = result {
        receipt["error"] = json!(error);
    }
    let path = session.join(format!("{tag}-receipt.json"));
    receipt["receipt_path"] = json!(path);
    if let Err(error) = write_new(&path, &serde_json::to_vec_pretty(&receipt).unwrap()) {
        receipt["pass"] = json!(false);
        receipt["receipt_write_error"] = json!(error);
    }
    println!("{receipt}");
    receipt
}
fn evidence_root() -> PathBuf {
    std::env::var_os("FLOW_SCENE_ERASER_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

fn cpu_tour() -> Result<(), String> {
    let session = evidence_root().join(format!(
        "gpui-scene-erasers-cpu-tour-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
    ));
    std::fs::create_dir(&session).map_err(|e| e.to_string())?;
    let mut results = Vec::new();
    for (case, tag) in [
        (Case::Order, "a-default"),
        (Case::Isolation, "b-default"),
        (Case::Masks, "c-default"),
        (Case::Root, "d-default"),
    ] {
        results.push(cpu_fixture_record(
            &document(case),
            case,
            tag,
            &cpu_expected(case).1,
            true,
            &session,
        ));
    }
    // Three fixture-specific variants, not another implementation test matrix.
    let mut masks = document(Case::Masks);
    for id in Case::Masks.erasers() {
        masks
            .update_object(&(*id).into(), |o| {
                o.eraser = EraserMode::Path;
                o.style.opacity = 0.01;
                o.style.fill = None;
            })
            .map_err(|e| e.to_string())?;
    }
    results.push(cpu_fixture_record(
        &masks,
        Case::Masks,
        "c-path-no-paint-positive-opacity",
        &[
            [0, 0, 255, 255],
            [0, 0, 255, 255],
            [0, 0, 255, 255],
            [0, 220, 90, 255],
        ],
        false,
        &session,
    ));
    for id in Case::Masks.erasers() {
        masks
            .update_object(&(*id).into(), |o| o.style.blend_mode = BlendMode::Multiply)
            .map_err(|e| e.to_string())?;
    }
    results.push(cpu_fixture_record(
        &masks,
        Case::Masks,
        "c-path-explicit-blend",
        &cpu_expected(Case::Masks).1,
        false,
        &session,
    ));
    let mut isolation = document(Case::Isolation);
    isolation
        .update_object(&"marker".into(), |o| o.eraser = EraserMode::None)
        .map_err(|e| e.to_string())?;
    results.push(cpu_fixture_record(
        &isolation,
        Case::Isolation,
        "b-hidden-marker-none",
        &[[0, 0, 0, 255], [0, 0, 255, 255]],
        false,
        &session,
    ));
    let passed = results.iter().filter(|r| r["pass"] == true).count();
    let summary = json!({"kind":"scene_erasers_cpu_tour_summary","session":session,"cases":results.len(),"passed":passed,
        "native_application_created":false,"scope":"fixture Scene/Engine geometry + public CPU scene export; no native callbacks, GPU readback or accepted framebuffer claim",
        "results":results.iter().map(|r|json!({"variant":r["variant"],"pass":r["pass"],"receipt":r["receipt_path"]})).collect::<Vec<_>>()});
    write_new(
        &session.join("summary.json"),
        &serde_json::to_vec_pretty(&summary).unwrap(),
    )?;
    println!("{summary}");
    if passed != results.len() {
        return Err(format!(
            "CPU fixture tour passed {passed}/{}; preserved evidence: {}",
            results.len(),
            session.display()
        ));
    }
    Ok(())
}
// Real App/entity regression, without a native window or rendered-frame claim.
fn session_reset_tour(cx: &mut App) -> Result<(Value, Vec<WeakEntity<SceneView>>), String> {
    let lab = cx.new(Lab::new);
    let mut retired = Vec::new();
    let mut results = Vec::new();
    for case in [
        Case::Order,
        Case::Isolation,
        Case::Masks,
        Case::Root,
        Case::Order,
        Case::Order,
    ] {
        let result = lab.update(cx, |lab, cx| -> Result<Value, String> {
            let previous = lab.view.clone();
            let old_id = previous.entity_id();
            let before = previous.read(cx).editor.scene().to_json().map_err(|e|e.to_string())?;
            let error = previous.update(cx, |v,cx|v.set_scene(document(case),cx)).err();
            if !matches!(error, Some(gpui_flow::interaction::scene::SceneInteractionError::CapabilityDenied(
                gpui_flow::interaction::Capability::Edit))) {
                return Err(format!("readonly LoadScene returned {error:?}"));
            }
            if previous.read(cx).editor.scene().to_json().map_err(|e|e.to_string())? != before {
                return Err("denied LoadScene changed canonical source".into());
            }
            lab.mode(EraserMode::None,cx);
            previous.update(cx,|v,_| {v.editor.viewport.zoom=1.7;v.editor.viewport.pan=ScenePoint::new(19.,23.);});
            lab.start_burst(cx);
            let old_token=lab.burst_token;
            lab.preset(case,cx);
            let fresh=lab.view.clone();
            let expected=document(case).to_json().map_err(|e|e.to_string())?;
            if fresh.entity_id()==old_id || lab.case!=case || fresh.read(cx).is_editable()
                || fresh.read(cx).editor.scene().to_json().map_err(|e|e.to_string())?!=expected
                || fresh.read(cx).editor.viewport.zoom!=1. || fresh.read(cx).editor.viewport.pan!=ScenePoint::ZERO
                || !lab.baseline.is_empty() || lab.burst_left!=0 || lab.burst_queued {
                return Err("new session/reset invariant failed".into());
            }
            if lab.advance_burst(old_token,cx) || fresh.read(cx).editor.scene().to_json().map_err(|e|e.to_string())?!=expected {
                return Err("retired queued callback mutated new session".into());
            }
            if fresh.update(cx,|v,_|v.editor.undo()) || fresh.update(cx,|v,_|v.editor.redo()) {
                return Err("new session inherited history".into());
            }
            lab.mode(EraserMode::Path,cx);
            lab.undo(cx);
            if fresh.read(cx).editor.scene().to_json().map_err(|e|e.to_string())?!=expected {
                return Err("new session host edit/Undo did not restore exact document".into());
            }
            retired.push(previous.downgrade());
            Ok(json!({"case":case.label(),"passed":true,"old_view":format!("{old_id:?}"),
                "new_view":format!("{:?}",fresh.entity_id()),"readonly_load_scene_denied":true,
                "fresh_history_viewport":true,"obsolete_callback_ignored":true,"host_edit_undo_exact":true}))
        })?;
        results.push(result);
    }
    Ok((
        json!({"kind":"scene_erasers_session_reset_tour","passed":results.len(),"cases":results,
        "native_windows_created":0,
        "scope":"real GPUI App/entities and preset callback; no physical input or rendered-frame acceptance"}),
        retired,
    ))
}
fn main() {
    if std::env::args().any(|arg| arg == "--session-reset-tour") {
        let failed = std::sync::Arc::new(AtomicBool::new(true));
        let result_flag = failed.clone();
        gpui_kit::application().run(move |cx| {
            // The Linux loop starts after its launch callback. Schedule the
            // tour on that loop so quit is not lost before run() begins.
            cx.spawn(async move |cx| {
                // update flushes queued notifications and release effects before
                // checking weak handles, exactly as between real UI callbacks.
                let result = cx.update(session_reset_tour);
                match result {
                    Ok((mut result, retired)) => {
                        let released = retired.iter().all(|view| view.upgrade().is_none());
                        result["retired_entities_released_after_effect_flush"] = json!(released);
                        println!("{result}");
                        result_flag.store(!released, std::sync::atomic::Ordering::Relaxed);
                        if !released { eprintln!("session-reset-tour: retired SceneView still retained after effect flush"); }
                    }
                    Err(error) => eprintln!("session-reset-tour: {error}"),
                }
                cx.update(|cx| cx.quit());
            })
            .detach();
        });
        if failed.load(std::sync::atomic::Ordering::Relaxed) {
            std::process::exit(1);
        }
        return;
    }

    if std::env::args().any(|arg| arg == "--cpu-tour") {
        if let Err(error) = cpu_tour() {
            eprintln!("scene_erasers --cpu-tour: {error}");
            std::process::exit(1);
        }
        return;
    }
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(|cx| {
            gpui_kit::init(cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(1080.), px(790.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                |window, cx| {
                    window.set_window_title("gpui-flow · bounded scene erasers");
                    cx.new(Lab::new)
                },
            )
            .unwrap();
            cx.activate(true);
        });
}
