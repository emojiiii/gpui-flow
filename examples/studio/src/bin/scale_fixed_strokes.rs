//! Bounded APP-02 scale-fixed-stroke acceptance fixture.
//! Build, helper and physical acceptance results are recorded separately.
//! Native observations, independent CPU exports, and the headless helper have
//! separate evidence directories. No GPU readback or backend-equivalence claim.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, SceneCompositing, SceneInputOperation, SceneView, SceneViewEvent,
    composite::{
        CompositeLimits, ImageInputs, SceneExportFormat, SceneExportPlan, SceneExportPreparation,
        SceneExportRequest, TextInputs,
    },
    core::Viewport,
    editor::SceneEditor,
    engine::SceneEngine,
    interaction::{
        Capability, InteractionError,
        scene::{
            SceneCommand, ScenePluginHost,
            plugins::{SceneViewportConstraints, SceneViewportConstraintsPlugin},
        },
    },
    scene::{
        Affine, BlendMode, Bounds as SceneBounds, Color, DisplayObject, EventKind, HitPolicy,
        HitTestOptions, Point as P, Scene, SceneError, Shape, Size as S, Stroke, StrokeAlignment,
        StrokeCap, StrokeJoin, StrokeScaleFixed as Fixed, Style, display,
    },
};
use serde_json::{Value, json};
use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
    time::{SystemTime, UNIX_EPOCH},
};

const WORLD_W: f32 = 450.;
const WORLD_H: f32 = 205.;
const CANVAS_W: f32 = 1040.;
const CANVAS_H: f32 = 420.;
const BG: Color = Color::rgb(23, 32, 43);
const INK: Color = Color::rgb(255, 94, 108);
const MAX_RECORDS: u32 = 256;
const MAX_EVENTS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Case {
    Uniform,
    Axes,
    Hierarchy,
    Dashes,
    Open,
    Layers,
}
impl Case {
    const ALL: [Self; 6] = [
        Self::Uniform,
        Self::Axes,
        Self::Hierarchy,
        Self::Dashes,
        Self::Open,
        Self::Layers,
    ];
    fn label(self) -> &'static str {
        match self {
            Self::Uniform => "A: true / numeric 1 / disabled",
            Self::Axes => "B: scalar X + Y-only threshold",
            Self::Hierarchy => "C: ancestry / rotation / reflection",
            Self::Dashes => "D: local dashes + three caps",
            Self::Open => "E: open Center / Inside / Outside",
            Self::Layers => "F: independent layers + rounded clips",
        }
    }
    fn hint(self) -> &'static str {
        match self {
            Self::Uniform => {
                "Columns true / numeric 1 / disabled. At zoom .5: logical widths 8 / 4 / 4; at 1: 8 / 8 / 8; at 2: 8 / 8 / 16."
            }
            Self::Axes => {
                "Columns (2,4) true / (0.5,1) numeric 1 / (0.5,1.25) numeric 1. At zoom 1 effective local widths 4 / 8 / 16; horizontal thickness 16 / 8 / 20."
            }
            Self::Hierarchy => {
                "Parent scales (2,1), (2,1), (-2,1); child rotations 0, 90, 35 degrees. All use X provenance 2, so local width is 4 at zoom 1."
            }
            Self::Dashes => {
                "Butt / round / square; local dash [18,10], phase -5 remain unchanged. Zoom changes centerline spacing, while fixed width stays 8 logical px."
            }
            Self::Open => {
                "Center / Inside / Outside, no fill paint. The diagonal is a fill-mask closure only; its midpoint must not gain closing-stroke ink or a painted hit."
            }
            Self::Layers => {
                "Red fixed 8, blue numeric 1 width 4, white disabled 2. Normal / reflected / reflected+rotated; two nested rounded clips intersect the ink."
            }
        }
    }
}
fn key(row: &str, col: usize, suffix: &str) -> String {
    format!("{row}-{col}-{suffix}")
}
fn path(svg: &str) -> Result<Shape, SceneError> {
    display::Path::from_svg(svg)
        .map(Into::into)
        .map_err(|e| SceneError::InvalidGeometry(e.to_string()))
}
fn add(scene: &mut Scene, object: DisplayObject, parent: Option<String>) -> Result<(), SceneError> {
    scene
        .insert(object, parent.map(Into::into), None)
        .map(|_| ())
}
fn group(id: String, transform: Affine) -> DisplayObject {
    let mut o = DisplayObject::new(id, display::Group::new()).with_transform(transform);
    o.hit_policy = HitPolicy::None;
    o
}
fn spec(
    case: Case,
    col: usize,
) -> (
    Affine,
    Affine,
    &'static str,
    Fixed,
    StrokeAlignment,
    StrokeCap,
) {
    let standard = "M10 10H95V50";
    let mut s = (
        Affine::IDENTITY,
        Affine::IDENTITY,
        standard,
        Fixed::Fixed,
        StrokeAlignment::Center,
        StrokeCap::Butt,
    );
    match case {
        Case::Uniform => s.3 = [Fixed::Fixed, Fixed::Threshold(1.), Fixed::Disabled][col],
        Case::Axes => {
            let (x, y) = [(2., 4.), (0.5, 1.), (0.5, 1.25)][col];
            s.0 = Affine::translation(22., 12.) * Affine::scale(x, y);
            s.2 = "M0 0H35V10";
            s.3 = if col == 0 {
                Fixed::Fixed
            } else {
                Fixed::Threshold(1.)
            };
        }
        Case::Hierarchy => {
            s.0 = Affine::scale(if col == 2 { -2. } else { 2. }, 1.);
            s.1 = match col {
                0 => Affine::translation(8., 18.),
                1 => Affine::translation(45., 10.) * Affine::rotation(90.),
                _ => Affine::translation(-40., 8.) * Affine::rotation(35.),
            };
            s.2 = "M0 0H36V30";
        }
        Case::Dashes => {
            s.2 = "M8 30H112";
            s.5 = [StrokeCap::Butt, StrokeCap::Round, StrokeCap::Square][col];
        }
        Case::Open => {
            s.4 = [
                StrokeAlignment::Center,
                StrokeAlignment::Inside,
                StrokeAlignment::Outside,
            ][col]
        }
        Case::Layers => {
            s.0 = match col {
                0 => Affine::translation(0., 0.),
                1 => Affine::translation(120., 0.) * Affine::scale(-1., 1.),
                _ => Affine::translation(117., -1.) * Affine::rotation(8.) * Affine::scale(-1., 1.),
            };
            s.2 = "M-8 12H130V60H-8Z";
        }
    }
    s
}
fn document(case: Case) -> Result<Scene, SceneError> {
    let mut scene = Scene::new();
    scene.data = json!({"case":format!("{case:?}"),"edited":false});
    for (row, y, cpu) in [("native", 24., false), ("cpu", 119., true)] {
        for col in 0..3 {
            let mut frame = display::Frame::new(126., 72.).rounded(18.);
            frame.clip_children = case == Case::Layers;
            let mut panel = DisplayObject::new(key(row, col, "panel"), frame)
                .with_transform(Affine::translation(18. + col as f32 * 146., y))
                .with_style(Style {
                    fill: None,
                    blend_mode: if cpu {
                        BlendMode::Normal
                    } else {
                        BlendMode::PassThrough
                    },
                    ..Default::default()
                });
            panel.hit_policy = HitPolicy::None;
            add(&mut scene, panel, None)?;
            let parent = if case == Case::Layers {
                let mut inner = display::Frame::new(118., 62.).rounded(13.);
                inner.clip_children = true;
                let mut object = DisplayObject::new(key(row, col, "inner"), inner)
                    .with_transform(Affine::translation(4., 4.))
                    .with_style(Style {
                        fill: None,
                        ..Default::default()
                    });
                object.hit_policy = HitPolicy::None;
                add(&mut scene, object, Some(key(row, col, "panel")))?;
                key(row, col, "inner")
            } else {
                key(row, col, "panel")
            };
            let (parent_transform, transform, svg, policy, alignment, cap) = spec(case, col);
            add(
                &mut scene,
                group(key(row, col, "scale"), parent_transform),
                Some(parent),
            )?;
            let mut stroke = Stroke::new(INK, 8.)
                .with_cap(cap)
                .with_join(StrokeJoin::Bevel)
                .with_alignment(alignment);
            stroke.scale_fixed = policy;
            if case == Case::Dashes {
                stroke.dash_pattern = vec![18., 10.];
                stroke.dash_offset = -5.;
            }
            let mut layers = vec![];
            if case == Case::Layers {
                let mut blue = Stroke::new(Color::rgb(50, 190, 255), 4.);
                blue.scale_fixed = Fixed::Threshold(1.);
                layers.push(blue);
                layers.push(Stroke::new(Color::WHITE, 2.));
            }
            let mut o = DisplayObject::new(key(row, col, "ink"), path(svg)?)
                .with_transform(transform)
                .with_style(Style {
                    fill: None,
                    stroke: Some(stroke),
                    stroke_layers: layers,
                    ..Default::default()
                });
            o.hit_policy = HitPolicy::Painted;
            add(&mut scene, o, Some(key(row, col, "scale")))?;
        }
    }
    scene.validate()?;
    Ok(scene)
}
// A trusted host edit. The input host remains readonly throughout.
fn host_edit(scene: &mut Scene, case: Case) -> Result<(), SceneError> {
    let edited = !scene.data["edited"].as_bool().unwrap_or(false);
    for row in ["native", "cpu"] {
        for col in 0..3 {
            let (base, _, _, policy, _, _) = spec(case, col);
            scene.update_object(&key(row, col, "scale").into(), |o| {
                o.transform = base * Affine::scale(if edited { 1.25 } else { 1. }, 1.);
            })?;
            scene.update_object(&key(row, col, "ink").into(), |o| {
                if let Some(stroke) = &mut o.style.stroke {
                    stroke.width = if edited { 12. } else { 8. };
                    stroke.scale_fixed = if edited && matches!(policy, Fixed::Threshold(_)) {
                        Fixed::Threshold(2.)
                    } else {
                        policy
                    };
                }
            })?;
        }
    }
    scene.data["edited"] = json!(edited);
    Ok(())
}
fn canonical(scene: &Scene) -> Result<String, String> {
    scene.to_json().map_err(|e| e.to_string())
}
fn fingerprint(bytes: &[u8]) -> String {
    format!(
        "{:016x}",
        bytes
            .iter()
            .fold(0xcbf29ce484222325u64, |h, b| (h ^ u64::from(*b))
                .wrapping_mul(0x100000001b3))
    )
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    f.write_all(bytes).map_err(|e| e.to_string())
}
fn json_new(path: &Path, value: &Value) -> Result<(), String> {
    write_new(
        path,
        &serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?,
    )
}
fn session(kind: &str) -> Result<PathBuf, String> {
    let root = PathBuf::from(std::env::var_os("FLOW_SCALE_FIXED_EVIDENCE_DIR").ok_or(
        "Set FLOW_SCALE_FIXED_EVIDENCE_DIR to an existing absolute per-run evidence root",
    )?);
    if !root.is_absolute() || !root.is_dir() {
        return Err("Evidence root must be an existing absolute directory".into());
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let session = root.join(format!("scale-fixed-{kind}-{}-{stamp}", std::process::id()));
    // create_dir fails on an existing directory. Every evidence file uses create_new.
    std::fs::create_dir(&session).map_err(|e| e.to_string())?;
    json_new(
        &session.join("manifest.json"),
        &json!({"fixture":"scale_fixed_strokes",
        "kind":kind,"session":session,"source_status":"fixture source; see separate validation record",
        "native_framebuffer_captured":false,"physical_native_acceptance":"not evaluated by this program"}),
    )?;
    Ok(session)
}
fn bounded_push(items: &mut Vec<Value>, item: Value) {
    if items.len() == MAX_EVENTS {
        items.remove(0);
    }
    items.push(item);
}
fn bounds(scene: &Scene, id: &str) -> Value {
    match scene.world_bounds(&id.into()) {
        Ok(b) => json!({"world":b}),
        Err(e) => json!({"typed_error":format!("{e:?}")}),
    }
}
fn hit_ids(scene: &Scene, point: P) -> Value {
    hit_ids_with_options(scene, point, HitTestOptions::default())
}
fn hit_ids_with_options(scene: &Scene, point: P, options: HitTestOptions) -> Value {
    match scene.hit_test_all(point, options) {
        Ok(h) => json!(h.iter().map(|h| h.id.to_string()).collect::<Vec<_>>()),
        Err(e) => json!({"typed_error":format!("{e:?}")}),
    }
}
fn strokes(object: &DisplayObject) -> Vec<Value> {
    object.style.stroke.iter().chain(object.style.stroke_layers.iter()).enumerate()
        .map(|(layer,s)|json!({"layer":layer,"width":s.width,
            "policy_debug":format!("{:?}",s.scale_fixed),"alignment_debug":format!("{:?}",s.alignment),
            "cap_debug":format!("{:?}",s.cap),"join_debug":format!("{:?}",s.join),
            "dash_local":s.dash_pattern,"phase_local":s.dash_offset})).collect()
}
fn probes(case: Case) -> [(&'static str, P); 3] {
    match case {
        Case::Axes => [
            ("horizontal", P::new(17.5, 0.)),
            ("vertical", P::new(35., 5.)),
            ("closure", P::new(17.5, 5.)),
        ],
        Case::Hierarchy => [
            ("horizontal", P::new(18., 0.)),
            ("vertical", P::new(36., 15.)),
            ("closure", P::new(18., 15.)),
        ],
        Case::Dashes => [
            ("dash-a", P::new(15., 30.)),
            ("dash-b", P::new(30., 30.)),
            ("off-line", P::new(50., 15.)),
        ],
        _ => [
            ("top-inner", P::new(45., 12.5)),
            ("top-outer", P::new(45., 7.5)),
            ("closure", P::new(52.5, 30.)),
        ],
    }
}
fn snapshot(view: &SceneView, case: Case) -> Value {
    let source = view.editor.scene();
    let presented = view.presented_geometry();
    let objects = ["native", "cpu"]
        .into_iter()
        .flat_map(|row| (0..3).map(move |col| (row, col)))
        .map(|(row, col)| {
            let id = key(row, col, "ink");
            let source_node = source.get(&id.as_str().into());
            let shown = presented.and_then(|s| s.get(&id.as_str().into()));
            let candidate = source.resolved_object(&id.as_str().into(), view.editor.viewport.zoom);
            let scale = source.stroke_scale(&id.as_str().into());
            let points = probes(case)
                .into_iter()
                .map(
                    |(label, local)| match source.local_to_world(&id.as_str().into(), local) {
                        Ok(world) => json!({"label":label,"local":local,"world":world,
                        "screen_in_canvas":view.editor.viewport.world_to_screen(world),
                        "reference_camera_1_ids":hit_ids(source,world),
                        "presented_ids":presented.map(|s|hit_ids(s,world))}),
                        Err(e) => json!({"label":label,"typed_error":format!("{e:?}")}),
                    },
                )
                .collect::<Vec<_>>();
            json!({"id":id,"intended_route":row,
                "authored":source_node.map(|n|strokes(&n.object)),
                "candidate_effective":candidate.as_ref().ok().map(strokes),
                "candidate_error":candidate.err().map(|e|format!("{e:?}")),
                "ancestry_scale":scale.as_ref().ok().map(|s|[s.x,s.y]),
                "scale_error":scale.err().map(|e|format!("{e:?}")),
                "presented_effective":shown.map(|n|strokes(&n.object)),
                "reference_camera_1_bounds":bounds(source,&id),
                "presented_bounds":presented.map(|s|bounds(s,&id)),"probes":points})
        })
        .collect::<Vec<_>>();
    json!({"evidence_kind":"live public SceneView state, separate from physical pixels",
        "case":case.label(),"readonly":!view.is_editable(),"revision":view.editor.engine.revision(),
        "viewport":view.editor.viewport,"pending":view.compositing_pending,
        "presented_available":presented.is_some(),
        "presented_logical_camera":presented.and_then(Scene::resolved_stroke_camera),
        "presented_visible_bounds":presented.map(|s|match s.visible_bounds(){Ok(b)=>json!(b),Err(e)=>json!({"typed_error":format!("{e:?}")})}),
        "composite_issues":view.composite_diagnostics.iter().map(|i|json!({"id":i.object,"typed_error":format!("{:?}",i.error)})).collect::<Vec<_>>(),
        "render_issues":format!("{:?}",view.diagnostics),"interaction_error":view.interaction_error,
        "canonical_json":canonical(source).map(|s|json!({"fnv1a64":fingerprint(s.as_bytes())})).unwrap_or_else(|e|json!({"error":e})),
        "objects":objects})
    // Do not serialize a presented Scene, Style or Stroke: Resolved is derived
    // and deliberately rejects document serde. The debug strings above are safe.
}

struct Lab {
    view: Entity<SceneView>,
    case: Case,
    session: PathBuf,
    status: String,
    last_pointer: String,
    sequence: u32,
    actions: Vec<Value>,
    inputs: Vec<Value>,
    undo: Vec<(String, String)>,
    redo: Vec<(String, String)>,
    constraints: bool,
    burst_token: u64,
    burst_left: u8,
    burst_queued: bool,
    actual_dpr: Option<f32>,
    _observe: Subscription,
    _input: Subscription,
}
impl Lab {
    fn new_view(case: Case, cx: &mut Context<Self>) -> Result<Entity<SceneView>, String> {
        let engine = SceneEngine::new(document(case).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        let view = cx.new(|cx| {
            let mut v = SceneView::from_engine(engine, cx);
            v.compositing_mode = SceneCompositing::Domains;
            v.theme.background = 0x17202b;
            v.show_grid = false;
            v.set_editable(false, cx);
            v.set_input_tracing(true);
            v
        });
        view.update(cx, |v, cx| {
            v.execute_command(
                SceneCommand::SetViewport {
                    viewport: Viewport {
                        pan: P::new(40., 12.),
                        ..Default::default()
                    },
                },
                cx,
            )
        })
        .map_err(|e| e.to_string())?;
        Ok(view)
    }
    fn subscriptions(
        view: &Entity<SceneView>,
        cx: &mut Context<Self>,
    ) -> (Subscription, Subscription) {
        let observe = cx.observe(view, |_, _, cx| cx.notify());
        let input=cx.subscribe(view,|this:&mut Self,entity,event,cx|{
            if let SceneViewEvent::InputTrace(trace)=event
                && let SceneInputOperation::Canvas(gpui_flow::interaction::CanvasEvent::PointerDown(pointer))=&trace.operation {
                let view=entity.read(cx);
                let world=trace.viewport.screen_to_world(pointer.position);
                let targets=trace.events.iter().filter(|e|e.kind==EventKind::PointerDown)
                    .map(|e|e.target.to_string()).collect::<Vec<_>>();
                this.last_pointer=format!("world {:.1},{:.1}; routed {:?}",world.x,world.y,targets);
                let record=json!({"kind":"actual_native_pointer_down","session_token":this.burst_token,
                    "entity":format!("{:?}",entity.entity_id()),"canvas_screen":pointer.position,
                    "world":world,"trace_viewport":trace.viewport,"routed_targets":targets,
                    "pointer_tolerance_screen":4.,"pointer_tolerance_world":4./trace.viewport.zoom,
                    "presented_hit_ids":view.presented_geometry().map(|s|hit_ids_with_options(s,world,HitTestOptions {
                        tolerance:4./trace.viewport.zoom,include_locked:true,..Default::default()})),
                    "strict_presented_hit_ids":view.presented_geometry().map(|s|hit_ids(s,world)),
                    "router_errors":trace.errors});
                println!("{record}");bounded_push(&mut this.inputs,record);cx.notify();
            }
        });
        (observe, input)
    }
    fn new(session: PathBuf, cx: &mut Context<Self>) -> Self {
        let view = Self::new_view(Case::Uniform, cx).expect("initial fixture construction failed");
        let (observe, input) = Self::subscriptions(&view, cx);
        Self {
            view,
            case: Case::Uniform,
            session,
            status: "Wait for pending=false, then Record and click the visible ink.".into(),
            last_pointer: "no native click yet".into(),
            sequence: 0,
            actions: vec![],
            inputs: vec![],
            undo: vec![],
            redo: vec![],
            constraints: false,
            burst_token: 0,
            burst_left: 0,
            burst_queued: false,
            actual_dpr: None,
            _observe: observe,
            _input: input,
        }
    }
    fn log(&mut self, value: Value) {
        println!("{value}");
        bounded_push(&mut self.actions, value);
    }
    fn stop_burst(&mut self) {
        self.burst_token += 1;
        self.burst_left = 0;
        self.burst_queued = false;
    }
    fn preset(&mut self, case: Case, cx: &mut Context<Self>) {
        self.stop_burst();
        let old = self.view.entity_id();
        match Self::new_view(case, cx) {
            Ok(view) => {
                let (observe, input) = Self::subscriptions(&view, cx);
                self._observe = observe;
                self._input = input;
                self.view = view;
                self.case = case;
                self.undo.clear();
                self.redo.clear();
                self.inputs.clear();
                self.constraints = false;
                self.last_pointer = "no native click in this fresh session".into();
                self.status =
                    "Fresh readonly session; wait for the current presentation, then record."
                        .into();
                self.log(json!({"kind":"preset","case":case.label(),"old_entity":format!("{old:?}"),
                    "new_entity":format!("{:?}",self.view.entity_id()),"retired_token_guard":self.burst_token,
                    "readonly":!self.view.read(cx).is_editable(),"history":"new editor"}));
            }
            Err(e) => self.status = format!("Preset failed: {e}"),
        }
        cx.notify();
    }
    fn nav(&mut self, label: &str, command: SceneCommand, cx: &mut Context<Self>) {
        let before = canonical(self.view.read(cx).editor.scene());
        let revision = self.view.read(cx).editor.engine.revision();
        let old_viewport = self.view.read(cx).editor.viewport;
        let result = self.view.update(cx, |v, cx| v.execute_command(command, cx));
        let v = self.view.read(cx);
        let source_same =
            before == canonical(v.editor.scene()) && revision == v.editor.engine.revision();
        let unchanged_on_error = result.is_ok() || old_viewport == v.editor.viewport;
        let event = json!({"kind":"navigation","action":label,"typed_error":result.as_ref().err().map(|e|format!("{e:?}")),
            "source_and_revision_unchanged":source_same,"rejected_viewport_unchanged":unchanged_on_error,
            "viewport_before":old_viewport,"viewport_after":v.editor.viewport,
            "presented_available_immediately":v.presented_geometry().is_some(),"pending_immediately":v.compositing_pending});
        self.status = format!(
            "{label}: {}; source/revision same={source_same}; rejection atomic={unchanged_on_error}",
            result
                .as_ref()
                .map(|_| "accepted".to_owned())
                .unwrap_or_else(|e| format!("{e:?}"))
        );
        self.log(event);
        cx.notify();
    }
    fn zoom(&mut self, z: f32, cx: &mut Context<Self>) {
        self.nav(
            &format!("Zoom {z}"),
            SceneCommand::Zoom {
                position: P::new(40., 12.),
                zoom: z,
            },
            cx,
        );
    }
    fn home(&mut self, cx: &mut Context<Self>) {
        self.stop_burst();
        self.nav(
            "SetViewport home",
            SceneCommand::SetViewport {
                viewport: Viewport {
                    pan: P::new(40., 12.),
                    ..Default::default()
                },
            },
            cx,
        );
    }
    fn fit(&mut self, cx: &mut Context<Self>) {
        self.stop_burst();
        self.nav(
            "Fit current candidate context",
            SceneCommand::Fit {
                canvas: S::new(CANVAS_W, CANVAS_H),
                padding: 24.,
            },
            cx,
        );
    }
    fn constraints(&mut self, cx: &mut Context<Self>) {
        self.stop_burst();
        let enable = !self.constraints;
        let result = self.view.update(cx, |v, cx| {
            if enable {
                v.install_plugin(
                    SceneViewportConstraintsPlugin {
                        constraints: SceneViewportConstraints {
                            min_zoom: Some(0.5),
                            max_zoom: Some(1.5),
                            bounds: None,
                        },
                    },
                    cx,
                )
            } else {
                v.uninstall_plugin("viewport-constraints", cx)
            }
        });
        if result.is_ok() {
            self.constraints = enable;
        }
        self.status = format!(
            "Constraints .5..1.5: {}; {:?}",
            self.constraints,
            result.err()
        );
        self.log(json!({"kind":"constraint_plugin","enabled":self.constraints}));
        cx.notify();
    }
    fn edit(&mut self, cx: &mut Context<Self>) {
        self.stop_burst();
        if self.undo.len() >= 16 {
            self.status = "16 host edits reached; use a fresh preset.".into();
            cx.notify();
            return;
        }
        let before = canonical(self.view.read(cx).editor.scene());
        let case = self.case;
        let result = self.view.update(cx, |v, cx| {
            let r = v.editor.transaction(|s| host_edit(s, case));
            cx.notify();
            r
        });
        let after = canonical(self.view.read(cx).editor.scene());
        if result.is_ok()
            && let (Ok(before), Ok(after)) = (&before, &after)
        {
            self.undo.push((before.clone(), after.clone()));
            self.redo.clear();
        }
        self.status = format!(
            "Host edit: {:?}; one transaction changes width, numeric threshold and parent X scale.",
            result
        );
        self.log(
            json!({"kind":"host_edit","typed_error":result.err().map(|e|format!("{e:?}")),
            "canonical_changed":before!=after,"readonly":!self.view.read(cx).is_editable()}),
        );
        cx.notify();
    }
    fn history(&mut self, redo: bool, cx: &mut Context<Self>) {
        self.stop_burst();
        let pair = if redo {
            self.redo.last().cloned()
        } else {
            self.undo.last().cloned()
        };
        let Some(pair) = pair else {
            self.status = "No matching fixture history entry.".into();
            cx.notify();
            return;
        };
        let changed = self.view.update(cx, |v, cx| {
            let changed = if redo {
                v.editor.redo()
            } else {
                v.editor.undo()
            };
            cx.notify();
            changed
        });
        let expected = if redo { &pair.1 } else { &pair.0 };
        let exact = canonical(self.view.read(cx).editor.scene()).as_ref() == Ok(expected);
        if changed && exact {
            if redo {
                self.redo.pop();
                self.undo.push(pair);
            } else {
                self.undo.pop();
                self.redo.push(pair);
            }
        }
        self.status = format!(
            "{}: changed={changed}; exact canonical JSON={exact}",
            if redo { "Redo" } else { "Undo" }
        );
        self.log(
            json!({"kind":"history","redo":redo,"changed":changed,"exact_canonical_json":exact}),
        );
        cx.notify();
    }
    fn reject(&mut self, cx: &mut Context<Self>) {
        self.stop_burst();
        if self.case != Case::Open || self.constraints {
            self.status =
                "Choose fresh E: Open, with constraints off, for the precision rejection.".into();
            cx.notify();
            return;
        }
        // Accepted current view remains untouched if the actual candidate's
        // aligned fixed width (8/1024) is below the existing 1/64 limit.
        let candidate = Viewport {
            pan: P::ZERO,
            zoom: 1024.,
            min_zoom: 0.1,
            max_zoom: 2048.,
        };
        self.nav(
            "Reject aligned zoom 1024",
            SceneCommand::SetViewport {
                viewport: candidate,
            },
            cx,
        );
        let v = self.view.read(cx);
        self.log(json!({"kind":"rejection_expectation","expected_variant":"InvalidInput(scene validation)",
            "actual_context_zoom":v.editor.viewport.zoom,"candidate_zoom":1024.,
            "candidate_effective_aligned_width":8./1024.,"minimum_aligned_width":1./64.}));
    }
    fn burst(&mut self, cx: &mut Context<Self>) {
        self.stop_burst();
        self.burst_left = 8;
        self.status =
            "Eight frame-spaced zoom commands; final zoom 1. Reset retires every queued callback."
                .into();
        cx.notify();
    }
    fn advance_burst(&mut self, token: u64, cx: &mut Context<Self>) -> bool {
        // Must run before touching any queue flag, viewport, action log or view.
        if token != self.burst_token {
            return false;
        }
        self.burst_queued = false;
        if self.burst_left == 0 {
            return false;
        }
        self.burst_left -= 1;
        let z = if self.burst_left == 0 {
            1.
        } else if self.burst_left.is_multiple_of(2) {
            0.5
        } else {
            2.
        };
        self.zoom(z, cx);
        true
    }
    fn record(&mut self, cx: &mut Context<Self>) {
        if self.sequence == MAX_RECORDS {
            self.status = "256 records reached; start a new evidence run.".into();
            cx.notify();
            return;
        }
        self.sequence += 1;
        let view = self.view.read(cx);
        let receipt = json!({"kind":"physical_session_diagnostics","sequence":self.sequence,
            "live":snapshot(view,self.case),"actual_window_dpr":self.actual_dpr,
            "actual_native_input":self.inputs,"action_observations":self.actions,
            "canonical_document":canonical(view.editor.scene()).ok().and_then(|s|serde_json::from_str::<Value>(&s).ok()),
            "native_visual_verdict":"unassessed; pair with original screenshot and physical-row record",
            "native_framebuffer_captured":false});
        let path = self.session.join(format!("{:03}-live.json", self.sequence));
        self.status = match json_new(&path, &receipt) {
            Ok(()) => format!(
                "Wrote {}",
                path.file_name().unwrap_or_default().to_string_lossy()
            ),
            Err(e) => format!("Receipt write failed: {e}"),
        };
        println!("{receipt}");
        cx.notify();
    }
}

impl Render for Lab {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.actual_dpr = Some(window.scale_factor());
        if self.burst_left > 0 && !self.burst_queued {
            self.burst_queued = true;
            let token = self.burst_token;
            let weak = cx.entity().downgrade();
            window.on_next_frame(move |_, cx| {
                let _ = weak.update(cx, |this, cx| this.advance_burst(token, cx));
            });
            window.request_animation_frame();
        }
        let v = self.view.read(cx);
        let live = format!(
            "Readonly={} | zoom {:.3} | DPR {:.2} | pending={} | presented={} | render/CPU issues={}/{}",
            !v.is_editable(),
            v.editor.viewport.zoom,
            self.actual_dpr.unwrap_or(0.),
            v.compositing_pending,
            v.presented_geometry().is_some(),
            v.diagnostics.len(),
            v.composite_diagnostics.len()
        );
        let first_bounds = v.presented_geometry().map(|s| bounds(s, "native-0-ink"));
        let bounds_line = format!(
            "Actual presented world bound native-0-ink: {}",
            first_bounds.unwrap_or(Value::Null)
        );
        let status = if self.status.chars().count() > 220 {
            format!(
                "{}… (full action in Record)",
                self.status.chars().take(220).collect::<String>()
            )
        } else {
            self.status.clone()
        };
        div().size_full().flex().flex_col().bg(rgb(0x111823)).text_color(rgb(0xffffff)).text_sm()
            .child(div().h(px(36.)).flex_shrink_0().px_2().flex().items_center().gap_1()
                .child("Scale-fixed strokes")
                .child(ControlButton::new("a","A: Width").on_click(cx.listener(|s,_,_,cx|s.preset(Case::Uniform,cx))))
                .child(ControlButton::new("b","B: Axes").on_click(cx.listener(|s,_,_,cx|s.preset(Case::Axes,cx))))
                .child(ControlButton::new("c","C: Ancestry").on_click(cx.listener(|s,_,_,cx|s.preset(Case::Hierarchy,cx))))
                .child(ControlButton::new("d","D: Dashes").on_click(cx.listener(|s,_,_,cx|s.preset(Case::Dashes,cx))))
                .child(ControlButton::new("e","E: Open").on_click(cx.listener(|s,_,_,cx|s.preset(Case::Open,cx))))
                .child(ControlButton::new("f","F: Layers").on_click(cx.listener(|s,_,_,cx|s.preset(Case::Layers,cx))))
                .child(ControlButton::new("record","Record").on_click(cx.listener(|s,_,_,cx|s.record(cx)))))
            .child(div().h(px(36.)).flex_shrink_0().px_2().flex().items_center().gap_1()
                .child(ControlButton::new("half","Zoom .5").on_click(cx.listener(|s,_,_,cx|{s.stop_burst();s.zoom(0.5,cx);})))
                .child(ControlButton::new("one","Zoom 1").on_click(cx.listener(|s,_,_,cx|{s.stop_burst();s.zoom(1.,cx);})))
                .child(ControlButton::new("two","Zoom 2").on_click(cx.listener(|s,_,_,cx|{s.stop_burst();s.zoom(2.,cx);})))
                .child(ControlButton::new("fit","Fit").on_click(cx.listener(|s,_,_,cx|s.fit(cx))))
                .child(ControlButton::new("home","Home").on_click(cx.listener(|s,_,_,cx|s.home(cx))))
                .child(ControlButton::new("constraints",format!("Limits: {}",self.constraints)).on_click(cx.listener(|s,_,_,cx|s.constraints(cx))))
                .child(ControlButton::new("reject","Reject 1024").on_click(cx.listener(|s,_,_,cx|s.reject(cx)))))
            .child(div().h(px(36.)).flex_shrink_0().px_2().flex().items_center().gap_1()
                .child(ControlButton::new("edit","Host edit").on_click(cx.listener(|s,_,_,cx|s.edit(cx))))
                .child(ControlButton::new("undo","Undo").on_click(cx.listener(|s,_,_,cx|s.history(false,cx))))
                .child(ControlButton::new("redo","Redo").on_click(cx.listener(|s,_,_,cx|s.history(true,cx))))
                .child(ControlButton::new("burst","8 zoom frames").on_click(cx.listener(|s,_,_,cx|s.burst(cx))))
                .child(format!("{} | top native, bottom CPU | columns left to right",self.case.label())))
            .child(div().w(px(CANVAS_W)).h(px(CANVAS_H)).flex_shrink_0().mx_2().overflow_hidden().child(self.view.clone()))
            .child(div().h(px(190.)).flex_shrink_0().px_3().py_2().text_xs().flex().flex_col().gap_1()
                .child(self.case.hint()).child(live).child(bounds_line)
                .child(format!("Native click: {}",self.last_pointer)).child(status)
                .child("Record stores live diagnostics and actual input targets. Native visuals need original screenshots and an explicit physical-row verdict.")
                .child("CPU files: separate --cpu-tour. Pure helper: --headless-tour. No GPU readback, font bridge, resource loading or backend pixel-identity claim."))
    }
}

// Independent canonical CPU exports, never the SceneView framebuffer.
fn cpu_tour(session: &Path) -> Result<(), String> {
    let source = document(Case::Uniform).map_err(|e| e.to_string())?;
    let before = canonical(&source)?;
    let mut receipts = vec![];
    for (index, (scale, dpr)) in [(1., 1.), (0.5, 1.), (2., 1.), (1., 2.), (2., 2.)]
        .into_iter()
        .enumerate()
    {
        let request = SceneExportRequest {
            world_bounds: SceneBounds::new(P::ZERO, S::new(WORLD_W, WORLD_H)),
            scale,
            pixel_ratio: dpr,
            background: BG,
            format: SceneExportFormat::Png,
        };
        let plan = SceneExportPlan::new(&source, request, CompositeLimits::default())
            .map_err(|e| format!("{e:?}"))?;
        let result = plan
            .encode_with_inputs(
                &ImageInputs::default(),
                &TextInputs::default(),
                SceneExportPreparation::default(),
                &AtomicBool::new(false),
            )
            .map_err(|e| format!("{e:?}"))?;
        let png = session.join(format!("{index:02}-scale-{scale}-dpr-{dpr}.png"));
        write_new(&png, &result.bytes)?;
        let decoded = image::load_from_memory(&result.bytes)
            .map_err(|e| e.to_string())?
            .to_rgba8();
        let factor = scale * dpr;
        // Actual decoded pixel rows around the long horizontal segment. These
        // samples are evidence, not a guessed antialiasing pixel-count oracle.
        let profiles=(0..3).map(|col|{
            let x=((18.+col as f32*146.+45.)*factor).floor() as u32;
            let middle=(34.*factor).floor() as i32;
            let radius=(18.*factor.max(dpr)).ceil() as i32;
            let samples=((middle-radius).max(0)..=(middle+radius).min(decoded.height() as i32-1))
                .map(|y|json!({"y":y,"rgba":decoded.get_pixel(x.min(decoded.width()-1),y as u32).0})).collect::<Vec<_>>();
            json!({"column":col,"x":x,"samples":samples})
        }).collect::<Vec<_>>();
        let expected_size = [
            (WORLD_W * factor).ceil() as u32,
            (WORLD_H * factor).ceil() as u32,
        ];
        let unchanged = before == canonical(&source)?;
        let receipt = json!({"kind":"independent_cpu_export","logical_scale":scale,"synthetic_dpr":dpr,
            "path":png,"encoded_byte_count":result.bytes.len(),"encoded_fnv1a64":fingerprint(&result.bytes),
            "decoded_size":[decoded.width(),decoded.height()],"expected_size":expected_size,
            "decoded_rgba_fnv1a64":fingerprint(decoded.as_raw()),"profiles":profiles,
            "expected_ideal_horizontal_thickness_physical_px":[8.*dpr,8.*scale.min(1.)*dpr,8.*scale*dpr],
            "source_unchanged":unchanged,"native_application_created":false,
            "native_framebuffer_captured":false});
        json_new(&session.join(format!("{index:02}-cpu.json")), &receipt)?;
        if !unchanged || [decoded.width(), decoded.height()] != expected_size {
            return Err("CPU dimensions/source invariance failed".into());
        }
        receipts.push(receipt);
    }
    json_new(
        &session.join("summary.json"),
        &json!({"kind":"cpu_export_tour","exports":receipts,
        "scope":"decoded bytes and dimensions only; physical profile judgment remains explicit",
        "native_acceptance":"not performed"}),
    )
}

// This helper runs before constructing a GPUI Application. It tests fixture
// documents and public host commands. It cannot certify rendering or callbacks.
fn headless_tour(session: &Path) -> Result<(), String> {
    let mut rows = vec![];
    for case in Case::ALL {
        let source = document(case).map_err(|e| e.to_string())?;
        let original = canonical(&source)?;
        for zoom in [0.5, 1., 2.] {
            let resolved = source.resolved_for_view(zoom).map_err(|e| e.to_string())?;
            let detail=(0..3).map(|col|{
                let id=key("native",col,"ink");
                let node=resolved.get(&id.as_str().into()).ok_or("missing resolved fixture object")?;
                let expected=match case {
                    Case::Uniform=>Some([8./zoom,8./zoom.max(1.),8.][col]),
                    Case::Axes if zoom==1.=>Some([4.,8.,16.][col]),
                    Case::Hierarchy if zoom==1.=>Some(4.),
                    _=>None,
                };
                if let Some(expected)=expected {
                    let actual=node.object.style.stroke.as_ref().ok_or("missing fixture stroke")?.width;
                    if (actual-expected).abs()>0.0001 {
                        return Err(format!("{case:?} column {col}: resolved width {actual}, expected {expected}"));
                    }
                }
                Ok(json!({"id":id,"strokes":strokes(&node.object),"world_bounds":bounds(&resolved,&id)}))
            }).collect::<Result<Vec<Value>,String>>()?;
            rows.push(json!({"case":case.label(),"zoom":zoom,"resolved":detail,
                "source_unchanged":canonical(&source)?==original}));
        }
    }
    let mut editor = SceneEditor::new(document(Case::Open).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let mut host = ScenePluginHost::with_defaults();
    host.set_editable(false, &mut editor);
    let initial = canonical(editor.scene())?;
    let denied = host.execute(
        &mut editor,
        SceneCommand::LoadScene(document(Case::Uniform).map_err(|e| e.to_string())?),
    );
    if !matches!(
        denied,
        Err(InteractionError::CapabilityDenied(Capability::Edit))
    ) || canonical(editor.scene())? != initial
    {
        return Err(format!("readonly LoadScene contract failed: {denied:?}"));
    }
    editor
        .transaction(|s| host_edit(s, Case::Open))
        .map_err(|e| e.to_string())?;
    let edited = canonical(editor.scene())?;
    for zoom in [0.5, 1., 2.] {
        host.execute(
            &mut editor,
            SceneCommand::Zoom {
                position: P::ZERO,
                zoom,
            },
        )
        .map_err(|e| e.to_string())?;
        if canonical(editor.scene())? != edited {
            return Err("navigation changed source".into());
        }
    }
    if !editor.undo() || canonical(editor.scene())? != initial || editor.undo() {
        return Err("host edit was not one exact Undo; navigation may have added history".into());
    }
    if !editor.redo() || canonical(editor.scene())? != edited || editor.redo() {
        return Err("Redo contract failed".into());
    }
    let revision = editor.engine.revision();
    let old_viewport = editor.viewport;
    let rejected = host.execute(
        &mut editor,
        SceneCommand::SetViewport {
            viewport: Viewport {
                zoom: 1024.,
                max_zoom: 2048.,
                ..Default::default()
            },
        },
    );
    let atomic = matches!(&rejected, Err(InteractionError::InvalidInput(_)))
        && editor.viewport == old_viewport
        && editor.engine.revision() == revision
        && canonical(editor.scene())? == edited;
    if !atomic {
        return Err(format!("precision rejection not atomic: {rejected:?}"));
    }
    host.execute(
        &mut editor,
        SceneCommand::SetViewport {
            viewport: Viewport::default(),
        },
    )
    .map_err(|e| e.to_string())?;
    host.execute(
        &mut editor,
        SceneCommand::Fit {
            canvas: S::new(CANVAS_W, CANVAS_H),
            padding: 24.,
        },
    )
    .map_err(|e| e.to_string())?;
    let fitted = editor
        .scene()
        .visible_bounds_for_view(editor.viewport.zoom)
        .map_err(|e| e.to_string())?;
    if let Some(b) = fitted {
        let screen = editor.viewport.world_rect_to_screen(b);
        if screen.min_x() < 23.99
            || screen.min_y() < 23.99
            || screen.max_x() > CANVAS_W - 23.99
            || screen.max_y() > CANVAS_H - 23.99
        {
            return Err("Fit bounds do not fit at the returned candidate zoom".into());
        }
    }
    host.install(SceneViewportConstraintsPlugin {
        constraints: SceneViewportConstraints {
            min_zoom: Some(0.5),
            max_zoom: Some(1.5),
            bounds: None,
        },
    })
    .map_err(|e| e.to_string())?;
    host.execute(
        &mut editor,
        SceneCommand::Fit {
            canvas: S::new(CANVAS_W, CANVAS_H),
            padding: 24.,
        },
    )
    .map_err(|e| e.to_string())?;
    let constrained = editor.viewport;
    if !(0.5..=1.5).contains(&constrained.zoom) {
        return Err("constraint Fit range failed".into());
    }
    let constrained_bounds = editor
        .scene()
        .visible_bounds_for_view(constrained.zoom)
        .map_err(|e| e.to_string())?;
    if let Some(b) = constrained_bounds {
        let screen = constrained.world_rect_to_screen(b);
        if screen.min_x() < 23.99
            || screen.min_y() < 23.99
            || screen.max_x() > CANVAS_W - 23.99
            || screen.max_y() > CANVAS_H - 23.99
        {
            return Err("constrained Fit bounds do not fit at the returned zoom".into());
        }
    }
    json_new(
        &session.join("headless.json"),
        &json!({"kind":"headless_fixture_helper","rows":rows,
        "readonly_load_scene_denied":true,"one_host_edit_exact_undo_redo":true,
        "precision_rejection":format!("{rejected:?}"),"rejected_source_revision_viewport_unchanged":atomic,
        "candidate_context_fit_bounds":fitted,"constrained_fit":constrained,"constrained_candidate_bounds":constrained_bounds,
        "native_application_created":false,"physical_input_exercised":false,"native_acceptance":"not performed"}),
    )
}

fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    let cpu = args.iter().any(|s| s == "--cpu-tour");
    let helper = args.iter().any(|s| s == "--headless-tour");
    if cpu && helper {
        eprintln!("Choose only one helper mode");
        std::process::exit(2);
    }
    let kind = if cpu {
        "cpu"
    } else if helper {
        "headless"
    } else {
        "physical"
    };
    let session = match session(kind) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    if cpu || helper {
        let result = if cpu {
            cpu_tour(&session)
        } else {
            headless_tour(&session)
        };
        if let Err(error) = result {
            let _ = json_new(
                &session.join("failure.json"),
                &json!({"kind":kind,"error":error,"native_acceptance":"not performed"}),
            );
            eprintln!(
                "{kind} failed: {error}; partial evidence preserved at {}",
                session.display()
            );
            std::process::exit(1);
        }
        println!(
            "{kind} evidence: {} (no native acceptance)",
            session.display()
        );
        return;
    }
    println!(
        "Physical-session diagnostics: {}; screenshots remain separate",
        session.display()
    );
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(1060.), px(790.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                |window, cx| {
                    window.set_window_title("gpui-flow · bounded scale-fixed strokes");
                    cx.new(|cx| Lab::new(session, cx))
                },
            )
            .expect("native window creation failed");
            cx.activate(true);
        });
}
