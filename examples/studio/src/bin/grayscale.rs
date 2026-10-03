//! Bounded APP-04 grayscale lab. Source, CPU helpers and physical QA are separate.
//! Native receipts, original operator screenshots and CPU exports are distinct.
//! No framebuffer access, renderer backdoor, resources, new dependencies or fonts.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, SceneCompositing, SceneInputOperation, SceneView, SceneViewEvent,
    composite::{
        CanvasBackdrop, CompositeError, CompositeLimits, ImageInputs, PixelBudget, RasterViewport,
        SceneExportFormat, SceneExportPlan, SceneExportPreparation, SceneExportRequest, SceneUnit,
        TextInputs, capture_subtree, owned_canvas, rasterize, scene_units,
    },
    core::Viewport,
    editor::SceneEditor,
    engine::SceneEngine,
    interaction::{
        Capability, InteractionError,
        scene::{SceneCommand, ScenePluginHost},
    },
    scene::{
        Affine, BlendMode, Bounds as B, Color, ContainerPaintOrder, DisplayObject, EraserMode,
        EventKind, HitPolicy, HitTestOptions, MaskMode, Point as P, Scene, SceneError, Shadow,
        Shape, Size as S, Stroke, Style, display,
    },
};
use serde_json::{Value, json};
use std::{
    io::Write,
    path::PathBuf,
    sync::atomic::AtomicBool,
    time::{SystemTime, UNIX_EPOCH},
};

const WORLD_W: f32 = 960.;
const WORLD_H: f32 = 380.;
const CANVAS_W: f32 = 1040.;
const CANVAS_H: f32 = 420.;
const RED: Color = Color::rgb(255, 0, 0);
const GREEN: Color = Color::rgb(0, 255, 0);
const BLUE: Color = Color::rgb(0, 0, 255);
const MAGENTA: Color = Color::rgb(255, 0, 255);
const MAX_RECORDS: u32 = 192;
const SESSION_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Case {
    Leaves,
    Group,
    Nested,
    Alpha,
    Reject,
}
impl Case {
    const ALL: [Self; 5] = [
        Self::Leaves,
        Self::Group,
        Self::Nested,
        Self::Alpha,
        Self::Reject,
    ];
    fn label(self) -> &'static str {
        match self {
            Self::Leaves => "A Leaves: columns 0 / .5 / 1",
            Self::Group => "B Normal group: grayscale + opacity",
            Self::Nested => "C Box / Frame: child .5 then parent .5",
            Self::Alpha => "D Alpha mask / Pixel / Path",
            Self::Reject => "E Unsafe PassThrough in OwnedCanvas",
        }
    }
    fn ids(self) -> &'static [&'static str] {
        match self {
            Self::Leaves => &["r2", "g2", "b2", "low1", "shadow1"],
            Self::Group | Self::Reject => &["focus"],
            Self::Nested => &["box", "frame"],
            Self::Alpha => &["mask", "pixel", "path"],
        }
    }
    fn hint(self) -> &'static str {
        match self {
            Self::Leaves => {
                "R/G/B rows; amount 1 gives 54/182/18. Bottom: colored shadows at 0/1 and low-alpha red at 0/1. Amount buttons edit the right column and bottom filtered objects."
            }
            Self::Group => {
                "Left filtered / right reference. Overlap receives group opacity once. Earlier blue backdrop and later magenta stay colored. Variant toggles left opacity .5 / 1."
            }
            Self::Nested => {
                "Left Box / right Frame inside fractional rounded ancestor clips. Child .5 then parent .5 gives red [105,41,41]. Variant swaps both paint phases; overflow differs."
            }
            Self::Alpha => {
                "Mask/Pixel/Path source amount 0 and 1 must give identical coverage and final pixels. Parents filter survivors to gray. Later green child becomes gray; geometric picking still sees erasers."
            }
            Self::Reject => {
                "Initial gray 1 + PassThrough + Multiply must reject with UnsupportedGrayscaleBackdrop. Variant chooses Normal for recovery. Accepted source may coexist with a rejected presentation."
            }
        }
    }
    fn probes(self) -> &'static [(&'static str, f32, f32)] {
        match self {
            Self::Leaves => &[
                ("red0", 80., 70.),
                ("red-half", 380., 70.),
                ("red1", 680., 70.),
                ("green1", 680., 135.),
                ("blue1", 680., 200.),
                ("low1", 680., 300.),
                ("shadow-color", 245., 342.),
            ],
            Self::Group => &[
                ("red-only", 110., 110.),
                ("overlap-green", 210., 150.),
                ("external-blue", 60., 70.),
                ("later-magenta", 350., 240.),
                ("reference-red", 590., 110.),
            ],
            Self::Nested => &[
                ("box-child", 145., 140.),
                ("frame-child", 605., 140.),
                ("box-stroke", 289., 140.),
                ("frame-stroke", 749., 140.),
                ("outside-clip", 62., 47.),
            ],
            Self::Alpha => &[
                ("mask-covered", 100., 130.),
                ("mask-outside", 260., 130.),
                ("pixel-hole", 455., 130.),
                ("path-hole", 755., 130.),
                ("later-survivor", 475., 235.),
            ],
            Self::Reject => &[
                ("inside-focus", 150., 130.),
                ("external-red", 75., 65.),
                ("later-magenta", 360., 260.),
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
fn group(id: &str, x: f32, y: f32, amount: f32) -> DisplayObject {
    let mut o =
        DisplayObject::new(id, display::Group::new()).with_transform(Affine::translation(x, y));
    o.style.blend_mode = BlendMode::Normal;
    o.style.grayscale = amount;
    o.hit_policy = HitPolicy::None;
    o
}
fn document(case: Case) -> Result<Scene, SceneError> {
    let mut s = Scene::new();
    s.data = json!({"fixture":"grayscale","case":format!("{case:?}")});
    match case {
        Case::Leaves => {
            // Eight domains: six positive primaries, low1, one shadow pair.
            // Even gray-0 shadow0 needs a domain, so share a Normal identity
            // parent instead of accidentally creating a ninth domain.
            for (col, amount) in [0., 0.5, 1.].into_iter().enumerate() {
                for (row, name, color) in [(0, "r", RED), (1, "g", GREEN), (2, "b", BLUE)] {
                    let mut o = rect(
                        &format!("{name}{col}"),
                        50. + 300. * col as f32,
                        50. + 65. * row as f32,
                        90.,
                        45.,
                        color,
                    );
                    o.style.grayscale = amount;
                    add(&mut s, o, None)?;
                }
            }
            for (id, x, amount) in [("low0", 350., 0.), ("low1", 650., 1.)] {
                let mut o = rect(id, x, 275., 90., 60., Color::rgba(255, 0, 0, 128));
                o.style.grayscale = amount;
                add(&mut s, o, None)?;
            }
            add(&mut s, group("shadow-pair", 0., 0., 0.), None)?;
            for (id, x, amount) in [("shadow0", 50., 0.), ("shadow1", 190., 1.)] {
                let mut o = rect(id, x, 275., 70., 60., Color::rgb(0, 255, 255));
                o.style.grayscale = amount;
                o.style.shadow = Some(Shadow::new(12., 12., 0., MAGENTA));
                o.style.inner_shadow_layers = vec![Shadow::new(8., 6., 0., RED)];
                o.style.stroke = Some(Stroke::new(BLUE, 4.));
                add(&mut s, o, Some("shadow-pair"))?;
            }
        }
        Case::Group => {
            for (id, x, amount) in [("focus", 80., 1.), ("reference", 560., 0.)] {
                add(
                    &mut s,
                    rect(&format!("{id}-external"), x - 30., 60., 380., 230., BLUE),
                    None,
                )?;
                let mut g = group(id, x, 80., amount);
                g.style.opacity = 0.5;
                add(&mut s, g, None)?;
                add(
                    &mut s,
                    rect(&format!("{id}-red"), 0., 0., 180., 150., RED),
                    Some(id),
                )?;
                add(
                    &mut s,
                    rect(&format!("{id}-green"), 90., 40., 180., 150., GREEN),
                    Some(id),
                )?;
                add(
                    &mut s,
                    rect(&format!("{id}-later"), x + 250., 220., 50., 50., MAGENTA),
                    None,
                )?;
            }
        }
        Case::Nested => {
            for (id, x, is_frame) in [("box", 60.25, false), ("frame", 520.25, true)] {
                let clip_id = format!("{id}-clip");
                let mut clip = DisplayObject::new(
                    clip_id.clone(),
                    display::Frame::new(280., 270.).rounded(35.),
                )
                .with_transform(Affine::translation(x, 45.25));
                clip.style.fill = None;
                clip.hit_policy = HitPolicy::None;
                add(&mut s, clip, None)?;
                let shape = if is_frame {
                    Shape::Frame(display::Frame::new(210., 180.).rounded(22.))
                } else {
                    Shape::Box(display::Box::new(210., 180.).rounded(22.))
                };
                let mut parent =
                    DisplayObject::new(id, shape).with_transform(Affine::translation(20.25, 30.25));
                parent.style = Style {
                    fill: Some(Color::rgb(0, 255, 255).into()),
                    grayscale: 0.5,
                    blend_mode: BlendMode::Normal,
                    stroke: Some(Stroke::new(MAGENTA, 10.)),
                    shadow: Some(Shadow::new(18., 12., 0., BLUE)),
                    inner_shadow_layers: vec![Shadow::new(8., 6., 0., GREEN)],
                    ..Default::default()
                };
                add(&mut s, parent, Some(&clip_id))?;
                let mut child = rect(&format!("{id}-child"), 20., 20., 230., 125., RED);
                child.style.grayscale = 0.5;
                child.style.shadow = Some(Shadow::new(10., 10., 0., MAGENTA));
                add(&mut s, child, Some(id))?;
            }
        }
        Case::Alpha => {
            for (id, x) in [
                ("mask-parent", 40.),
                ("pixel-parent", 340.),
                ("path-parent", 640.),
            ] {
                add(&mut s, group(id, x, 65., 1.), None)?;
                if id == "mask-parent" {
                    let mut mask = rect("mask", 20., 20., 140., 160., Color::rgba(0, 0, 255, 128));
                    mask.mask = MaskMode::Alpha;
                    mask.style.grayscale = 1.;
                    mask.hit_policy = HitPolicy::None;
                    add(&mut s, mask, Some(id))?;
                }
                add(
                    &mut s,
                    rect(&format!("{id}-red"), 0., 0., 250., 210., RED),
                    Some(id),
                )?;
                if id != "mask-parent" {
                    let cut_id = if id == "pixel-parent" {
                        "pixel"
                    } else {
                        "path"
                    };
                    let mut cut = DisplayObject::new(cut_id, display::Ellipse::new(120., 140.))
                        .with_transform(Affine::translation(60., 15.))
                        .with_style(Style::fill(Color::rgba(0, 0, 255, 128)));
                    cut.style.grayscale = 1.;
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
                    rect(&format!("{id}-later"), 100., 155., 60., 35., GREEN),
                    Some(id),
                )?;
            }
        }
        Case::Reject => {
            add(&mut s, rect("external", 70., 60., 340., 250., RED), None)?;
            let mut g = group("focus", 90., 80., 1.);
            g.style.blend_mode = BlendMode::PassThrough;
            add(&mut s, g, None)?;
            let mut child = rect("multiply", 0., 0., 240., 180., BLUE);
            child.style.blend_mode = BlendMode::Multiply;
            add(&mut s, child, Some("focus"))?;
            add(&mut s, rect("later", 340., 245., 60., 40., MAGENTA), None)?;
        }
    }
    s.validate()?;
    Ok(s)
}
fn amount(scene: &mut Scene, case: Case, value: f32) -> Result<(), SceneError> {
    for id in case.ids() {
        scene.update_object(&(*id).into(), |o| o.style.grayscale = value)?;
    }
    Ok(())
}
fn variant(scene: &mut Scene, case: Case) -> Result<(), SceneError> {
    match case {
        Case::Group => scene.update_object(&"focus".into(), |o| {
            o.style.opacity = if o.style.opacity == 0.5 { 1. } else { 0.5 }
        }),
        Case::Nested => {
            for id in case.ids() {
                scene.update_object(&(*id).into(), |o| {
                    let order = match &mut o.shape {
                        Shape::Box(v) => &mut v.paint_order,
                        Shape::Frame(v) => &mut v.paint_order,
                        _ => return,
                    };
                    *order = if *order == ContainerPaintOrder::BeforeChildren {
                        ContainerPaintOrder::StrokeAfterChildren
                    } else {
                        ContainerPaintOrder::BeforeChildren
                    };
                })?;
            }
            Ok(())
        }
        Case::Reject => scene.update_object(&"focus".into(), |o| {
            o.style.blend_mode = if o.style.blend_mode == BlendMode::Normal {
                BlendMode::PassThrough
            } else {
                BlendMode::Normal
            }
        }),
        _ => Ok(()),
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
        let root = PathBuf::from(
            std::env::var_os("FLOW_GRAYSCALE_EVIDENCE_DIR")
                .ok_or("Set FLOW_GRAYSCALE_EVIDENCE_DIR to an existing absolute evidence root")?,
        );
        if !root.is_absolute() || !root.is_dir() {
            return Err("Evidence root must exist and be absolute".into());
        }
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos();
        let path = root.join(format!("grayscale-{kind}-{}-{stamp}", std::process::id()));
        std::fs::create_dir(&path).map_err(|e| e.to_string())?;
        let mut out = Self {
            path,
            bytes: 0,
            next: 0,
        };
        out.json("manifest.json",&json!({"fixture":"grayscale","kind":kind,
            "native_acceptance":"operator assessment required; never set by this program",
            "native_framebuffer_captured":false,"no_runtime_budget_escalation":true,
            "maximum_session_bytes":SESSION_BYTES,"source":"record exact integrated source and executable hashes separately"}))?;
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
    json!({"available":v.presented_geometry().is_some(),"pending":v.compositing_pending,
        "amounts":v.presented_geometry().map(|s|s.nodes().map(|n|json!({"id":n.object.id,
            "grayscale":n.object.style.grayscale,"opacity":n.object.style.opacity})).collect::<Vec<_>>()),
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
        "publication_generation":"not public; entity/session/revision/source fingerprint recorded instead"})
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
                        pan: P::new(20., 20.),
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
                    "source_revision":v.editor.engine.revision(),"pending":v.compositing_pending,
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
            &format!("grayscale {value}"),
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
                ("above-one", 1.1),
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
                            o.style.grayscale = value;
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
                        pan: P::new(20., 20.),
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
        self.burst_left = 8;
        self.status="Eight frame-spaced amount edits, final 1. Current-source presentation is the acceptance condition.".into();
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
            "native_visual_verdict":"unassessed","native_framebuffer_captured":false}));
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
                .child("Grayscale")
                .child(ControlButton::new("a","A Leaves").on_click(cx.listener(|s,_,_,cx|s.preset(Case::Leaves,cx))))
                .child(ControlButton::new("b","B Group").on_click(cx.listener(|s,_,_,cx|s.preset(Case::Group,cx))))
                .child(ControlButton::new("c","C Nested").on_click(cx.listener(|s,_,_,cx|s.preset(Case::Nested,cx))))
                .child(ControlButton::new("d","D Alpha").on_click(cx.listener(|s,_,_,cx|s.preset(Case::Alpha,cx))))
                .child(ControlButton::new("e","E Reject").on_click(cx.listener(|s,_,_,cx|s.preset(Case::Reject,cx))))
                .child(ControlButton::new("record","Record").on_click(cx.listener(|s,_,_,cx|s.record(cx)))))
            .child(div().h(px(36.)).flex_shrink_0().px_2().flex().items_center().gap_1()
                .child(ControlButton::new("zero","Amount 0").on_click(cx.listener(|s,_,_,cx|s.set_amount(0.,cx))))
                .child(ControlButton::new("half","Amount .5").on_click(cx.listener(|s,_,_,cx|s.set_amount(0.5,cx))))
                .child(ControlButton::new("one","Amount 1").on_click(cx.listener(|s,_,_,cx|s.set_amount(1.,cx))))
                .child(ControlButton::new("variant","Variant").on_click(cx.listener(|s,_,_,cx|s.change_variant(cx))))
                .child(ControlButton::new("undo","Undo").on_click(cx.listener(|s,_,_,cx|s.history(false,cx))))
                .child(ControlButton::new("redo","Redo").on_click(cx.listener(|s,_,_,cx|s.history(true,cx))))
                .child(ControlButton::new("invalid","Invalid model").on_click(cx.listener(|s,_,_,cx|s.invalid(cx)))))
            .child(div().h(px(36.)).flex_shrink_0().px_2().flex().items_center().gap_1()
                .child(ControlButton::new("zhalf","Zoom .5").on_click(cx.listener(|s,_,_,cx|s.zoom(0.5,cx))))
                .child(ControlButton::new("z1","Home 1").on_click(cx.listener(|s,_,_,cx|s.zoom(1.,cx))))
                .child(ControlButton::new("z2","Zoom 2").on_click(cx.listener(|s,_,_,cx|s.zoom(2.,cx))))
                .child(ControlButton::new("burst","8 amount frames").on_click(cx.listener(|s,_,_,cx|s.burst(cx))))
                .child(self.case.label()))
            .child(div().w(px(CANVAS_W)).h(px(CANVAS_H)).flex_shrink_0().mx_2().overflow_hidden().child(self.view.clone()))
            .child(div().h(px(190.)).flex_shrink_0().px_3().py_2().text_xs().flex().flex_col().gap_1()
                .child(self.case.hint()).child(live).child(format!("Current composite error: {error}"))
                .child(format!("Actual native click: {}",self.last_pointer)).child(status)
                .child("Fresh preset replaces the readonly entity/subscriptions/history. Sticky input errors are recorded, never silently cleared.")
                .child("Original screenshots are operator images. Separate --cpu-tour and --headless-tour never certify native pixels or input."))
    }
}

// Exact scalar contract on premultiplied bytes. This independent oracle is only
// used against public CPU output and is never a renderer or native replacement.
fn expected_gray(p: [u8; 4], amount: f32) -> [u8; 4] {
    let y = (2126. * f64::from(p[0]) + 7152. * f64::from(p[1]) + 722. * f64::from(p[2])) / 10000.;
    let mut out = p;
    for c in 0..3 {
        out[c] = (f64::from(p[c]) + f64::from(amount) * (y - f64::from(p[c])))
            .round()
            .clamp(0., f64::from(p[3])) as u8;
    }
    out
}
fn primitive_audit() -> Result<Value, String> {
    let mut rows = vec![];
    for input in [
        [255, 0, 0, 255],
        [0, 255, 0, 255],
        [0, 0, 255, 255],
        [128, 0, 0, 128],
        [128, 64, 0, 128],
        [128, 128, 128, 128],
    ] {
        for amount in [0., 0.5, 1.] {
            // Choose straight source values whose premultiplication is exact.
            let straight = if input[3] == 128 {
                [
                    255,
                    if input[1] == 64 {
                        128
                    } else if input[1] == 128 {
                        255
                    } else {
                        0
                    },
                    if input[2] == 128 { 255 } else { 0 },
                    128,
                ]
            } else {
                input
            };
            let mut scene = Scene::new();
            let mut o = rect(
                "sample",
                0.,
                0.,
                8.,
                8.,
                Color::rgba(straight[0], straight[1], straight[2], straight[3]),
            );
            o.style.grayscale = amount;
            add(&mut scene, o, None).map_err(|e| e.to_string())?;
            let source = capture_subtree(&scene, &"sample".into(), CompositeLimits::default())
                .map_err(|e| format!("{e:?}"))?;
            let bounds = B::new(P::ZERO, S::new(8., 8.));
            let mut budget =
                PixelBudget::new(CompositeLimits::default(), 0).map_err(|e| format!("{e:?}"))?;
            let frame = rasterize(
                &source,
                RasterViewport {
                    world_to_screen: Affine::IDENTITY,
                    canvas_bounds: bounds,
                    content_mask: bounds,
                    device_scale: 1.,
                },
                &mut budget,
                &AtomicBool::new(false),
            )
            .map_err(|e| format!("{e:?}"))?
            .ok_or("primitive output absent")?;
            let offset = ((frame.height / 2 * frame.width + frame.width / 2) * 4) as usize;
            let actual: [u8; 4] = frame.premul_rgba[offset..offset + 4]
                .try_into()
                .map_err(|_| "bad primitive output")?;
            let expected = expected_gray(input, amount);
            rows.push(json!({"input_premul":input,"amount":amount,"actual_premul":actual,"expected_premul":expected,"pass":actual==expected,"work":budget.surface_writes(),"peak_bytes":budget.peak_bytes()}));
            budget.discard_frame(frame);
            if actual != expected || budget.retained_bytes() != 0 {
                return Err(format!(
                    "primitive mismatch input={input:?} amount={amount} actual={actual:?} expected={expected:?}"
                ));
            }
        }
    }
    Ok(json!(rows))
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
    let png_name = format!("{tag}.png");
    evidence.write(&png_name, &out.bytes)?;
    let samples=case.probes().iter().map(|(label,x,y)|json!({"label":label,"world":[x,y],"decoded_straight_rgba":decoded.get_pixel(*x as u32,*y as u32).0})).collect::<Vec<_>>();
    let unchanged = before == canonical(scene)?;
    evidence.json(&format!("{tag}.json"),&json!({"kind":"independent_cpu_export","png":png_name,
        "encoded_mime":"image/png","encoded_bytes":out.bytes.len(),"encoded_fnv1a64":fingerprint(&out.bytes),
        "decoded_dimensions":decoded.dimensions(),"expected_dimensions":[960,380],"decoded_rgba_bytes":decoded.as_raw().len(),
        "decoded_straight_rgba_fnv1a64":fingerprint(decoded.as_raw()),"source_unchanged":unchanged,"samples":samples,
        "world_crop":[0,0,WORLD_W,WORLD_H],"scale":1,"synthetic_dpr":1,
        "native_application_created":false,"native_framebuffer_captured":false}))?;
    if !unchanged || decoded.dimensions() != (960, 380) {
        return Err("CPU source or dimensions changed".into());
    }
    Ok(decoded)
}
fn assert_sample(
    rows: &mut Vec<Value>,
    img: &image::RgbaImage,
    label: &str,
    x: u32,
    y: u32,
    expected: [u8; 4],
    tolerance: u8,
) -> Result<(), String> {
    let actual = img.get_pixel(x, y).0;
    let pass = actual
        .into_iter()
        .zip(expected)
        .all(|(a, b)| a.abs_diff(b) <= tolerance);
    rows.push(json!({"label":label,"xy":[x,y],"actual_straight_rgba":actual,"expected_straight_rgba":expected,"tolerance":tolerance,"pass":pass}));
    if pass {
        Ok(())
    } else {
        Err(format!(
            "{label}: actual {actual:?}, expected {expected:?} tolerance {tolerance}"
        ))
    }
}
fn cpu_tour(evidence: &mut Evidence) -> Result<(), String> {
    let primitive = primitive_audit()?;
    evidence.json("primitive-premul.json", &primitive)?;
    let mut assertions = vec![];
    for case in [Case::Leaves, Case::Group, Case::Nested, Case::Alpha] {
        let scene = document(case).map_err(|e| e.to_string())?;
        let tag = format!("{:?}", case).to_lowercase();
        let image = cpu_file(evidence, &scene, case, &tag)?;
        match case {
            Case::Leaves => {
                for (col, amount) in [0., 0.5, 1.].into_iter().enumerate() {
                    for (row, input) in [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]]
                        .into_iter()
                        .enumerate()
                    {
                        assert_sample(
                            &mut assertions,
                            &image,
                            "primary",
                            80 + 300 * col as u32,
                            70 + 65 * row as u32,
                            expected_gray(input, amount),
                            0,
                        )?;
                    }
                }
                assert_sample(
                    &mut assertions,
                    &image,
                    "low-alpha red grayscale",
                    680,
                    300,
                    [54, 54, 54, 128],
                    1,
                )?;
                assert_sample(
                    &mut assertions,
                    &image,
                    "own outer magenta shadow filtered",
                    245,
                    342,
                    [73, 73, 73, 255],
                    0,
                )?;
                assert_sample(
                    &mut assertions,
                    &image,
                    "own inner red shadow filtered",
                    195,
                    295,
                    [54, 54, 54, 255],
                    0,
                )?;
            }
            Case::Group => {
                assert_sample(
                    &mut assertions,
                    &image,
                    "group red once opacity",
                    110,
                    110,
                    [27, 27, 154, 255],
                    1,
                )?;
                assert_sample(
                    &mut assertions,
                    &image,
                    "group overlap green once opacity",
                    210,
                    150,
                    [91, 91, 218, 255],
                    1,
                )?;
                assert_sample(
                    &mut assertions,
                    &image,
                    "outside backdrop",
                    60,
                    70,
                    [0, 0, 255, 255],
                    0,
                )?;
                assert_sample(
                    &mut assertions,
                    &image,
                    "later sibling",
                    350,
                    240,
                    [255, 0, 255, 255],
                    0,
                )?;
            }
            Case::Nested => {
                assert_sample(
                    &mut assertions,
                    &image,
                    "child .5 then Box .5",
                    145,
                    140,
                    [105, 41, 41, 255],
                    0,
                )?;
                assert_sample(
                    &mut assertions,
                    &image,
                    "child .5 then Frame .5",
                    605,
                    140,
                    [105, 41, 41, 255],
                    0,
                )?;
                assert_sample(
                    &mut assertions,
                    &image,
                    "outside rounded ancestor",
                    62,
                    47,
                    [0, 0, 0, 0],
                    0,
                )?;
                assert_sample(
                    &mut assertions,
                    &image,
                    "Box stroke before opaque child",
                    289,
                    140,
                    [105, 41, 41, 255],
                    0,
                )?;
                assert_sample(
                    &mut assertions,
                    &image,
                    "Frame stroke before opaque child",
                    749,
                    140,
                    [105, 41, 41, 255],
                    0,
                )?;
                let mut after = scene.clone();
                variant(&mut after, case).map_err(|e| e.to_string())?;
                let changed = cpu_file(evidence, &after, case, "nested-stroke-after")?;
                assert_sample(
                    &mut assertions,
                    &changed,
                    "Box stroke after child then grayscale",
                    289,
                    140,
                    [164, 36, 164, 255],
                    0,
                )?;
                assert_sample(
                    &mut assertions,
                    &changed,
                    "Frame stroke after child then grayscale",
                    749,
                    140,
                    [164, 36, 164, 255],
                    0,
                )?;
            }
            Case::Alpha => {
                assert_sample(
                    &mut assertions,
                    &image,
                    "alpha mask coverage",
                    100,
                    130,
                    [54, 54, 54, 128],
                    1,
                )?;
                assert_sample(
                    &mut assertions,
                    &image,
                    "outside alpha mask",
                    260,
                    130,
                    [0, 0, 0, 0],
                    0,
                )?;
                assert_sample(
                    &mut assertions,
                    &image,
                    "Pixel leaves half-alpha",
                    455,
                    130,
                    [54, 54, 54, 127],
                    1,
                )?;
                assert_sample(
                    &mut assertions,
                    &image,
                    "Path ignores source alpha",
                    755,
                    130,
                    [0, 0, 0, 0],
                    0,
                )?;
                assert_sample(
                    &mut assertions,
                    &image,
                    "later surviving child",
                    475,
                    235,
                    [182, 182, 182, 255],
                    0,
                )?;
                let mut zero = scene.clone();
                amount(&mut zero, case, 0.).map_err(|e| e.to_string())?;
                let other = cpu_file(evidence, &zero, case, "alpha-source-amount-zero")?;
                if image != other {
                    return Err(
                        "mask/Pixel/Path RGB-only source changes altered final image".into(),
                    );
                }
                assertions.push(json!({"rule":"mask/Pixel/Path source grayscale preserves coverage and final output","all_decoded_bytes_equal":true}));
            }
            Case::Reject => unreachable!(),
        }
    }
    let mut unsafe_scene = document(Case::Reject).map_err(|e| e.to_string())?;
    let before = canonical(&unsafe_scene)?;
    let error = encode(&unsafe_scene).err();
    let expected = CompositeError::UnsupportedGrayscaleBackdrop("focus".into());
    evidence.json("unsafe-rejection.json",&json!({"error":error.as_ref().map(|e|format!("{e:?}")),"expected":format!("{expected:?}"),"source_unchanged":before==canonical(&unsafe_scene)?,"output_file_published":false}))?;
    if error != Some(expected) || before != canonical(&unsafe_scene)? {
        return Err("unsafe backdrop typed error/source atomicity mismatch".into());
    }
    variant(&mut unsafe_scene, Case::Reject).map_err(|e| e.to_string())?;
    let restored = cpu_file(evidence, &unsafe_scene, Case::Reject, "normal-recovery")?;
    assert_sample(
        &mut assertions,
        &restored,
        "Normal recovery blue source",
        150,
        130,
        [18, 18, 18, 255],
        0,
    )?;
    assert_sample(
        &mut assertions,
        &restored,
        "Normal recovery external red",
        75,
        65,
        [255, 0, 0, 255],
        0,
    )?;
    evidence.json("summary.json",&json!({"kind":"CPU contract checks","assertions":assertions,"native_acceptance":"not performed","png_exports":7,"budgets":"unchanged defaults"}))
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
        let expected = CompositeError::UnsupportedGrayscaleBackdrop("focus".into());
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
        SceneCommand::LoadScene(document(Case::Group).map_err(|e| e.to_string())?),
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
            ("above-one", 1.1),
            ("NaN", f32::NAN),
            ("infinity", f32::INFINITY),
        ] {
            let revision = editor.engine.revision();
            let r = editor.transaction(|s| {
                s.update_object(&"r2".into(), |o| {
                    o.visible = !hidden;
                    o.style.opacity = opacity;
                    o.style.grayscale = value;
                })
            });
            let atomic = r.is_err()
                && editor.engine.revision() == revision
                && canonical(editor.scene())? == edited;
            rejects.push(json!({"hidden":hidden,"opacity":opacity,"invalid":name,"typed_error":r.err().map(|e|format!("{e:?}")),"atomic":atomic}));
            if !atomic {
                return Err("invalid retained grayscale mutation was not atomic".into());
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
    evidence.json("headless.json",&json!({"kind":"public pure fixture helper","documents_valid":5,"readonly_load_scene_denied":true,
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
                        size(px(1060.), px(770.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                |window, cx| {
                    window.set_window_title("gpui-flow · bounded grayscale");
                    cx.new(|cx| Lab::new(evidence, cx))
                },
            )
            .expect("native window creation failed");
            cx.activate(true);
        });
}
