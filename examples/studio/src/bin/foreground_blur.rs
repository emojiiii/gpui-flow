//! Bounded APP-04 foreground blur lab. Source, CPU helpers and physical QA are separate.
//! Native receipts, original operator screenshots and CPU exports are distinct.
//! No framebuffer access, renderer backdoor, resources, new dependencies or fonts.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, SceneCompositing, SceneInputOperation, SceneView, SceneViewEvent,
    composite::{
        CanvasBackdrop, CompositeError, CompositeLimits, CpuFrame, DomainSource, ImageInputs,
        PixelBudget, RasterViewport, SceneExportFormat, SceneExportPlan, SceneExportPreparation,
        SceneExportRequest, SceneUnit, TextInputs, capture_subtree, owned_canvas, rasterize,
        scene_units,
    },
    core::Viewport,
    editor::SceneEditor,
    engine::SceneEngine,
    interaction::{
        Capability, InteractionError,
        scene::{SceneCommand, ScenePluginHost},
    },
    scene::{
        Affine, BlendMode, Bounds as B, Color, DisplayObject, EraserMode, EventKind, HitPolicy,
        HitTestOptions, MaskMode, Point as P, Scene, SceneError, Shadow, Shape, Size as S, Stroke,
        Style, display,
    },
};
use serde_json::{Value, json};
use std::{
    io::Write,
    path::PathBuf,
    sync::atomic::AtomicBool,
    time::{SystemTime, UNIX_EPOCH},
};

const WORLD_W: f32 = 180.;
const WORLD_H: f32 = 110.;
const CANVAS_W: f32 = 440.;
const CANVAS_H: f32 = 220.;
const RED: Color = Color::rgb(255, 0, 0);
const GREEN: Color = Color::rgb(0, 255, 0);
const BLUE: Color = Color::rgb(0, 0, 255);
const MAGENTA: Color = Color::rgb(255, 0, 255);
const MAX_RECORDS: u32 = 192;
const SESSION_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Case {
    Leaves,
    Offscreen,
    Nested,
    Alpha,
    Reject,
}
impl Case {
    const ALL: [Self; 5] = [
        Self::Leaves,
        Self::Offscreen,
        Self::Nested,
        Self::Alpha,
        Self::Reject,
    ];
    fn label(self) -> &'static str {
        match self {
            Self::Leaves => "A Leaf / gray / opacity",
            Self::Offscreen => "B Outside source / Frame shadow",
            Self::Nested => "C Nested parent / child crop",
            Self::Alpha => "D Soft mask / Pixel / Path",
            Self::Reject => "E Unsafe PassThrough",
        }
    }
    fn ids(self) -> &'static [&'static str] {
        match self {
            Self::Leaves => &["color", "focus"],
            Self::Offscreen => &["edge", "frame-child"],
            Self::Nested => &["parent", "child"],
            Self::Alpha => &["mask", "pixel", "path"],
            Self::Reject => &["focus"],
        }
    }
    fn hint(self) -> &'static str {
        match self {
            Self::Leaves => {
                "Left: red reference. Middle: red blur. Right: complete own colored shadows, then blur, gray 1 and opacity .5. Variant changes right opacity. Halo must not become a hit target."
            }
            Self::Offscreen => {
                "Red source lies left of viewport; only its halo enters. Right Frame contains an outside child whose halo enters its clip; its green parent shadow projects the entering halo six units right. Variant shifts edge source inside."
            }
            Self::Nested => {
                "One small clipped parent, nested child blur, own magenta stroke and green shadow. Variant toggles Frame/Box own child clipping. Compare generous/tight CPU crops separately."
            }
            Self::Alpha => {
                "Blue half-alpha mask softens red coverage; blue half-alpha Pixel source softens erasure; Path remains a hard geometric cut regardless of its valid blur. Later green survives in the cut columns."
            }
            Self::Reject => {
                "Full-opacity PassThrough + positive blur + Multiply descendant must reject with UnsupportedBlurBackdrop. Variant selects Normal for recovery. No unfiltered substitute counts as success."
            }
        }
    }
    fn probes(self) -> &'static [(&'static str, f32, f32)] {
        match self {
            Self::Leaves => &[
                ("reference", 35., 36.),
                ("color", 65., 36.),
                ("color-halo", 59., 36.),
                ("focus", 95., 36.),
                ("focus-halo", 89., 36.),
            ],
            Self::Offscreen => &[
                ("entering-edge-halo", 0., 44.),
                ("outside-child-halo", 80., 43.),
                ("frame-shadow", 86., 43.),
            ],
            Self::Nested => &[
                ("child", 48., 47.),
                ("nested-halo", 39., 47.),
                ("outside-ancestor", 35., 34.),
            ],
            Self::Alpha => &[
                ("mask-center", 37., 37.),
                ("mask-halo", 33., 37.),
                ("pixel-center", 77., 37.),
                ("pixel-halo", 73., 37.),
                ("path-center", 117., 37.),
                ("path-outside", 113., 37.),
                ("later", 80., 47.),
            ],
            Self::Reject => &[
                ("multiply", 44., 44.),
                ("external", 32., 32.),
                ("later", 63., 63.),
            ],
        }
    }
}
fn add(s: &mut Scene, o: DisplayObject, parent: Option<&str>) -> Result<(), SceneError> {
    s.insert(o, parent.map(Into::into), None).map(|_| ())
}
fn rect(id: &str, x: f32, y: f32, w: f32, h: f32, color: Color) -> DisplayObject {
    DisplayObject::new(id, display::Rect::new(w, h))
        .with_transform(Affine::translation(x, y))
        .with_style(Style::fill(color))
}
fn group(id: &str, x: f32, y: f32, blur: f32) -> DisplayObject {
    let mut o =
        DisplayObject::new(id, display::Group::new()).with_transform(Affine::translation(x, y));
    o.style.blend_mode = BlendMode::Normal;
    o.style.blur = blur;
    o.hit_policy = HitPolicy::None;
    o
}
fn document(case: Case) -> Result<Scene, SceneError> {
    let mut s = Scene::new();
    s.data = json!({"fixture":"foreground-blur","case":format!("{case:?}"),"contract":"bounded-foreground-v1"});
    match case {
        Case::Leaves => {
            add(&mut s, rect("reference", 30., 30., 10., 12., RED), None)?;
            let mut color = rect("color", 60., 30., 10., 12., RED);
            color.style.blur = 1.;
            add(&mut s, color, None)?;
            let mut focus = rect("focus", 90., 30., 10., 12., RED);
            focus.style.blur = 1.;
            focus.style.grayscale = 1.;
            focus.style.opacity = 0.5;
            focus.style.shadow = Some(Shadow::new(3., 2., 0., MAGENTA));
            focus.style.inner_shadow_layers = vec![Shadow::new(2., 1., 0., BLUE)];
            add(&mut s, focus, None)?;
        }
        Case::Offscreen => {
            let mut edge = rect("edge", -7., 40., 6., 8., RED);
            edge.style.blur = 1.;
            add(&mut s, edge, None)?;
            let mut frame = DisplayObject::new("frame", display::Frame::new(20., 20.))
                .with_transform(Affine::translation(80., 35.));
            frame.style.fill = None;
            frame.style.stroke = None;
            frame.style.shadow = Some(Shadow::new(6., 0., 0., GREEN));
            frame.hit_policy = HitPolicy::None;
            add(&mut s, frame, None)?;
            let mut child = rect("frame-child", -7., 4., 6., 8., RED);
            child.style.blur = 1.;
            add(&mut s, child, Some("frame"))?;
        }
        Case::Nested => {
            let mut clip = DisplayObject::new("clip", display::Frame::new(28., 28.).rounded(4.))
                .with_transform(Affine::translation(35.25, 34.25));
            clip.style.fill = None;
            clip.hit_policy = HitPolicy::None;
            add(&mut s, clip, None)?;
            let mut parent =
                DisplayObject::new("parent", display::Frame::new(16., 14.).rounded(2.))
                    .with_transform(Affine::translation(5., 5.));
            parent.style = Style {
                fill: Some(BLUE.into()),
                blur: 0.5,
                blend_mode: BlendMode::Normal,
                stroke: Some(Stroke::new(MAGENTA, 1.)),
                shadow: Some(Shadow::new(2., 1., 0., GREEN)),
                ..Default::default()
            };
            add(&mut s, parent, Some("clip"))?;
            let mut child = rect("child", 3., 3., 16., 8., RED);
            child.style.blur = 0.5;
            add(&mut s, child, Some("parent"))?;
        }
        Case::Alpha => {
            for (id, x) in [
                ("mask-parent", 30.),
                ("pixel-parent", 70.),
                ("path-parent", 110.),
            ] {
                add(&mut s, group(id, x, 30., 0.), None)?;
                if id == "mask-parent" {
                    let mut m = rect("mask", 4., 2., 6., 10., Color::rgba(0, 0, 255, 128));
                    m.style.blur = 1.;
                    m.mask = MaskMode::Alpha;
                    m.hit_policy = HitPolicy::None;
                    add(&mut s, m, Some(id))?;
                }
                add(
                    &mut s,
                    rect(&format!("{id}-red"), 0., 0., 18., 20., RED),
                    Some(id),
                )?;
                if id != "mask-parent" {
                    let cut_id = if id == "pixel-parent" {
                        "pixel"
                    } else {
                        "path"
                    };
                    let mut cut = rect(cut_id, 4., 2., 6., 10., Color::rgba(0, 0, 255, 128));
                    cut.style.blur = 1.;
                    cut.hit_policy = HitPolicy::Geometry;
                    cut.eraser = if cut_id == "pixel" {
                        EraserMode::Pixel
                    } else {
                        EraserMode::Path
                    };
                    add(&mut s, cut, Some(id))?;
                }
                add(
                    &mut s,
                    rect(&format!("{id}-later"), 7., 15., 6., 4., GREEN),
                    Some(id),
                )?;
            }
        }
        Case::Reject => {
            add(&mut s, rect("external", 30., 30., 38., 38., RED), None)?;
            let mut g = group("focus", 40., 40., 1.);
            g.style.blend_mode = BlendMode::PassThrough;
            add(&mut s, g, None)?;
            let mut child = rect("multiply", 0., 0., 10., 10., BLUE);
            child.style.blend_mode = BlendMode::Multiply;
            add(&mut s, child, Some("focus"))?;
            add(&mut s, rect("later", 60., 60., 6., 6., MAGENTA), None)?;
        }
    }
    s.validate()?;
    Ok(s)
}
fn amount(scene: &mut Scene, case: Case, value: f32) -> Result<(), SceneError> {
    for id in case.ids() {
        scene.update_object(&(*id).into(), |o| o.style.blur = value)?;
    }
    Ok(())
}
fn variant(scene: &mut Scene, case: Case) -> Result<(), SceneError> {
    match case {
        Case::Leaves => scene.update_object(&"focus".into(), |o| {
            o.style.opacity = if o.style.opacity == 0.5 { 1. } else { 0.5 }
        }),
        Case::Offscreen => scene.update_object(&"edge".into(), |o| {
            o.transform = Affine::translation(if o.transform.tx < 0. { 10. } else { -7. }, 40.)
        }),
        Case::Nested => scene.update_object(&"parent".into(), |o| {
            o.shape = if matches!(&o.shape, Shape::Frame(_)) {
                Shape::Box(display::Box::new(16., 14.).rounded(2.))
            } else {
                Shape::Frame(display::Frame::new(16., 14.).rounded(2.))
            }
        }),
        Case::Alpha => scene.update_object(&"path".into(), |o| {
            o.style.blur = if o.style.blur > 0. { 0. } else { 1. }
        }),
        Case::Reject => scene.update_object(&"focus".into(), |o| {
            o.style.blend_mode = if o.style.blend_mode == BlendMode::Normal {
                BlendMode::PassThrough
            } else {
                BlendMode::Normal
            }
        }),
    }
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
struct Evidence {
    path: PathBuf,
    bytes: usize,
    next: u32,
}
impl Evidence {
    fn new(kind: &str) -> Result<Self, String> {
        let root = PathBuf::from(std::env::var_os("FLOW_FOREGROUND_BLUR_EVIDENCE_DIR").ok_or(
            "Set FLOW_FOREGROUND_BLUR_EVIDENCE_DIR to an existing absolute evidence root",
        )?);
        if !root.is_absolute() || !root.is_dir() {
            return Err("Evidence root must exist and be absolute".into());
        }
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos();
        let path = root.join(format!(
            "foreground-blur-{kind}-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir(&path).map_err(|e| e.to_string())?;
        let mut out = Self {
            path,
            bytes: 0,
            next: 0,
        };
        out.json("manifest.json",&json!({"fixture":"foreground-blur","kind":kind,
            "native_acceptance":"operator assessment required; never set by this program",
            "native_framebuffer_captured":false,"no_runtime_budget_escalation":true,
            "maximum_session_bytes":SESSION_BYTES,"source":"record exact integrated source and executable hashes separately"}))?;
        out.write(
            "compiled-fixture-source.rs",
            include_bytes!("foreground_blur.rs"),
        )?;
        out.json("source-identity.json", &json!({"fixture_source_fnv1a64":fingerprint(include_bytes!("foreground_blur.rs")),
            "caller_source_revision":std::env::var("FLOW_FOREGROUND_BLUR_SOURCE_REVISION").ok(),
            "complete_repository_and_executable_sha256":"parent must record externally; not inferred from fixture source"}))?;
        Ok(out)
    }
    fn write(&mut self, name: &str, bytes: &[u8]) -> Result<(), String> {
        if self.bytes.saturating_add(bytes.len()) > SESSION_BYTES {
            return Err("8 MiB session evidence limit; existing files preserved".into());
        }
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(self.path.join(name))
            .map_err(|e| e.to_string())?;
        // A failed write remains named and is never overwritten or silently retried.
        self.bytes = self.bytes.saturating_add(bytes.len());
        f.write_all(bytes).map_err(|e| e.to_string())
    }
    fn json(&mut self, name: &str, v: &Value) -> Result<(), String> {
        self.write(
            name,
            &serde_json::to_vec_pretty(v).map_err(|e| e.to_string())?,
        )
    }
    fn event(&mut self, v: &Value) -> Result<(), String> {
        println!("{v}");
        if self.next >= MAX_RECORDS {
            return Err("192 evidence records reached; start a fresh invocation".into());
        }
        self.next += 1;
        self.json(&format!("{:03}-event.json", self.next), v)
    }
}
fn hits(s: &Scene, p: P, options: HitTestOptions) -> Value {
    match s.hit_test_all(p, options) {
        Ok(h) => json!(h.iter().map(|h| h.id.to_string()).collect::<Vec<_>>()),
        Err(e) => json!({"typed_error":format!("{e:?}")}),
    }
}
fn presented_state(v: &SceneView) -> Value {
    json!({"available":v.presented_geometry().is_some(),"pending":v.compositing_pending,"geometry_is_upload_proof":false,
        "public_geometry_effects":v.presented_geometry().map(|s|s.nodes().map(|n|json!({"id":n.object.id,
            "blur":n.object.style.blur,"grayscale":n.object.style.grayscale,"opacity":n.object.style.opacity})).collect::<Vec<_>>()),
        "composite_issues":v.composite_diagnostics.iter().map(|i|json!({"id":i.object,"typed_error":format!("{:?}",i.error)})).collect::<Vec<_>>(),
        "render_issues":format!("{:?}",v.diagnostics),"sticky_interaction_error":v.interaction_error})
}
fn snapshot(v: &SceneView, case: Case) -> Value {
    let scene = v.editor.scene();
    let probes=case.probes().iter().map(|(label,x,y)| {
        let p=P::new(*x,*y);json!({"label":label,"world":p,"screen_in_canvas":v.editor.viewport.world_to_screen(p),
            "strict_source_ids":hits(scene,p,HitTestOptions::default()),
            "strict_presented_ids":v.presented_geometry().map(|s|hits(s,p,HitTestOptions::default()))})
    }).collect::<Vec<_>>();
    json!({"case":case.label(),"readonly":!v.is_editable(),"engine_revision":v.editor.engine.revision(),
        "viewport":v.editor.viewport,"compositing_mode":format!("{:?}",v.compositing_mode),
        "source_fnv1a64":canonical(scene).map(|s|fingerprint(s.as_bytes())).ok(),
        "source_document":canonical(scene).ok().and_then(|s|serde_json::from_str::<Value>(&s).ok()),
        "presented":presented_state(v),"strict_geometric_probes":probes,
        "bounds":case.ids().iter().map(|id|json!({"id":id,"world_bounds":scene.world_bounds(&(*id).into()).ok()})).collect::<Vec<_>>(),
        "publication_generation":"not public; presented_geometry can return canonical early and does not prove a private CPU upload generation"})
}
struct Lab {
    view: Entity<SceneView>,
    case: Case,
    evidence: Evidence,
    status: String,
    last_pointer: String,
    undo: Vec<(String, String)>,
    redo: Vec<(String, String)>,
    token: u64,
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
            v.show_grid = false;
            v.theme.background = 0x17202b;
            v.compositing_mode = if case == Case::Reject {
                SceneCompositing::OwnedCanvas
            } else {
                SceneCompositing::Domains
            };
            v.set_editable(false, cx);
            v.set_input_tracing(true);
            v
        });
        view.update(cx, |v, cx| {
            v.execute_command(
                SceneCommand::SetViewport {
                    viewport: Viewport {
                        pan: P::ZERO,
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
        let input=cx.subscribe(view,|this:&mut Self,entity,event,cx| {
            if let SceneViewEvent::InputTrace(trace)=event
                && let SceneInputOperation::Canvas(gpui_flow::interaction::CanvasEvent::PointerDown(pointer))=&trace.operation {
                let v=entity.read(cx);let world=trace.viewport.screen_to_world(pointer.position);
                let targets=trace.events.iter().filter(|e|e.kind==EventKind::PointerDown).map(|e|e.target.to_string()).collect::<Vec<_>>();
                this.last_pointer=format!("world {:.1},{:.1} routed {:?}",world.x,world.y,targets);
                let record=json!({"kind":"actual_native_pointer_down","token":this.token,
                    "entity":format!("{:?}",entity.entity_id()),"canvas_screen":pointer.position,
                    "world":world,"viewport":trace.viewport,"routed_targets":targets,
                    "source_revision":v.editor.engine.revision(),"pending":v.compositing_pending,"geometry_is_upload_proof":false,
                    "pointer_tolerance_screen_px":4.,"pointer_tolerance_world":4./trace.viewport.zoom,
                    "tolerant_presented_ids":v.presented_geometry().map(|s|hits(s,world,HitTestOptions {tolerance:4./trace.viewport.zoom,include_locked:true,..Default::default()})),
                    "strict_presented_ids":v.presented_geometry().map(|s|hits(s,world,HitTestOptions::default())),"router_errors":trace.errors});
                this.log(record);cx.notify();
            }
        });
        (observe, input)
    }
    fn new(evidence: Evidence, cx: &mut Context<Self>) -> Self {
        let view = Self::new_view(Case::Leaves, cx).expect("initial fixture construction failed");
        let (observe, input) = Self::subscriptions(&view, cx);
        Self {
            view,
            case: Case::Leaves,
            evidence,
            status: "Wait for pending=false, inspect, click a primitive, then Record.".into(),
            last_pointer: "no native click yet".into(),
            undo: vec![],
            redo: vec![],
            token: 0,
            burst_left: 0,
            burst_queued: false,
            actual_dpr: None,
            _observe: observe,
            _input: input,
        }
    }
    fn log(&mut self, v: Value) {
        if let Err(e) = self.evidence.event(&v) {
            self.status = format!("Evidence write failed: {e}");
            eprintln!("{}", self.status);
        }
    }
    fn stop_burst(&mut self) {
        self.token = self.token.wrapping_add(1);
        self.burst_left = 0;
        self.burst_queued = false;
    }
    fn preset(&mut self, case: Case, cx: &mut Context<Self>) {
        let retired_left = self.burst_left;
        let old_entity = self.view.entity_id();
        self.stop_burst();
        match Self::new_view(case, cx) {
            Ok(view) => {
                let (observe, input) = Self::subscriptions(&view, cx);
                self.view = view;
                self._observe = observe;
                self._input = input;
                self.case = case;
                self.undo.clear();
                self.redo.clear();
                self.last_pointer = "no native click in this fresh entity".into();
                self.status =
                    "Fresh readonly entity and history; inspect current presentation.".into();
                self.log(json!({"kind":"preset","case":case.label(),"old_entity":format!("{old_entity:?}"),
                    "new_entity":format!("{:?}",self.view.entity_id()),"token":self.token,"retired_burst_steps_observed":retired_left,
                    "callback_cancellation_timing":"not inferred from button order","live":snapshot(self.view.read(cx),case)}));
            }
            Err(e) => {
                self.status = format!("Preset failed: {e}");
                self.log(json!({"kind":"preset_error","error":e}));
            }
        }
        cx.notify();
    }
    fn edit(
        &mut self,
        label: &str,
        change: impl FnOnce(&mut Scene) -> Result<(), SceneError>,
        cx: &mut Context<Self>,
    ) {
        if self.undo.len() >= 32 {
            self.status = "32 host edits reached; choose a fresh preset.".into();
            cx.notify();
            return;
        }
        let before = canonical(self.view.read(cx).editor.scene());
        let revision = self.view.read(cx).editor.engine.revision();
        let result = self.view.update(cx, |v, cx| {
            let r = v.editor.transaction(change);
            cx.notify();
            r
        });
        let v = self.view.read(cx);
        let after = canonical(v.editor.scene());
        if result.is_ok()
            && before != after
            && let (Ok(a), Ok(b)) = (&before, &after)
        {
            self.undo.push((a.clone(), b.clone()));
            self.redo.clear();
        }
        let record = json!({"kind":"host_edit","action":label,"error":result.as_ref().err().map(|e|format!("{e:?}")),
            "before_revision":revision,"after_revision":v.editor.engine.revision(),"before_source":before.as_ref().ok().map(|s|fingerprint(s.as_bytes())),
            "after_source":after.as_ref().ok().map(|s|fingerprint(s.as_bytes())),"source_changed":before!=after,
            "readonly":!v.is_editable(),"immediate_presented":presented_state(v)});
        self.status = format!("{label}: {:?}; inspect settled state with Record", result);
        self.log(record);
        cx.notify();
    }
    fn set_amount(&mut self, value: f32, cx: &mut Context<Self>) {
        self.stop_burst();
        let case = self.case;
        self.edit(
            &format!("blur {value}"),
            move |s| amount(s, case, value),
            cx,
        );
    }
    fn change_variant(&mut self, cx: &mut Context<Self>) {
        self.stop_burst();
        let case = self.case;
        self.edit("case variant", move |s| variant(s, case), cx);
    }
    fn history(&mut self, redo: bool, cx: &mut Context<Self>) {
        self.stop_burst();
        let pair = if redo {
            self.redo.last().cloned()
        } else {
            self.undo.last().cloned()
        };
        let Some(pair) = pair else {
            self.status = "No fixture history entry.".into();
            cx.notify();
            return;
        };
        let changed = self.view.update(cx, |v, cx| {
            let r = if redo {
                v.editor.redo()
            } else {
                v.editor.undo()
            };
            cx.notify();
            r
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
            "{} changed={changed}; exact canonical source={exact}",
            if redo { "Redo" } else { "Undo" }
        );
        self.log(json!({"kind":"history","redo":redo,"changed":changed,"source_exact":exact,"live":snapshot(self.view.read(cx),self.case)}));
        cx.notify();
    }
    fn invalid(&mut self, cx: &mut Context<Self>) {
        self.stop_burst();
        let id = self.case.ids()[0];
        let mut rows = vec![];
        for (visibility, hidden, opacity) in [("hidden", true, 1.), ("zero-opacity", false, 0.)] {
            for (label, value) in [
                ("negative", -0.1),
                ("above-256", 256.1),
                ("NaN", f32::NAN),
                ("infinity", f32::INFINITY),
            ] {
                let v = self.view.read(cx);
                let before = canonical(v.editor.scene());
                let revision = v.editor.engine.revision();
                let shown = presented_state(v);
                let result = self.view.update(cx, |v, _| {
                    v.editor.transaction(|s| {
                        s.update_object(&id.into(), |o| {
                            o.visible = !hidden;
                            o.style.opacity = opacity;
                            o.style.blur = value;
                        })
                    })
                });
                let v = self.view.read(cx);
                let atomic = result.is_err()
                    && before == canonical(v.editor.scene())
                    && revision == v.editor.engine.revision();
                rows.push(json!({"visibility":visibility,"invalid":label,"typed_error":result.err().map(|e|format!("{e:?}")),
                    "source_revision_atomic":atomic,"presented_public_state_unchanged":shown==presented_state(v)}));
            }
        }
        self.status="Eight hidden/zero-opacity invalid mutations attempted; all must reject atomically. Records preserve errors.".into();
        self.log(json!({"kind":"invalid_model_attempts","rows":rows,"live":snapshot(self.view.read(cx),self.case)}));
        cx.notify();
    }
    fn zoom(&mut self, z: f32, cx: &mut Context<Self>) {
        self.stop_burst();
        let before = canonical(self.view.read(cx).editor.scene());
        let revision = self.view.read(cx).editor.engine.revision();
        let result = self.view.update(cx, |v, cx| {
            v.execute_command(
                SceneCommand::SetViewport {
                    viewport: Viewport {
                        pan: P::ZERO,
                        zoom: z,
                        ..Default::default()
                    },
                },
                cx,
            )
        });
        let v = self.view.read(cx);
        self.status = format!(
            "Zoom {z}: {:?}; interaction_error can retain an earlier message",
            result
        );
        self.log(json!({"kind":"navigation","zoom":z,"error":result.err().map(|e|format!("{e:?}")),
            "source_revision_unchanged":before==canonical(v.editor.scene())&&revision==v.editor.engine.revision(),"live":snapshot(v,self.case)}));
        cx.notify();
    }
    fn burst(&mut self, cx: &mut Context<Self>) {
        self.stop_burst();
        self.burst_left = 4;
        self.status="Four frame-spaced amount edits, final 1. Current-source presentation is the acceptance condition.".into();
        cx.notify();
    }
    fn advance_burst(&mut self, token: u64, cx: &mut Context<Self>) -> bool {
        // Token comparison precedes any queue flag, source, history or log write.
        if token != self.token {
            return false;
        }
        self.burst_queued = false;
        if self.burst_left == 0 {
            return false;
        }
        self.burst_left -= 1;
        let value = if self.burst_left == 0 {
            1.
        } else if self.burst_left.is_multiple_of(2) {
            0.
        } else {
            0.5
        };
        let case = self.case;
        self.edit(
            &format!("burst amount {value}"),
            move |s| amount(s, case, value),
            cx,
        );
        true
    }
    fn record(&mut self, cx: &mut Context<Self>) {
        self.status="Recorded live public state; pair with the original operator screenshot and row verdict.".into();
        self.log(json!({"kind":"native_observation","entity":format!("{:?}",self.view.entity_id()),"token":self.token,
            "actual_window_dpr":self.actual_dpr,"burst_steps_remaining":self.burst_left,"live":snapshot(self.view.read(cx),self.case),
            "native_visual_verdict":"unassessed","source_contract":"bounded foreground; native gate separately unassessed","native_framebuffer_captured":false}));
        cx.notify();
    }
}
impl Render for Lab {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.actual_dpr = Some(window.scale_factor());
        if self.burst_left > 0 && !self.burst_queued {
            self.burst_queued = true;
            let token = self.token;
            let weak = cx.entity().downgrade();
            window.on_next_frame(move |_, cx| {
                let _ = weak.update(cx, |s, cx| s.advance_burst(token, cx));
            });
            window.request_animation_frame();
        }
        let v = self.view.read(cx);
        let live = format!(
            "Readonly={} | zoom {:.2} | actual DPR {:.2} | pending={} | presented={} | CPU/render errors {}/{}",
            !v.is_editable(),
            v.editor.viewport.zoom,
            self.actual_dpr.unwrap_or(0.),
            v.compositing_pending,
            v.presented_geometry().is_some(),
            v.composite_diagnostics.len(),
            v.diagnostics.len()
        );
        let error = v
            .composite_diagnostics
            .first()
            .map(|e| format!("{:?}", e.error))
            .unwrap_or_else(|| "none".into());
        let status = self.status.chars().take(180).collect::<String>();
        div().size_full().flex().flex_col().bg(rgb(0x111823)).text_color(rgb(0xffffff)).text_sm()
            .child(div().h(px(36.)).flex_shrink_0().px_2().flex().items_center().gap_1()
                .child("Foreground blur")
                .child(ControlButton::new("a","A Leaves").on_click(cx.listener(|s,_,_,cx|s.preset(Case::Leaves,cx))))
                .child(ControlButton::new("b","B Outside").on_click(cx.listener(|s,_,_,cx|s.preset(Case::Offscreen,cx))))
                .child(ControlButton::new("c","C Nested").on_click(cx.listener(|s,_,_,cx|s.preset(Case::Nested,cx))))
                .child(ControlButton::new("d","D Alpha").on_click(cx.listener(|s,_,_,cx|s.preset(Case::Alpha,cx))))
                .child(ControlButton::new("e","E Reject").on_click(cx.listener(|s,_,_,cx|s.preset(Case::Reject,cx))))
                .child(ControlButton::new("record","Record").on_click(cx.listener(|s,_,_,cx|s.record(cx)))))
            .child(div().h(px(36.)).flex_shrink_0().px_2().flex().items_center().gap_1()
                .child(ControlButton::new("zero","Blur 0").on_click(cx.listener(|s,_,_,cx|s.set_amount(0.,cx))))
                .child(ControlButton::new("half","Blur .5").on_click(cx.listener(|s,_,_,cx|s.set_amount(0.5,cx))))
                .child(ControlButton::new("one","Blur 1").on_click(cx.listener(|s,_,_,cx|s.set_amount(1.,cx))))
                .child(ControlButton::new("variant","Variant").on_click(cx.listener(|s,_,_,cx|s.change_variant(cx))))
                .child(ControlButton::new("undo","Undo").on_click(cx.listener(|s,_,_,cx|s.history(false,cx))))
                .child(ControlButton::new("redo","Redo").on_click(cx.listener(|s,_,_,cx|s.history(true,cx))))
                .child(ControlButton::new("invalid","Invalid model").on_click(cx.listener(|s,_,_,cx|s.invalid(cx)))))
            .child(div().h(px(36.)).flex_shrink_0().px_2().flex().items_center().gap_1()
                .child(ControlButton::new("zhalf","Zoom .5").on_click(cx.listener(|s,_,_,cx|s.zoom(0.5,cx))))
                .child(ControlButton::new("z1","Home 1").on_click(cx.listener(|s,_,_,cx|s.zoom(1.,cx))))
                .child(ControlButton::new("z2","Zoom 2").on_click(cx.listener(|s,_,_,cx|s.zoom(2.,cx))))
                .child(ControlButton::new("burst","4 blur frames").on_click(cx.listener(|s,_,_,cx|s.burst(cx))))
                .child(self.case.label()))
            .child(div().w(px(CANVAS_W)).h(px(CANVAS_H)).flex_shrink_0().mx_2().overflow_hidden().child(self.view.clone()))
            .child(div().h(px(215.)).flex_shrink_0().px_3().py_2().text_xs().flex().flex_col().gap_1()
                .child(self.case.hint()).child(live).child(format!("Current composite error: {error}"))
                .child(format!("Actual native click: {}",self.last_pointer)).child(status)
                .child("Fresh preset replaces the readonly entity/subscriptions/history. Sticky input errors are recorded, never silently cleared.")
                .child("Original screenshots are operator images. Separate --cpu-tour and --headless-tour never certify native pixels or input."))
    }
}

// Independent finite f64 separable convolution. This never invokes image::blur
// or private compositor helpers. Transparent padding is explicit. The locked
// kernel-size equation is tested at the four stated sigmas only.
fn reference_blur(input: &[u8], w: usize, h: usize, sigma: f64) -> Vec<u8> {
    let n = (((((sigma - 0.8) / 0.3) + 1.) * 2.) + 1.).max(3.) as usize | 1;
    let radius = n / 2;
    let mut kernel = (0..n)
        .map(|i| (-((i as f64 - radius as f64).powi(2)) / (2. * sigma * sigma)).exp())
        .collect::<Vec<_>>();
    let total: f64 = kernel.iter().sum();
    for k in &mut kernel {
        *k /= total;
    }
    let mut horizontal = vec![0.; input.len()];
    for y in 0..h {
        for x in 0..w {
            for c in 0..4 {
                for (i, k) in kernel.iter().enumerate() {
                    let sx = x as isize + i as isize - radius as isize;
                    if (0..w as isize).contains(&sx) {
                        horizontal[(y * w + x) * 4 + c] +=
                            f64::from(input[(y * w + sx as usize) * 4 + c]) * k;
                    }
                }
            }
        }
    }
    let mut output = vec![0; input.len()];
    for y in 0..h {
        for x in 0..w {
            for c in 0..4 {
                let mut value = 0.;
                for (i, k) in kernel.iter().enumerate() {
                    let sy = y as isize + i as isize - radius as isize;
                    if (0..h as isize).contains(&sy) {
                        value += horizontal[(sy as usize * w + x) * 4 + c] * k;
                    }
                }
                output[(y * w + x) * 4 + c] = value.round().clamp(0., 255.) as u8;
            }
        }
    }
    output
}
fn gray(p: [u8; 4]) -> [u8; 4] {
    let y = ((2126. * f64::from(p[0]) + 7152. * f64::from(p[1]) + 722. * f64::from(p[2])) / 10000.)
        .round()
        .min(f64::from(p[3])) as u8;
    [y, y, y, p[3]]
}
fn viewport(zoom: f32, dpr: f32, bounds: B) -> RasterViewport {
    RasterViewport {
        world_to_screen: Affine::scale(zoom, zoom),
        canvas_bounds: bounds,
        content_mask: bounds,
        device_scale: dpr,
    }
}
fn frame_pixel(frame: &CpuFrame, physical_x: i32, physical_y: i32, dpr: f32) -> [u8; 4] {
    let x = physical_x - (frame.screen_bounds.origin.x * dpr).round() as i32;
    let y = physical_y - (frame.screen_bounds.origin.y * dpr).round() as i32;
    if x < 0 || y < 0 || x >= frame.width as i32 || y >= frame.height as i32 {
        return [0; 4];
    }
    frame.premul_rgba[((y as u32 * frame.width + x as u32) * 4) as usize..][..4]
        .try_into()
        .expect("four channels")
}
fn retain_frame(
    evidence: &mut Evidence,
    tag: &str,
    frame: &CpuFrame,
    dpr: f32,
    probes: &[(&str, f32, f32)],
    zoom: f32,
) -> Result<Value, String> {
    let name = format!("{tag}.premul-rgba");
    evidence.write(&name, &frame.premul_rgba)?;
    Ok(
        json!({"file":name,"encoding":"raw row-major premultiplied encoded-sRGB RGBA8, no header",
        "dimensions":[frame.width,frame.height],"logical_screen_bounds":frame.screen_bounds,
        "bytes":frame.premul_rgba.len(),"fnv1a64":fingerprint(&frame.premul_rgba),"synthetic_dpr":dpr,"zoom":zoom,
        "samples":probes.iter().map(|(name,x,y)|json!({"label":name,"world":[x,y],
            "actual_premul_rgba":frame_pixel(frame,(x*zoom*dpr).floor() as i32,(y*zoom*dpr).floor() as i32,dpr)})).collect::<Vec<_>>(),
        "native_framebuffer":false}),
    )
}
fn domain_sources(scene: &Scene, case: Case) -> Result<Vec<DomainSource>, String> {
    if case == Case::Reject {
        return owned_canvas(
            scene,
            CanvasBackdrop {
                color: Color::rgb(23, 32, 43),
                grid: None,
            },
            CompositeLimits::default(),
        )
        .map(|s| vec![s])
        .map_err(|e| format!("{e:?}"));
    }
    let units = scene_units(scene, CompositeLimits::default()).map_err(|e| e.to_string())?;
    let mut sources = vec![];
    for unit in units {
        match unit {
            SceneUnit::Domain(s) => sources.push(*s),
            SceneUnit::Rejected(issue) => return Err(format!("{:?}", issue.error)),
            _ => {}
        }
    }
    if sources.len() > CompositeLimits::default().max_domains {
        return Err("domain ceiling exceeded".into());
    }
    Ok(sources)
}
fn budget_matrix(evidence: &mut Evidence) -> Result<(), String> {
    // Exactly four synthetic camera/DPR combinations per safe document. Outputs
    // remain retained together until the request ends, matching shared ownership.
    for case in Case::ALL {
        let mut scene = document(case).map_err(|e| e.to_string())?;
        if case == Case::Reject {
            variant(&mut scene, case).map_err(|e| e.to_string())?;
        }
        let sources = domain_sources(&scene, case)?;
        for zoom in [1., 2.] {
            for dpr in [1., 2.] {
                let mut budget = PixelBudget::new(CompositeLimits::default(), 0)
                    .map_err(|e| format!("{e:?}"))?;
                let bounds = B::new(P::ZERO, S::new(CANVAS_W, CANVAS_H));
                let mut frames = vec![];
                let mut rows = vec![];
                for (i, source) in sources.iter().enumerate() {
                    match rasterize(
                        source,
                        viewport(zoom, dpr, bounds),
                        &mut budget,
                        &AtomicBool::new(false),
                    ) {
                        Ok(Some(frame)) => {
                            let record = retain_frame(
                                evidence,
                                &format!("budget-{case:?}-z{zoom}-d{dpr}-domain{i}"),
                                &frame,
                                dpr,
                                case.probes(),
                                zoom,
                            )?;
                            rows.push(record);
                            frames.push(frame);
                        }
                        Ok(None) => {
                            return Err(format!(
                                "{case:?} zoom={zoom} DPR={dpr} domain={i}: expected visible output absent"
                            ));
                        }
                        Err(e) => {
                            evidence.event(&json!({"kind":"budget_rejection","case":case.label(),"zoom":zoom,"dpr":dpr,"domain":i,
                            "typed_error":format!("{e:?}"),"work":budget.surface_writes(),"peak_bytes":budget.peak_bytes(),
                            "retained_bytes":budget.retained_bytes(),"native_acceptance":"not performed"}))?;
                            return Err(format!("{case:?} zoom={zoom} DPR={dpr}: {e:?}"));
                        }
                    }
                }
                let work = budget.surface_writes();
                let peak = budget.peak_bytes();
                let retained = budget.retained_bytes();
                for frame in frames {
                    budget.discard_frame(frame);
                }
                let released = budget.retained_bytes() == 0;
                evidence.event(&json!({"kind":"public_budget_request","case":case.label(),"source_document":serde_json::from_str::<Value>(&canonical(&scene)?).map_err(|e|e.to_string())?,
                "zoom":zoom,"synthetic_dpr":dpr,"domain_count":sources.len(),"work":work,"peak_tracked_bytes":peak,
                "retained_output_bytes":retained,"released":released,"frames":rows,"limits":"unchanged defaults","native_upload_proof":false}))?;
                if !released {
                    return Err("discard_frame retained bytes did not return to zero".into());
                }
            }
        }
    }
    Ok(())
}
fn oracle(evidence: &mut Evidence) -> Result<(), String> {
    for sigma in [0.001f32, 0.5, 1., 2.] {
        for filtered in [false, true] {
            let mut s = Scene::new();
            let mut root = group("oracle", 0., 0., sigma);
            root.style.grayscale = if filtered { 1. } else { 0. };
            root.style.opacity = if filtered { 0.5 } else { 1. };
            add(&mut s, root, None).map_err(|e| e.to_string())?;
            add(
                &mut s,
                rect("red", 20., 20., 8., 8., Color::rgba(255, 0, 0, 128)),
                Some("oracle"),
            )
            .map_err(|e| e.to_string())?;
            add(
                &mut s,
                rect("blue", 24., 20., 4., 8., Color::rgba(0, 0, 255, 128)),
                Some("oracle"),
            )
            .map_err(|e| e.to_string())?;
            let source = capture_subtree(&s, &"oracle".into(), CompositeLimits::default())
                .map_err(|e| format!("{e:?}"))?;
            let mut budget =
                PixelBudget::new(CompositeLimits::default(), 0).map_err(|e| format!("{e:?}"))?;
            let frame = rasterize(
                &source,
                viewport(1., 1., B::new(P::ZERO, S::new(50., 50.))),
                &mut budget,
                &AtomicBool::new(false),
            )
            .map_err(|e| format!("{e:?}"))?
            .ok_or("oracle output absent")?;
            let mut input = vec![0u8; frame.premul_rgba.len()];
            let ox = frame.screen_bounds.origin.x.round() as i32;
            let oy = frame.screen_bounds.origin.y.round() as i32;
            for y in 20..28 {
                for x in 20..28 {
                    let ix = x - ox;
                    let iy = y - oy;
                    if ix < 0 || iy < 0 || ix >= frame.width as i32 || iy >= frame.height as i32 {
                        return Err("oracle source unexpectedly cropped".into());
                    }
                    let offset = ((iy as u32 * frame.width + ix as u32) * 4) as usize;
                    input[offset..offset + 4].copy_from_slice(if x < 24 {
                        &[128, 0, 0, 128]
                    } else {
                        &[64, 0, 128, 192]
                    });
                }
            }
            let mut expected = reference_blur(
                &input,
                frame.width as usize,
                frame.height as usize,
                f64::from(sigma),
            );
            if filtered {
                for p in expected.as_chunks_mut::<4>().0 {
                    let q = gray([p[0], p[1], p[2], p[3]]);
                    for c in 0..4 {
                        p[c] = (f64::from(q[c]) * 0.5).round() as u8;
                    }
                }
            }
            // Fixed independent f64/f32 plus compositing rounding allowance, never
            // widened at runtime. Plain Gaussian: 1 byte; gray+opacity chain: 2.
            let tolerance = if filtered { 2u8 } else { 1u8 };
            let max_error = frame
                .premul_rgba
                .iter()
                .zip(&expected)
                .map(|(a, b)| a.abs_diff(*b))
                .max()
                .unwrap_or(0);
            let valid_premul = frame
                .premul_rgba
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| p[..3].iter().all(|c| *c <= p[3]));
            let record = retain_frame(
                evidence,
                &format!("oracle-s{sigma}-gray-opacity-{filtered}"),
                &frame,
                1.,
                &[("halo", 19., 24.), ("overlap", 25., 24.)],
                1.,
            )?;
            evidence.event(&json!({"kind":"independent_cpu_oracle","sigma":sigma,"gray_then_opacity":filtered,
            "max_channel_error":max_error,"tolerance":tolerance,"valid_premul":valid_premul,"actual":record,
            "expected_fnv1a64":fingerprint(&expected),"work":budget.surface_writes(),"peak_bytes":budget.peak_bytes(),
            "one_tap_deviation":"image 0.25.10 identity corrected only if adjacent f32 weights exactly underflow zero; not upstream byte parity"}))?;
            budget.discard_frame(frame);
            if max_error > tolerance || !valid_premul || budget.retained_bytes() != 0 {
                return Err(format!(
                    "oracle mismatch sigma={sigma} filtered={filtered} max_error={max_error}"
                ));
            }
        }
    }
    Ok(())
}
fn encode(scene: &Scene) -> Result<gpui_flow::composite::SceneExport, CompositeError> {
    SceneExportPlan::new(
        scene,
        SceneExportRequest {
            world_bounds: B::new(P::ZERO, S::new(WORLD_W, WORLD_H)),
            scale: 1.,
            pixel_ratio: 1.,
            background: Color::rgba(0, 0, 0, 0),
            format: SceneExportFormat::Png,
        },
        CompositeLimits::default(),
    )?
    .encode_with_inputs(
        &ImageInputs::default(),
        &TextInputs::default(),
        SceneExportPreparation::default(),
        &AtomicBool::new(false),
    )
}
fn cpu_file(
    evidence: &mut Evidence,
    scene: &Scene,
    case: Case,
    tag: &str,
) -> Result<image::RgbaImage, String> {
    let before = canonical(scene)?;
    let out = encode(scene).map_err(|e| format!("{e:?}"))?;
    let decoded = image::load_from_memory(&out.bytes)
        .map_err(|e| e.to_string())?
        .to_rgba8();
    let name = format!("{tag}.png");
    evidence.write(&name, &out.bytes)?;
    let unchanged = before == canonical(scene)?;
    let samples=case.probes().iter().map(|(label,x,y)|json!({"label":label,"world":[x,y],"decoded_straight_rgba":decoded.get_pixel(*x as u32,*y as u32).0})).collect::<Vec<_>>();
    evidence.json(&format!("{tag}.json"),&json!({"kind":"canonical_scene_cpu_export","png":name,"encoding":"image/png",
        "encoded_bytes":out.bytes.len(),"encoded_fnv1a64":fingerprint(&out.bytes),"decoded_dimensions":decoded.dimensions(),
        "decoded_straight_rgba_fnv1a64":fingerprint(decoded.as_raw()),"samples":samples,"source_unchanged":unchanged,
        "source_document":serde_json::from_str::<Value>(&before).map_err(|e|e.to_string())?,"world_crop":[0,0,WORLD_W,WORLD_H],"scale":1,"synthetic_dpr":1,
        "native_framebuffer":false,"premul_samples":"separate public CpuFrame files; do not interpret decoded PNG as premultiplied"}))?;
    if !unchanged || decoded.dimensions() != (WORLD_W as u32, WORLD_H as u32) {
        return Err("export source/dimensions changed".into());
    }
    Ok(decoded)
}
fn crop_audit(evidence: &mut Evidence) -> Result<(), String> {
    // The same immutable source is sampled at a generous and a tight viewport.
    // This is canonical CPU crop equivalence, not native pixel equivalence.
    for (case, id, tight) in [
        (
            Case::Offscreen,
            "edge",
            B::new(P::new(0., 38.), S::new(5., 14.)),
        ),
        (
            Case::Offscreen,
            "frame",
            B::new(P::new(79., 34.), S::new(12., 23.)),
        ),
        (
            Case::Nested,
            "clip",
            B::new(P::new(39., 38.), S::new(18., 19.)),
        ),
    ] {
        let s = document(case).map_err(|e| e.to_string())?;
        let source = capture_subtree(&s, &id.into(), CompositeLimits::default())
            .map_err(|e| format!("{e:?}"))?;
        let mut broad_budget =
            PixelBudget::new(CompositeLimits::default(), 0).map_err(|e| format!("{e:?}"))?;
        let broad = rasterize(
            &source,
            viewport(1., 1., B::new(P::new(-20., 10.), S::new(150., 80.))),
            &mut broad_budget,
            &AtomicBool::new(false),
        )
        .map_err(|e| format!("{e:?}"))?
        .ok_or("generous crop absent")?;
        let mut tight_budget =
            PixelBudget::new(CompositeLimits::default(), 0).map_err(|e| format!("{e:?}"))?;
        let small = rasterize(
            &source,
            viewport(1., 1., tight),
            &mut tight_budget,
            &AtomicBool::new(false),
        )
        .map_err(|e| format!("{e:?}"))?
        .ok_or("tight crop absent")?;
        let mut equal = true;
        let mut alpha_sum = 0u64;
        for y in tight.origin.y as i32..(tight.origin.y + tight.size.height) as i32 {
            for x in tight.origin.x as i32..(tight.origin.x + tight.size.width) as i32 {
                let a = frame_pixel(&small, x, y, 1.);
                let b = frame_pixel(&broad, x, y, 1.);
                equal &= a == b;
                alpha_sum += u64::from(a[3]);
            }
        }
        let full = retain_frame(
            evidence,
            &format!("crop-{id}-generous"),
            &broad,
            1.,
            case.probes(),
            1.,
        )?;
        let cropped = retain_frame(
            evidence,
            &format!("crop-{id}-tight"),
            &small,
            1.,
            case.probes(),
            1.,
        )?;
        evidence.event(&json!({"kind":"canonical_cpu_crop","id":id,"tight_viewport":tight,"exact_premul_crop":equal,"tight_alpha_sum":alpha_sum,"generous":full,"tight":cropped}))?;
        broad_budget.discard_frame(broad);
        tight_budget.discard_frame(small);
        if !equal
            || alpha_sum == 0
            || broad_budget.retained_bytes() != 0
            || tight_budget.retained_bytes() != 0
        {
            return Err(format!("crop audit failed for {id}"));
        }
    }
    Ok(())
}
fn cpu_tour(evidence: &mut Evidence) -> Result<(), String> {
    oracle(evidence)?;
    crop_audit(evidence)?;
    for case in [Case::Leaves, Case::Offscreen, Case::Nested, Case::Alpha] {
        let mut scene = document(case).map_err(|e| e.to_string())?;
        let image = cpu_file(evidence, &scene, case, &format!("{case:?}-default"))?;
        if case == Case::Offscreen {
            let edge = image.get_pixel(0, 44).0;
            let entering = image.get_pixel(80, 43).0;
            let shadow = image.get_pixel(86, 43).0;
            let pass = edge[3] > 0
                && entering[0] > 0
                && entering[1] == 0
                && entering[2] == 0
                && entering[3] > 0
                && shadow[0] == 0
                && shadow[1] > 0
                && shadow[2] == 0
                && shadow[3] > 0
                && entering[3] == shadow[3];
            evidence.event(&json!({"kind":"paintless_Frame_parent_shadow","edge_halo_straight":edge,
                "child_entering_red_straight":entering,"parent_offset_green_straight":shadow,
                "expected":"positive red at (80,43), same positive alpha in green at (86,43); no Frame fill or stroke", "pass":pass}))?;
            if !pass {
                return Err(
                    "offscreen child halo/paintless Frame parent shadow check failed".into(),
                );
            }
        }
        if case == Case::Alpha {
            let mask = image.get_pixel(33, 37).0;
            let pixel = image.get_pixel(73, 37).0;
            let path_inside = image.get_pixel(117, 37).0;
            let path_outside = image.get_pixel(113, 37).0;
            let pass = mask[3] > 0
                && mask[3] < 128
                && pixel[3] > 0
                && pixel[3] < 255
                && path_inside == [0; 4]
                && path_outside == [255, 0, 0, 255];
            evidence.event(&json!({"kind":"alpha_semantics","mask_halo_straight":mask,"pixel_halo_straight":pixel,"path_inside":path_inside,"path_outside":path_outside,"pass":pass}))?;
            if !pass {
                return Err("mask/Pixel/Path discriminating alpha checks failed".into());
            }
            variant(&mut scene, case).map_err(|e| e.to_string())?;
            let path_zero = cpu_file(evidence, &scene, case, "Alpha-path-blur-zero")?;
            let same = image == path_zero;
            evidence.event(&json!({"kind":"Path_ignores_blur","entire_decoded_png_equal":same}))?;
            if !same {
                return Err("Path blur zero/positive altered pixels".into());
            }
        }
    }
    let mut scene = document(Case::Reject).map_err(|e| e.to_string())?;
    let before = canonical(&scene)?;
    let error = encode(&scene).err();
    let expected = CompositeError::UnsupportedBlurBackdrop("focus".into());
    evidence.event(&json!({"kind":"unsafe_export","typed_error":error.as_ref().map(|e|format!("{e:?}")),"expected":format!("{expected:?}"),"source_unchanged":before==canonical(&scene)?,"png_published":false}))?;
    if error != Some(expected) || before != canonical(&scene)? {
        return Err("unsafe export did not reject atomically with expected blur diagnostic".into());
    }
    variant(&mut scene, Case::Reject).map_err(|e| e.to_string())?;
    cpu_file(evidence, &scene, Case::Reject, "Reject-Normal-recovery")?;
    evidence.json("cpu-summary.json",&json!({"cpu_checks_completed":true,"native_acceptance":"not performed","png_count":6,"default_limits_unchanged":true}))
}
fn headless_tour(evidence: &mut Evidence) -> Result<(), String> {
    let mut planner_rows = vec![];
    for case in Case::ALL {
        let mut scene = document(case).map_err(|e| e.to_string())?;
        let units = scene_units(&scene, CompositeLimits::default()).map_err(|e| e.to_string())?;
        let domains = units
            .iter()
            .filter(|u| matches!(u, SceneUnit::Domain(_)))
            .count();
        let rejected = units
            .iter()
            .filter_map(|u| {
                if let SceneUnit::Rejected(i) = u {
                    Some(i.error.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        let expected = CompositeError::UnsupportedBlurBackdrop("focus".into());
        let domain_ok = if case == Case::Reject {
            rejected == vec![expected.clone()]
        } else {
            rejected.is_empty() && domains <= CompositeLimits::default().max_domains
        };
        let owned = if case == Case::Reject {
            let rejected_owned = owned_canvas(
                &scene,
                CanvasBackdrop {
                    color: Color::WHITE,
                    grid: None,
                },
                CompositeLimits::default(),
            )
            .err();
            variant(&mut scene, case).map_err(|e| e.to_string())?;
            let normal = owned_canvas(
                &scene,
                CanvasBackdrop {
                    color: Color::WHITE,
                    grid: None,
                },
                CompositeLimits::default(),
            );
            let pass = rejected_owned == Some(expected) && normal.is_ok();
            Some(
                json!({"unsafe_error":rejected_owned.map(|e|format!("{e:?}")),"Normal_recovery_error":normal.err().map(|e|format!("{e:?}")),"pass":pass}),
            )
        } else {
            None
        };
        let pass = domain_ok && owned.as_ref().is_none_or(|v| v["pass"] == true);
        let row = json!({"kind":"public_planner_check","case":case.label(),"domain_count":domains,
            "max_domains":CompositeLimits::default().max_domains,"errors":rejected.iter().map(|e|format!("{e:?}")).collect::<Vec<_>>(),
            "owned_canvas":owned,"pass":pass});
        evidence.event(&row)?;
        planner_rows.push(row);
        if !pass {
            return Err(format!(
                "{case:?}: public domain planner/OwnedCanvas contract mismatch"
            ));
        }
    }
    let mut editor = SceneEditor::new(document(Case::Leaves).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let mut host = ScenePluginHost::with_defaults();
    host.set_editable(false, &mut editor);
    let original = canonical(editor.scene())?;
    let denied = host.execute(
        &mut editor,
        SceneCommand::LoadScene(document(Case::Offscreen).map_err(|e| e.to_string())?),
    );
    if !matches!(
        denied,
        Err(InteractionError::CapabilityDenied(Capability::Edit))
    ) || canonical(editor.scene())? != original
    {
        return Err("readonly LoadScene guard failed".into());
    }
    editor
        .transaction(|s| amount(s, Case::Leaves, 0.5))
        .map_err(|e| e.to_string())?;
    let edited = canonical(editor.scene())?;
    let mut rejects = vec![];
    for (hidden, opacity) in [(true, 1.), (false, 0.)] {
        for (name, value) in [
            ("negative", -0.1),
            ("above-256", 256.1),
            ("NaN", f32::NAN),
            ("infinity", f32::INFINITY),
        ] {
            let revision = editor.engine.revision();
            let r = editor.transaction(|s| {
                s.update_object(&"color".into(), |o| {
                    o.visible = !hidden;
                    o.style.opacity = opacity;
                    o.style.blur = value;
                })
            });
            let atomic = r.is_err()
                && editor.engine.revision() == revision
                && canonical(editor.scene())? == edited;
            rejects.push(json!({"hidden":hidden,"opacity":opacity,"invalid":name,"typed_error":r.err().map(|e|format!("{e:?}")),"atomic":atomic}));
            if !atomic {
                return Err("invalid retained blur mutation was not atomic".into());
            }
        }
    }
    for zoom in [0.5, 2., 1.] {
        host.execute(
            &mut editor,
            SceneCommand::SetViewport {
                viewport: Viewport {
                    zoom,
                    ..Default::default()
                },
            },
        )
        .map_err(|e| e.to_string())?;
        if canonical(editor.scene())? != edited {
            return Err("navigation changed authored source".into());
        }
    }
    if !editor.undo() || canonical(editor.scene())? != original || editor.undo() {
        return Err(
            "safe amount edit did not undo exactly once after invalid input/navigation".into(),
        );
    }
    if !editor.redo() || canonical(editor.scene())? != edited || editor.redo() {
        return Err("safe amount Redo mismatch".into());
    }
    geometry_audit(evidence)?;
    budget_matrix(evidence)?;
    admission_rejections(evidence)?;
    evidence.json("headless.json",&json!({"kind":"public headless fixture helper","documents_valid":5,"readonly_load_scene_denied":true,
        "public_planners":planner_rows,"rejections":rejects,"one_safe_edit_exact_undo_redo":true,"navigation_keeps_source":true,
        "native_application_created":false,"native_input_exercised":false,"native_acceptance":"not performed"}))
}
fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    let cpu = args.iter().any(|a| a == "--cpu-tour");
    let helper = args.iter().any(|a| a == "--headless-tour");
    if cpu && helper {
        eprintln!("Choose one helper mode");
        std::process::exit(2);
    }
    let kind = if cpu {
        "cpu"
    } else if helper {
        "headless"
    } else {
        "physical"
    };
    let mut evidence = match Evidence::new(kind) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    if cpu || helper {
        let result = if cpu {
            cpu_tour(&mut evidence)
        } else {
            headless_tour(&mut evidence)
        };
        if let Err(error) = result {
            let _ = evidence.json(
                "failure.json",
                &json!({"kind":kind,"error":error,"native_acceptance":"not performed"}),
            );
            eprintln!(
                "{kind} failed: {error}; partial evidence preserved at {}",
                evidence.path.display()
            );
            std::process::exit(1);
        }
        println!(
            "{kind} evidence {} (no native acceptance)",
            evidence.path.display()
        );
        return;
    }
    println!(
        "Physical diagnostics: {}; original screenshots remain separate",
        evidence.path.display()
    );
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(1060.), px(600.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                |window, cx| {
                    window.set_window_title("gpui-flow · bounded foreground blur");
                    cx.new(|cx| Lab::new(evidence, cx))
                },
            )
            .expect("native window creation failed");
            cx.activate(true);
        });
}
fn admission_rejections(evidence: &mut Evidence) -> Result<(), String> {
    for (label, blur, camera, expected) in [
        (
            "sigma below .001",
            0.0005,
            Affine::IDENTITY,
            CompositeError::TooLarge,
        ),
        (
            "sigma above 64",
            65.,
            Affine::IDENTITY,
            CompositeError::TooLarge,
        ),
        (
            "nonuniform camera",
            1.,
            Affine::scale(2., 1.),
            CompositeError::UnsupportedTransform,
        ),
    ] {
        let mut s = Scene::new();
        let mut o = rect("sample", 20., 20., 4., 4., RED);
        o.style.blur = blur;
        add(&mut s, o, None).map_err(|e| e.to_string())?;
        let source = capture_subtree(&s, &"sample".into(), CompositeLimits::default())
            .map_err(|e| format!("{e:?}"))?;
        let mut budget =
            PixelBudget::new(CompositeLimits::default(), 0).map_err(|e| format!("{e:?}"))?;
        let bounds = B::new(P::ZERO, S::new(60., 60.));
        let result = rasterize(
            &source,
            RasterViewport {
                world_to_screen: camera,
                canvas_bounds: bounds,
                content_mask: bounds,
                device_scale: 1.,
            },
            &mut budget,
            &AtomicBool::new(false),
        );
        let error = result.as_ref().err().cloned();
        let pass = error == Some(expected.clone()) && budget.retained_bytes() == 0;
        evidence.event(&json!({"kind":"typed_physical_rejection","label":label,"scalar":blur,"camera":camera,
            "error":error.map(|e|format!("{e:?}")),"expected":format!("{expected:?}"),"work":budget.surface_writes(),"peak_bytes":budget.peak_bytes(),"retained_bytes":budget.retained_bytes(),"pass":pass}))?;
        if !pass {
            return Err(format!("{label}: expected typed physical rejection"));
        }
    }
    Ok(())
}

fn geometry_audit(evidence: &mut Evidence) -> Result<(), String> {
    for case in [Case::Leaves, Case::Alpha] {
        let mut s = document(case).map_err(|e| e.to_string())?;
        let signature = |s: &Scene| -> Result<Value, String> {
            let bounds = case
                .ids()
                .iter()
                .map(|id| s.world_bounds(&(*id).into()).map_err(|e| e.to_string()))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(
                json!({"geometric_bounds":bounds,"strict_hits":case.probes().iter().map(|(label,x,y)|json!({"label":label,"ids":hits(s,P::new(*x,*y),HitTestOptions::default())})).collect::<Vec<_>>()}),
            )
        };
        let before = signature(&s)?;
        amount(&mut s, case, 0.).map_err(|e| e.to_string())?;
        let after = signature(&s)?;
        let equal = before == after;
        evidence.event(&json!({"kind":"geometry_invariance","case":case.label(),"blur_positive":before,"blur_zero":after,"equal":equal,
            "pointer_tolerance":"native adapter separately uses 4 logical screen pixels; a tolerance hit near an edge is not a blur-halo hit"}))?;
        if !equal {
            return Err(format!(
                "{case:?}: blur altered geometric bounds/strict hits"
            ));
        }
    }
    Ok(())
}
