//! Native startup-registration / shaped-run acceptance only; no outline stroking.
//! The native panels paint the initial bound candidates' exact GPUI WrappedLines.
//! Post-registration receipts inspect freshly shaped lines; their display is not claimed.
use gpui::{prelude::*, *};
use gpui_flow::ControlButton;
use gpui_flow_text::{
    GpuiTextRasterizer,
    outline_registration::{
        OutlineBindingError, StartupOutlineFont, application_with_static_outlines,
    },
    static_outlines::{PreparedFont, StaticStyle},
};
use serde_json::{Value, json};
use std::{
    borrow::Cow,
    fmt::Write as _,
    io::Write as _,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

const TTF: &[u8] =
    include_bytes!("../../../../crates/flow-text/tests/fixtures/outline-quadratic.ttf");
const CFF: &[u8] = include_bytes!("../../../../crates/flow-text/tests/fixtures/outline-cubic.otf");
const TEXT: &str = "m c é m c é m c é m c é";
const WIDTH: f32 = 340.;
const LABELS: [&str; 11] = [
    "TTF bound",
    "CFF bound",
    "Both + wraps",
    "Fallback only",
    "Mixed runs",
    "Wrong owner",
    "Wrong style",
    "Cancelled",
    "Invalidate + fresh",
    "Add unrelated",
    "Add same alias",
];
const IDS: [&str; 11] = [
    "ttf",
    "cff",
    "both",
    "fallback",
    "mixed",
    "owner",
    "style",
    "cancel",
    "epoch",
    "unrelated",
    "same-alias",
];
fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut out, "{byte:02x}").unwrap();
    }
    out
}
fn identity(font: &PreparedFont) -> Value {
    let id = font.identity();
    json!({"private_family":font.private_alias(), "source_sha256":hex(&id.source_sha256),
        "registration_sha256":hex(&id.registration_sha256), "face_index":id.face_index,
        "weight":id.style.weight, "italic":id.style.italic,
        "units_per_em":font.units_per_em(), "glyph_count":font.glyph_count(),
        "registration_bytes":font.bytes().len()})
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    file.write_all(bytes).map_err(|e| e.to_string())
}

struct Trace {
    directory: PathBuf,
    sequence: u64,
}
impl Trace {
    fn new() -> Result<Self, String> {
        let root = std::env::var_os("FLOW_OUTLINE_BINDING_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("gpui-flow-outline-binding"));
        let session = format!(
            "{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_nanos()
        );
        let directory = root.join(session);
        std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
        Ok(Self {
            directory,
            sequence: 0,
        })
    }
    fn emit(&mut self, action: &str, evidence: Value) -> Result<(), String> {
        self.sequence += 1;
        let receipt = json!({"fixture":"outline-font-binding", "schema":1,
            "sequence":self.sequence, "action":action, "evidence":evidence,
            "scope":"native registration and binding-level candidate checks; no Canvas publication or stroke rasterization",
            "directory":self.directory.to_string_lossy()});
        println!("{receipt}");
        write_new(
            &self
                .directory
                .join(format!("{:03}-{action}.json", self.sequence)),
            &serde_json::to_vec_pretty(&receipt).map_err(|e| e.to_string())?,
        )
    }
}

struct Sample {
    descriptor: Font,
    app_id: FontId,
    size: f32,
    line_height: f32,
    lines: Arc<Vec<WrappedLine>>,
}
fn shape(
    system: &Arc<TextSystem>,
    descriptor: &Font,
    text: &str,
    size: f32,
    width: Option<f32>,
) -> Result<Vec<WrappedLine>, String> {
    // No independent shaper: this wrapper holds the factory App's exact Arc.
    WindowTextSystem::new(system.clone())
        .shape_text(
            text.to_owned().into(),
            px(size),
            &[TextRun {
                len: text.len(),
                font: descriptor.clone(),
                color: rgb(0x13384d).into(),
                ..Default::default()
            }],
            width.map(px),
            None,
        )
        .map(|lines| lines.into_vec())
        .map_err(|e| e.to_string())
}

#[derive(Default, Debug, PartialEq, Eq)]
struct Decision {
    checked: usize,
    bound: usize,
    unbound: usize,
    other_errors: usize,
    glyphs: usize,
    missing_glyphs: usize,
}
impl Decision {
    fn accepted(&self, fresh: bool) -> bool {
        self.checked > 0
            && self.glyphs > 0
            && self.missing_glyphs == 0
            && self.bound == self.checked
            && self.unbound == 0
            && self.other_errors == 0
            && fresh
    }
    fn expected_fallback(&self, mixed: bool) -> bool {
        self.checked > 0
            && self.unbound > 0
            && self.other_errors == 0
            && self.glyphs > 0
            && self.missing_glyphs == 0
            && (!mixed || self.bound > 0)
    }
}
struct Candidate {
    evidence: Value,
    decision: Decision,
    accepted: bool,
    rows: usize,
}
struct Lab {
    bridge: Arc<GpuiTextRasterizer>,
    system: Arc<TextSystem>,
    fonts: [Arc<PreparedFont>; 2],
    samples: [Sample; 2],
    trace: Trace,
    statuses: Vec<String>,
    last: String,
    unrelated_added: bool,
    same_alias_added: bool,
}
impl Lab {
    fn new(
        bridge: Arc<GpuiTextRasterizer>,
        system: Arc<TextSystem>,
        fonts: [Arc<PreparedFont>; 2],
        mut trace: Trace,
    ) -> Result<Self, String> {
        if !bridge.matches_text_system(&system) {
            return Err("wrong startup App Arc".into());
        }
        let descriptors = bridge
            .static_outline_descriptors()
            .map_err(|e| e.to_string())?;
        if descriptors.len() != 2 {
            return Err("expected exactly two bound descriptors".into());
        }
        let mut values = Vec::new();
        for (ix, descriptor) in descriptors.into_iter().enumerate() {
            let size = [28., 38.][ix];
            values.push(Sample {
                app_id: system.resolve_font(&descriptor),
                lines: Arc::new(shape(&system, &descriptor, TEXT, size, Some(WIDTH))?),
                descriptor,
                size,
                line_height: size * 1.25,
            });
        }
        let samples: [Sample; 2] = values.try_into().map_err(|_| "sample count")?;
        if samples[0].app_id == samples[1].app_id {
            return Err("duplicate App font ID".into());
        }
        for (ix, font) in fonts.iter().enumerate() {
            write_new(
                &trace.directory.join(format!("{ix}-registered.font")),
                font.bytes(),
            )?;
            write_new(
                &trace.directory.join(format!("{ix}-source.font")),
                [TTF, CFF][ix],
            )?;
        }
        trace.emit("startup", json!({"factory":"application_with_static_outlines",
            "factory_returned_complete_batch":true, "same_app_arc":true,
            "direct_to_app_identity":"verified inside startup factory; private direct IDs are not exposed",
            "independently_observed_app_ids":samples.iter().map(|s|s.app_id.0).collect::<Vec<_>>(),
            "fonts":fonts.iter().map(|f|identity(f)).collect::<Vec<_>>(),
            "source_lengths":[TTF.len(),CFF.len()], "epoch":bridge.epoch(),
            "linux":cfg!(target_os="linux"), "x11_opt_in":cfg!(feature="outline-font-binding-x11"),
            "wayland_opt_in":cfg!(feature="outline-font-binding-wayland"),
            "source_and_prepared_bytes_saved":true}))?;
        Ok(Self {
            bridge,
            system,
            fonts,
            samples,
            trace,
            statuses: vec!["Not run".into(); LABELS.len()],
            last: "Click every check; JSON and exact font bytes are saved per session".into(),
            unrelated_added: false,
            same_alias_added: false,
        })
    }
    fn candidate(
        &self,
        lines: &[WrappedLine],
        owner: &Arc<TextSystem>,
        style: StaticStyle,
        epoch: u64,
        cancelled: &AtomicBool,
    ) -> Candidate {
        let mut decision = Decision::default();
        let mut evidence = Vec::new();
        let mut rows = 0;
        // Deliberately never use an early-returning collect::<Result<...>>() here.
        // Even an unbound first run must not hide the remaining actual runs.
        for (line_ix, line) in lines.iter().enumerate() {
            rows += line.wrap_boundaries.len() + 1;
            let layout = &line.unwrapped_layout;
            let mut runs = Vec::new();
            for (run_ix, run) in layout.runs.iter().enumerate() {
                decision.checked += 1;
                decision.glyphs += run.glyphs.len();
                decision.missing_glyphs += run.glyphs.iter().filter(|g| g.id.0 == 0).count();
                let lookup = match self.bridge.static_outline_font(
                    owner,
                    run.font_id,
                    style,
                    epoch,
                    cancelled,
                ) {
                    Ok(font) => {
                        let original = self.fonts.iter().find(|f| std::ptr::eq(f.as_ref(), font));
                        let in_range = run
                            .glyphs
                            .iter()
                            .all(|g| g.id.0 < u32::from(font.glyph_count()));
                        if original.is_some() && in_range {
                            decision.bound += 1;
                        } else {
                            decision.other_errors += 1;
                        }
                        json!({"result":"bound", "immutable_startup_object":original.is_some(),
                            "glyph_ids_in_bound_face_range":in_range, "identity":identity(font)})
                    }
                    Err(error) => {
                        if error == OutlineBindingError::UnboundFont {
                            decision.unbound += 1;
                        } else {
                            decision.other_errors += 1;
                        }
                        json!({"result":"rejected", "error":format!("{error:?}")})
                    }
                };
                runs.push(json!({"run":run_ix, "actual_font_id":run.font_id.0, "lookup":lookup,
                    "glyphs":run.glyphs.iter().map(|g|json!({"id":g.id.0,"text_byte_index":g.index,
                        "position":[f32::from(g.position.x),f32::from(g.position.y)], "emoji":g.is_emoji})).collect::<Vec<_>>()}));
            }
            evidence.push(json!({"line":line_ix, "text":line.text.to_string(),
                "font_size":f32::from(layout.font_size), "unwrapped_width":f32::from(layout.width),
                "ascent":f32::from(layout.ascent), "descent":f32::from(layout.descent),
                "wrap_width":line.wrap_width.map(f32::from),
                "wrap_boundaries":line.wrap_boundaries.iter().map(|b|json!({"run":b.run_ix,"glyph":b.glyph_ix})).collect::<Vec<_>>(),
                "runs":runs}));
        }
        let fresh = self.bridge.epoch() == epoch && !cancelled.load(Ordering::Acquire);
        let accepted = decision.accepted(fresh);
        Candidate {
            evidence: json!({"lines":evidence, "rows":rows,"runs_checked":decision.checked,
            "bound_runs":decision.bound,"unbound_runs":decision.unbound,"other_errors":decision.other_errors,
            "glyphs":decision.glyphs,"missing_glyphs":decision.missing_glyphs,
            "same_app_arc":self.bridge.matches_text_system(owner),"requested_style":{"weight":style.weight,"italic":style.italic},
            "snapshot_epoch":epoch,"final_epoch":self.bridge.epoch(),"final_freshness_check":fresh,
            "all_actual_runs_collected_before_decision":true,"binding_candidate_accepted":accepted}),
            decision,
            accepted,
            rows,
        }
    }
    fn bound(&self, ix: usize) -> Candidate {
        self.candidate(
            &self.samples[ix].lines,
            &self.system,
            self.fonts[ix].identity().style,
            self.bridge.epoch(),
            &AtomicBool::new(false),
        )
    }
    fn binding_lookup(
        &self,
        owner: &Arc<TextSystem>,
        style: StaticStyle,
        epoch: u64,
        cancelled: bool,
    ) -> (Value, Option<OutlineBindingError>) {
        let result = self.bridge.static_outline_font(
            owner,
            self.samples[0].app_id,
            style,
            epoch,
            &AtomicBool::new(cancelled),
        );
        match result {
            Ok(font) => (
                json!({"result":"bound","font_id":self.samples[0].app_id.0,
                "same_app_arc":self.bridge.matches_text_system(owner),"requested_weight":style.weight,"requested_italic":style.italic,
                "snapshot_epoch":epoch,"current_epoch":self.bridge.epoch(),"cancelled":cancelled,"identity":identity(font)}),
                None,
            ),
            Err(error) => (
                json!({"result":"rejected","font_id":self.samples[0].app_id.0,
                "same_app_arc":self.bridge.matches_text_system(owner),"requested_weight":style.weight,"requested_italic":style.italic,
                "snapshot_epoch":epoch,"current_epoch":self.bridge.epoch(),"cancelled":cancelled,"error":format!("{error:?}")}),
                Some(error),
            ),
        }
    }
    fn stable_after_registration(&self) -> (bool, Value) {
        let mut ok = true;
        let mut faces = Vec::new();
        for ix in 0..2 {
            let current_id = self.system.resolve_font(&self.samples[ix].descriptor);
            let current = shape(
                &self.system,
                &self.samples[ix].descriptor,
                TEXT,
                self.samples[ix].size,
                Some(WIDTH),
            );
            match current {
                Ok(lines) => {
                    let candidate = self.candidate(
                        &lines,
                        &self.system,
                        self.fonts[ix].identity().style,
                        self.bridge.epoch(),
                        &AtomicBool::new(false),
                    );
                    let baseline = self.bound(ix);
                    let unchanged_shape = candidate.evidence["lines"] == baseline.evidence["lines"];
                    ok &= current_id == self.samples[ix].app_id
                        && candidate.accepted
                        && unchanged_shape;
                    faces.push(json!({"face":ix,"before_id":self.samples[ix].app_id.0,"after_id":current_id.0,
                        "unchanged_id":current_id == self.samples[ix].app_id,"unchanged_shaped_ids_glyphs_positions_and_metrics":unchanged_shape,"candidate":candidate.evidence}));
                }
                Err(error) => {
                    ok = false;
                    faces.push(json!({"face":ix,"shape_error":error}));
                }
            }
        }
        (ok, json!(faces))
    }
    fn check(&mut self, ix: usize) -> Result<(bool, &'static str, Value), String> {
        let style = self.fonts[0].identity().style;
        let epoch = self.bridge.epoch();
        match ix {
            0 | 1 => {
                let c = self.bound(ix);
                let id = self.samples[ix].app_id;
                let exact = self.samples[ix]
                    .lines
                    .iter()
                    .flat_map(|l| &l.unwrapped_layout.runs)
                    .all(|r| r.font_id == id);
                Ok((
                    c.accepted && exact,
                    "accepted",
                    json!({"app_resolved_id":id.0,"all_shaped_ids_equal_app_id":exact,"candidate":c.evidence}),
                ))
            }
            2 => {
                let a = self.bound(0);
                let b = self.bound(1);
                Ok((
                    a.accepted && b.accepted && a.rows > 1 && b.rows > 1,
                    "accepted",
                    json!({"different_app_ids":self.samples[0].app_id != self.samples[1].app_id,"faces":[a.evidence,b.evidence]}),
                ))
            }
            3 | 4 => {
                let text = if ix == 3 { "WQ" } else { "mc WQ mc" };
                // These characters are absent from both tiny admitted faces. Native
                // fallback must expose an unbound ID; an environment without one
                // fails this check rather than claiming an expected rejection.
                let lines = shape(&self.system, &self.samples[0].descriptor, text, 28., None)?;
                let c = self.candidate(&lines, &self.system, style, epoch, &AtomicBool::new(false));
                Ok((
                    !c.accepted && c.decision.expected_fallback(ix == 4),
                    "expected rejection",
                    c.evidence,
                ))
            }
            5 => {
                let noop = Arc::new(TextSystem::new(Arc::new(NoopTextSystem::new())));
                let (lookup, error) = self.binding_lookup(&noop, style, epoch, false);
                Ok((
                    error == Some(OutlineBindingError::WrongTextSystem),
                    "expected rejection",
                    json!({"owner":"separate Noop TextSystem; no second native application","reused_real_numeric_id":true,"lookup":lookup}),
                ))
            }
            6 => {
                let mut wrong_weight = style;
                wrong_weight.weight = if style.weight == 700 { 400 } else { 700 };
                let mut wrong_slant = style;
                wrong_slant.italic = !style.italic;
                let (weight, we) = self.binding_lookup(&self.system, wrong_weight, epoch, false);
                let (slant, se) = self.binding_lookup(&self.system, wrong_slant, epoch, false);
                Ok((
                    we == Some(OutlineBindingError::StyleMismatch)
                        && se == Some(OutlineBindingError::StyleMismatch),
                    "expected rejection",
                    json!({"wrong_weight":weight,"wrong_slant":slant}),
                ))
            }
            7 => {
                let cancelled = self.candidate(
                    &self.samples[0].lines,
                    &self.system,
                    style,
                    epoch,
                    &AtomicBool::new(true),
                );
                let (lookup, error) = self.binding_lookup(&self.system, style, epoch, true);
                let recovery = self.bound(0);
                Ok((
                    !cancelled.accepted
                        && error == Some(OutlineBindingError::StaleSnapshot)
                        && recovery.accepted,
                    "expected rejection",
                    json!({"cancelled":cancelled.evidence,"lookup":lookup,"fresh_recovery":recovery.evidence}),
                ))
            }
            8 => {
                self.bridge.invalidate();
                let (stale, error) = self.binding_lookup(&self.system, style, epoch, false);
                let fresh = self.bound(0);
                let second = self.bound(1);
                Ok((
                    error == Some(OutlineBindingError::StaleSnapshot)
                        && fresh.accepted
                        && second.accepted,
                    "rejected + recovered",
                    json!({"before_epoch":epoch,"after_epoch":self.bridge.epoch(),"stale":stale,"fresh_faces":[fresh.evidence,second.evidence]}),
                ))
            }
            9 | 10 => {
                let already_added = if ix == 9 {
                    self.unrelated_added
                } else {
                    self.same_alias_added
                };
                if !already_added {
                    let bytes = if ix == 9 {
                        Cow::Borrowed(TTF)
                    } else {
                        Cow::Owned(self.fonts[0].bytes().to_vec())
                    };
                    self.system
                        .add_fonts(vec![bytes])
                        .map_err(|e| e.to_string())?;
                    self.bridge.invalidate();
                    if ix == 9 {
                        self.unrelated_added = true;
                    } else {
                        self.same_alias_added = true;
                    }
                }
                let (stable, faces) = self.stable_after_registration();
                let unrelated = if ix == 9 {
                    let mut descriptor = font("Flow Fixture");
                    descriptor.fallbacks = Some(FontFallbacks::default());
                    let id = self.system.resolve_font(&descriptor);
                    let rejected = self.bridge.static_outline_font(
                        &self.system,
                        id,
                        style,
                        self.bridge.epoch(),
                        &AtomicBool::new(false),
                    );
                    let unbound = matches!(rejected, Err(OutlineBindingError::UnboundFont));
                    let resolved_family = self
                        .system
                        .get_font_for_id(id)
                        .map(|font| font.family.to_string());
                    let family_visible = self
                        .system
                        .all_font_names()
                        .iter()
                        .any(|name| name == "Flow Fixture");
                    (
                        unbound
                            && family_visible
                            && resolved_family.as_deref() == Some("Flow Fixture"),
                        json!({"resolved_id":id.0,"resolved_family":resolved_family,"family_visible":family_visible,"not_bound":unbound,"result":format!("{rejected:?}")}),
                    )
                } else {
                    (true, Value::Null)
                };
                Ok((
                    stable && unrelated.0,
                    "accepted",
                    json!({"already_added_no_repeat_allocation":already_added,
                    "added":"one immutable buffer", "same_alias_uses_identical_registered_bytes":ix == 10,
                    "source_sha256":hex(&self.fonts[0].identity().source_sha256),
                    "added_buffer_sha256":hex(if ix == 9 {&self.fonts[0].identity().source_sha256} else {&self.fonts[0].identity().registration_sha256}),
                    "epoch":self.bridge.epoch(),"unchanged_bindings":faces,"unrelated_lookup":unrelated.1}),
                ))
            }
            _ => Err("unknown check".into()),
        }
    }
    fn click(&mut self, ix: usize, cx: &mut Context<Self>) {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.check(ix)));
        let (passed, label, evidence) = match result {
            Ok(Ok(value)) => value,
            Ok(Err(error)) => (false, "failed", json!({"error":error})),
            Err(_) => (false, "failed", json!({"error":"native check panicked"})),
        };
        self.statuses[ix] = format!("{}: {label}", if passed { "PASS" } else { "FAIL" });
        self.last = format!(
            "{} · {} · epoch {}",
            LABELS[ix],
            self.statuses[ix],
            self.bridge.epoch()
        );
        if let Err(error) = self.trace.emit(
            IDS[ix],
            json!({"passed":passed,"expected_outcome":label,"details":evidence}),
        ) {
            self.statuses[ix] = "FAIL: trace save".into();
            self.last = error;
        }
        cx.notify();
    }
}
impl Render for Lab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut previews = div().flex().gap_3().h(px(190.)).flex_shrink_0();
        for ix in 0..2 {
            let lines = self.samples[ix].lines.clone();
            let height = self.samples[ix].line_height;
            previews = previews.child(div().flex_1().min_w_0().p_3().rounded_md().bg(rgb(0xe7eef1))
                .text_color(rgb(0x13384d)).child(format!("{} · {}px · App FontId {}",
                    if ix == 0 {"TrueType quadratic"} else {"CFF cubic"},self.samples[ix].size,self.samples[ix].app_id.0))
                .child(div().text_xs().child("Native m / c / é / space · wrapped geometric glyphs"))
                .child(div().text_xs().child(self.samples[ix].descriptor.family.to_string()))
                .child(canvas(|_,_,_|(),move |bounds,_,window,cx| {
                    let mut origin = bounds.origin;
                    for line in lines.iter() {
                        if let Err(error) = line.paint(origin,px(height),TextAlign::Left,None,window,cx) {
                            eprintln!("{}",json!({"fixture":"outline-font-binding","paint_error":error.to_string()}));
                        }
                        origin.y += px(height * (line.wrap_boundaries.len()+1) as f32);
                    }
                }).w(px(WIDTH)).h(px(115.))));
        }
        let mut controls = div().flex().flex_col().gap_2();
        for row in 0..4 {
            let mut row_view = div().flex().gap_3().h(px(60.)).flex_shrink_0();
            for col in 0..3 {
                let ix = row * 3 + col;
                let mut cell = div().flex_1().min_w_0();
                if ix < LABELS.len() {
                    cell = cell
                        .child(
                            ControlButton::new(IDS[ix], LABELS[ix])
                                .on_click(cx.listener(move |s, _, _, cx| s.click(ix, cx))),
                        )
                        .child(div().text_xs().child(self.statuses[ix].clone()));
                }
                row_view = row_view.child(cell);
            }
            controls = controls.child(row_view);
        }
        div().size_full().p_4().flex().flex_col().gap_2().bg(rgb(0x142331)).text_color(rgb(0xe5eef5))
            .child(div().text_lg().child("Static font binding · native acceptance"))
            .child(div().text_sm().child("Same-App shaping and immutable startup faces. Binding checks only; no strokeText or Canvas pixel transaction."))
            .child(previews).child(controls)
            .child(div().text_sm().child(self.last.clone()))
            .child(div().text_xs().child("Expected rejections preserve the displayed samples. Repeat checks after mutations; JSON records every actual run."))
            .child(div().text_xs().child(format!("Trace: {}",self.trace.directory.display())))
    }
}

fn helper_tour() -> Result<Value, String> {
    let a = PreparedFont::prepare(TTF).map_err(|e| e.to_string())?;
    let b = PreparedFont::prepare(CFF).map_err(|e| e.to_string())?;
    if a.private_alias() == b.private_alias()
        || a.identity().source_sha256 == b.identity().source_sha256
    {
        return Err("fixture identities must differ".into());
    }
    let bound = Decision {
        checked: 2,
        bound: 2,
        glyphs: 6,
        ..Default::default()
    };
    let mixed = Decision {
        checked: 3,
        bound: 2,
        unbound: 1,
        glyphs: 6,
        ..Default::default()
    };
    if !bound.accepted(true)
        || bound.accepted(false)
        || mixed.accepted(true)
        || !mixed.expected_fallback(true)
        || Decision::default().accepted(true)
    {
        return Err("candidate reduction contract".into());
    }
    Ok(
        json!({"status":"passed","scope":"headless preparation and decision helpers only; no native registration, shaping or paint",
        "fonts":[identity(&a),identity(&b)],"buttons":IDS,"native_constructed":false}),
    )
}
fn run() -> Result<(), String> {
    if std::env::args().any(|arg| arg == "--helper-tour") {
        println!("{}", helper_tour()?);
        return Ok(());
    }
    let fonts = [
        Arc::new(PreparedFont::prepare(TTF).map_err(|e| e.to_string())?),
        Arc::new(PreparedFont::prepare(CFF).map_err(|e| e.to_string())?),
    ];
    let requests = fonts
        .iter()
        .map(|font| StartupOutlineFont {
            font: font.clone(),
            style: font.identity().style,
        })
        .collect();
    let (app, bridge) = application_with_static_outlines(requests).map_err(|e| e.to_string())?;
    let system = app.text_system();
    let lab = Lab::new(bridge, system, fonts, Trace::new()?)?;
    app.with_assets(gpui_kit::assets::Assets).run(move |cx| {
        gpui_kit::init(cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1000.), px(800.)),
                    cx,
                ))),
                ..Default::default()
            },
            move |window, cx| {
                window.set_window_title("gpui-flow · static font binding");
                cx.new(|_| lab)
            },
        )
        .expect("open outline binding fixture window");
        cx.activate(true);
    });
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!(
            "{}",
            json!({"fixture":"outline-font-binding","startup":"failed","error":error})
        );
        std::process::exit(1);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    #[test]
    fn helper_tour_is_headless_and_checks_both_actual_font_sources() {
        let trace = helper_tour().unwrap();
        assert_eq!(trace["native_constructed"], false);
        assert_eq!(trace["fonts"].as_array().unwrap().len(), 2);
    }
    #[test]
    fn reducer_requires_every_run_and_final_freshness() {
        for bad_ix in 0..3 {
            let mut d = Decision::default();
            for ix in 0..3 {
                d.checked += 1;
                d.glyphs += 1;
                if ix == bad_ix {
                    d.unbound += 1;
                } else {
                    d.bound += 1;
                }
            }
            assert_eq!(d.checked, 3);
            assert!(!d.accepted(true));
            assert!(d.expected_fallback(true));
        }
        assert!(
            !Decision {
                checked: 1,
                bound: 1,
                glyphs: 1,
                ..Default::default()
            }
            .accepted(false)
        );
    }
    #[test]
    fn empty_or_missing_glyphs_do_not_count_as_fallback_acceptance() {
        assert!(!Decision::default().accepted(true));
        assert!(
            !Decision {
                checked: 1,
                unbound: 1,
                glyphs: 1,
                missing_glyphs: 1,
                ..Default::default()
            }
            .expected_fallback(false)
        );
        assert!(
            !Decision {
                checked: 1,
                unbound: 1,
                glyphs: 1,
                ..Default::default()
            }
            .expected_fallback(true)
        );
    }
    #[test]
    fn source_byte_hashes_and_lengths_are_frozen() {
        assert_eq!([TTF.len(), CFF.len()], [1540, 1576]);
        for (bytes, expected) in [
            (
                TTF,
                "315383d2601dcd74cbbaa73d7b897e38997123668a12f5e664403ffde4af4429",
            ),
            (
                CFF,
                "cfb44080e7dd0a928b1baf5e870e3b382a71ff43049b3e25c6b9e8bf31537b71",
            ),
        ] {
            assert_eq!(
                hex(&PreparedFont::prepare(bytes)
                    .unwrap()
                    .identity()
                    .source_sha256),
                expected
            );
        }
    }
}
