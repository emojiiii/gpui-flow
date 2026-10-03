//! Actual same-App StrokeTextBox commands, owned Canvas publication, and lossless evidence.
//! This opt-in X11 fixture does not inject cancellation during rasterization/upload.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, SceneView,
    composite::{
        CanvasCommand, CanvasStrokeOptions, CanvasSurface, CompositeError, ImageInputs, StaticImage,
    },
    resources::CanvasEditError,
    scene::{
        Affine, Color, DisplayObject, FillRule, Point as ScenePoint, Scene, Shape,
        Size as SceneSize, Stroke, StrokeCap, StrokeJoin, Style, display,
    },
};
use gpui_flow_text::{
    GpuiTextRasterizer,
    outline_registration::{
        OutlineBindingError, StartupOutlineFont, application_with_static_outlines,
    },
    static_outlines::{OutlineBudget, OutlineCommand, PreparedFont, StaticStyle},
};
use serde_json::{Value, json};
use std::{
    fmt::Write as _,
    io::Write as _,
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
    time::{SystemTime, UNIX_EPOCH},
};

const TTF: &[u8] =
    include_bytes!("../../../../crates/flow-text/tests/fixtures/stroke-quadratic.ttf");
const CFF: &[u8] = include_bytes!("../../../../crates/flow-text/tests/fixtures/stroke-cubic.otf");
const WIDTH: f32 = 440.;
const HEIGHT: f32 = 340.;
const IDS: [&str; 2] = ["one", "two"];
const SCENARIOS: [&str; 4] = [
    "Curves + counters",
    "Typography",
    "Pens + affine",
    "Clips + alpha",
];
const NOTES: [&str; 4] = [
    "TTF gold / CFF cyan: curved m, O/B counters, composite é; fi / AV / combining acute below",
    "Left boxes: wrap + centered padding + paragraph spacing. Right boxes: upper/lower case + end ellipsis",
    "Three dashed rows: butt/miter, round/round, square/bevel. Bottom: rotation, skew, reflection + nonuniform scale",
    "Frozen current-path even-odd clip + nested clip; restored right stripe. Bottom: translucent overlapping glyph contours",
];
fn point(x: f32, y: f32) -> ScenePoint {
    ScenePoint::new(x, y)
}
fn path(svg: &str) -> display::Path {
    display::Path::from_svg(svg).unwrap()
}
fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut out, "{byte:02x}").unwrap();
    }
    out
}
// Noncryptographic receipt checksum; exact byte comparisons are authoritative.
// Font SHA-256 identities come from PreparedFont. No new dependency/lockfile graph.
fn checksum(bytes: &[u8]) -> String {
    let value = bytes.iter().fold(0xcbf29ce484222325_u64, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
    });
    format!("{value:016x}")
}
fn identity(font: &PreparedFont) -> Value {
    let id = font.identity();
    json!({"private_alias":font.private_alias(), "source_sha256":hex(&id.source_sha256),
        "registration_sha256":hex(&id.registration_sha256), "face_index":id.face_index,
        "weight":id.style.weight,"italic":id.style.italic,"glyph_count":font.glyph_count(),
        "units_per_em":font.units_per_em(),"registration_bytes":font.bytes().len()})
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
        let root = std::env::var_os("FLOW_CANVAS_STROKE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("gpui-flow-canvas-stroke"));
        std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        let directory = root.join(format!(
            "{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_nanos()
        ));
        std::fs::create_dir(&directory).map_err(|e| e.to_string())?;
        Ok(Self {
            directory,
            sequence: 0,
        })
    }
    fn begin(&mut self, action: &str) -> Result<PathBuf, String> {
        self.sequence += 1;
        let folder = self
            .directory
            .join(format!("{:03}-{action}", self.sequence));
        std::fs::create_dir(&folder).map_err(|e| e.to_string())?;
        Ok(folder)
    }
    fn finish(
        &self,
        folder: &Path,
        action: &str,
        origin: &str,
        evidence: Value,
    ) -> Result<(), String> {
        let receipt = json!({"fixture":"canvas-stroke-text", "schema":1,
            "sequence":self.sequence,"action":action,"origin":origin,"evidence":evidence,
            "directory":folder.to_string_lossy(),
            "timing_scope":"physical entry/batch checks only; no mid-raster or post-upload timing injection"});
        write_new(
            &folder.join("receipt.json"),
            &serde_json::to_vec_pretty(&receipt).map_err(|e| e.to_string())?,
        )?;
        println!("{receipt}");
        Ok(())
    }
}

fn text(font: &PreparedFont, content: &str, size: f32, width: f32, height: f32) -> display::Text {
    let mut t = display::Text::new(content, SceneSize::new(width, height));
    t.font_family = font.private_alias().into();
    t.font_size = size;
    t.line_height = 1.12;
    t.wrap = false;
    t
}
fn ink(face: usize) -> Color {
    Color::hex([0xffd778, 0x71dcf5][face])
}
fn stroke(t: display::Text, x: f32, y: f32, color: Color, width: f32) -> CanvasCommand {
    CanvasCommand::StrokeTextBox {
        text: t,
        position: point(x, y),
        color,
        options: CanvasStrokeOptions {
            width,
            ..Default::default()
        },
    }
}
fn frame(x: f32, y: f32, width: f32, height: f32) -> CanvasCommand {
    CanvasCommand::StrokePath {
        path: path(&format!("M {x} {y} h {width} v {height} h {} Z", -width)),
        stroke: Stroke::new(Color::hex(0x53617c), 0.65),
    }
}
// A recognizable persistent curved band. Text commands must not consume it.
fn sentinel() -> Vec<CanvasCommand> {
    vec![
        CanvasCommand::BeginPath,
        CanvasCommand::MoveTo {
            to: point(18., 322.),
        },
        CanvasCommand::QuadraticCurveTo {
            control: point(220., 296.),
            to: point(422., 322.),
        },
        CanvasCommand::LineTo {
            to: point(422., 331.),
        },
        CanvasCommand::LineTo {
            to: point(18., 331.),
        },
        CanvasCommand::ClosePath,
    ]
}
fn replay(surface: &CanvasSurface) -> Result<CanvasSurface, String> {
    let mut clone = surface.clone();
    clone.clear().map_err(|e| e.to_string())?;
    clone
        .draw_commands(&[
            CanvasCommand::FillCurrentPath {
                paint: Color::WHITE.into(),
                rule: FillRule::EvenOdd,
            },
            CanvasCommand::StrokeCurrentPath {
                stroke: Stroke::new(Color::hex(0x438eff), 2.),
            },
        ])
        .map_err(|e| e.to_string())?;
    Ok(clone)
}
fn gallery(fonts: &[Arc<PreparedFont>; 2], scenario: usize) -> Vec<CanvasCommand> {
    let mut out = Vec::new();
    match scenario {
        0 => {
            for (face, font) in fonts.iter().enumerate() {
                let y = 5. + face as f32 * 145.;
                out.push(stroke(
                    text(font, "m O B é", 67., 416., 85.),
                    12.,
                    y,
                    ink(face),
                    2.0,
                ));
                out.push(stroke(
                    text(font, "fi AV m\u{301} e\u{301}", 42., 416., 60.),
                    12.,
                    y + 82.,
                    ink(face),
                    1.25,
                ));
            }
        }
        1 => {
            for (face, font) in fonts.iter().enumerate() {
                let y = 8. + face as f32 * 150.;
                let mut wrapped = text(font, "m c O B m c O B\nAV fi é", 28., 205., 140.);
                wrapped.wrap = true;
                wrapped.padding = [7., 9., 7., 9.];
                wrapped.align = display::TextAlign::Center;
                wrapped.vertical_align = display::TextVerticalAlign::Middle;
                wrapped.para_spacing = 10.;
                out.push(frame(8., y, 205., 140.));
                out.push(stroke(wrapped, 8., y, ink(face), 1.1));
                let mut upper = text(font, "m c o b av fi m c o b", 28., 200., 65.);
                upper.text_case = display::TextCase::Upper;
                upper.overflow = display::TextOverflow::Ellipsis;
                upper.padding = [5., 8., 5., 8.];
                upper.align = display::TextAlign::End;
                upper.vertical_align = display::TextVerticalAlign::Bottom;
                out.push(frame(225., y, 200., 65.));
                out.push(stroke(upper, 225., y, ink(face), 1.1));
                let mut lower = text(font, "M C O B AV FI M C O B", 28., 200., 65.);
                lower.text_case = display::TextCase::Lower;
                lower.overflow = display::TextOverflow::Ellipsis;
                lower.padding = [5., 8., 5., 8.];
                out.push(frame(225., y + 75., 200., 65.));
                out.push(stroke(lower, 225., y + 75., ink(face), 1.1));
            }
        }
        2 => {
            for row in 0..3 {
                for (face, font) in fonts.iter().enumerate() {
                    out.push(CanvasCommand::StrokeTextBox {
                        text: text(font, "O AV", 43., 202., 60.),
                        position: point(8. + face as f32 * 218., 4. + row as f32 * 62.),
                        color: ink(face),
                        options: CanvasStrokeOptions {
                            width: 3.5,
                            cap: [StrokeCap::Butt, StrokeCap::Round, StrokeCap::Square][row],
                            join: [StrokeJoin::Miter, StrokeJoin::Round, StrokeJoin::Bevel][row],
                            miter_limit: 2.,
                            dash_pattern: vec![10., 5., 3.],
                            dash_offset: row as f32 * 3.,
                        },
                    });
                }
            }
            for (face, font) in fonts.iter().enumerate() {
                out.push(CanvasCommand::Save);
                out.push(CanvasCommand::Transform(if face == 0 {
                    Affine::translation(16., 196.)
                        * Affine::rotation(12.)
                        * Affine::skew(-16., 0.)
                        * Affine::scale(1.1, 0.85)
                } else {
                    Affine::translation(426., 214.)
                        * Affine::rotation(-8.)
                        * Affine::skew(12., 0.)
                        * Affine::scale(-1.15, 1.3)
                }));
                out.push(stroke(
                    text(font, "O B é", 42., 180., 75.),
                    0.,
                    0.,
                    ink(face),
                    2.,
                ));
                out.push(CanvasCommand::Restore);
            }
        }
        _ => {
            out.extend([
                CanvasCommand::BeginPath,
                CanvasCommand::SetTransform(Affine::translation(12., 8.)),
                CanvasCommand::MoveTo { to: point(0., 0.) },
                CanvasCommand::LineTo {
                    to: point(414., 0.),
                },
                CanvasCommand::LineTo {
                    to: point(414., 192.),
                },
                CanvasCommand::LineTo {
                    to: point(0., 192.),
                },
                CanvasCommand::ClosePath,
                CanvasCommand::MoveTo {
                    to: point(152., 38.),
                },
                CanvasCommand::LineTo {
                    to: point(259., 38.),
                },
                CanvasCommand::LineTo {
                    to: point(259., 145.),
                },
                CanvasCommand::LineTo {
                    to: point(152., 145.),
                },
                CanvasCommand::ClosePath,
                CanvasCommand::SetTransform(Affine::IDENTITY),
                CanvasCommand::Save,
                CanvasCommand::ClipCurrentPath {
                    rule: FillRule::EvenOdd,
                },
                CanvasCommand::SetOpacity(0.66),
                CanvasCommand::Save,
                CanvasCommand::Transform(Affine::translation(5., 0.)),
                CanvasCommand::ClipPath {
                    path: path("M 0 0 L 265 0 Q 208 104 282 207 L 0 207 Z"),
                    rule: FillRule::NonZero,
                },
                CanvasCommand::SetTransform(Affine::IDENTITY),
            ]);
            for (face, font) in fonts.iter().enumerate() {
                out.push(stroke(
                    text(font, "O B m O B m", 61., 430., 92.),
                    2.,
                    9. + face as f32 * 94.,
                    ink(face),
                    4.,
                ));
            }
            out.push(CanvasCommand::Restore);
            out.push(stroke(
                text(&fonts[1], "O B", 45., 150., 80.),
                294.,
                94.,
                ink(1),
                2.,
            ));
            out.push(CanvasCommand::Restore);
            // Wide single-glyph outlines overlap within one StrokeTextBox. Below,
            // two independent source-over draws overlap too. Neither is group alpha.
            out.push(CanvasCommand::SetOpacity(0.5));
            for (face, font) in fonts.iter().enumerate() {
                out.push(stroke(
                    text(font, "O B", 64., 194., 99.),
                    15. + face as f32 * 211.,
                    208.,
                    ink(face),
                    15.,
                ));
                out.push(stroke(
                    text(font, "O", 64., 85., 99.),
                    57. + face as f32 * 211.,
                    208.,
                    Color::rgba(226, 125, 210, 128),
                    5.,
                ));
            }
            out.push(CanvasCommand::SetOpacity(1.));
        }
    }
    // Reestablish recognizable persistent geometry in every settled gallery.
    out.extend(sentinel());
    out.push(CanvasCommand::StrokeCurrentPath {
        stroke: Stroke::new(Color::hex(0x75889c), 0.8),
    });
    out
}
fn document() -> Scene {
    let mut scene = Scene::new();
    for (id, x, dpr) in [("one", 30., 1.), ("two", 520., 2.)] {
        let mut config = display::Canvas::new(WIDTH, HEIGHT);
        config.pixel_ratio = dpr;
        config.safe_resize = true;
        scene
            .insert(
                DisplayObject::new(id, config)
                    .with_transform(Affine::translation(x, 46.))
                    .with_style(Style {
                        fill: Some(Color::hex(0x17283b).into()),
                        stroke: Some(Stroke::new(Color::hex(0x496b87), 1.)),
                        ..Default::default()
                    }),
                None,
                None,
            )
            .unwrap();
        let mut label = display::Text::new(
            format!(
                "{} resource · initial DPR {dpr}",
                if id == "one" { "Left" } else { "Right" }
            ),
            SceneSize::new(WIDTH, 30.),
        );
        label.font_size = 17.;
        scene
            .insert(
                DisplayObject::new(format!("label-{id}"), label)
                    .with_transform(Affine::translation(x, 8.))
                    .with_style(Style::fill(Color::WHITE)),
                None,
                None,
            )
            .unwrap();
    }
    scene
}

#[derive(Clone)]
struct Capture {
    surface: CanvasSurface,
    resource: Arc<StaticImage>,
}
fn capture(view: &mut SceneView, id: &str, cx: &mut Context<SceneView>) -> Result<Capture, String> {
    let handle = view
        .editor
        .engine
        .handle(&id.into())
        .ok_or("missing Canvas handle")?;
    let mut surface = None;
    // A path/pixel-neutral edit ensures pending resize resources are settled.
    view.edit_canvas(
        handle,
        |s| {
            surface = Some(s.clone());
            Ok(())
        },
        cx,
    )
    .map_err(|e| e.to_string())?;
    let surface = surface.ok_or("no surface observation")?;
    let resource = view
        .resources()
        .and_then(|r| r.canvas_snapshot(&view.editor.engine, handle))
        .ok_or("no committed resource")?;
    if !Arc::ptr_eq(&resource, &surface.snapshot()) {
        return Err("candidate differs from committed resource".into());
    }
    Ok(Capture { surface, resource })
}
fn diagnostics(image: &StaticImage) -> Value {
    let mut zero = 0_u64;
    let mut partial = 0_u64;
    let mut opaque = 0_u64;
    let mut invalid = 0_u64;
    let mut histogram = [0_u64; 16];
    let mut bounds = [image.width(), image.height(), 0, 0];
    for (i, p) in image
        .premultiplied_rgba()
        .as_chunks::<4>()
        .0
        .iter()
        .enumerate()
    {
        match p[3] {
            0 => zero += 1,
            255 => opaque += 1,
            _ => partial += 1,
        };
        histogram[usize::from(p[3]) / 16] += 1;
        invalid += u64::from(p[..3].iter().any(|v| *v > p[3]));
        if p[3] > 0 {
            let x = i as u32 % image.width();
            let y = i as u32 / image.width();
            bounds = [
                bounds[0].min(x),
                bounds[1].min(y),
                bounds[2].max(x + 1),
                bounds[3].max(y + 1),
            ];
        }
    }
    json!({"transparent_pixels":zero,"partial_alpha_pixels":partial,"opaque_pixels":opaque,
        "nonzero_alpha_pixels":partial+opaque,"invalid_premultiplied_pixels":invalid,
        "alpha_histogram_16_bins":histogram,"ink_bounds_physical_exclusive":if partial+opaque>0{Some(bounds)}else{None}})
}
fn export_image(folder: &Path, stem: &str, image: &StaticImage) -> Result<Value, String> {
    let raw = format!("{stem}.premul-rgba");
    let png = format!("{stem}.png");
    let encoded = image
        .encode_png(Default::default())
        .map_err(|e| e.to_string())?;
    write_new(&folder.join(&raw), image.premultiplied_rgba())?;
    write_new(&folder.join(&png), &encoded)?;
    Ok(
        json!({"raw":raw,"png":png,"width":image.width(),"height":image.height(),
        "raw_bytes":image.byte_len(),"raw_fnv1a64":checksum(image.premultiplied_rgba()),
        "png_fnv1a64":checksum(&encoded),"diagnostics":diagnostics(image),
        "raw_encoding":"row-major premultiplied RGBA8; exact authoritative bytes",
        "png_encoding":"straight RGBA8; existing StaticImage encoder demultiplies each row"}),
    )
}
fn export_capture(
    folder: &Path,
    stem: &str,
    capture: &Capture,
    with_replay: bool,
) -> Result<Value, String> {
    let image = export_image(folder, stem, &capture.resource)?;
    let path_replay = if with_replay {
        Some(export_image(
            folder,
            &format!("{stem}-path-replay"),
            &replay(&capture.surface)?.snapshot(),
        )?)
    } else {
        None
    };
    Ok(
        json!({"image":image,"config":capture.surface.config(),"dpr":capture.surface.config().pixel_ratio,
        "generation":capture.surface.generation(),"current_path_bytes":capture.surface.current_path_byte_len(),
        "resource_and_surface_snapshot_identical":Arc::ptr_eq(&capture.resource,&capture.surface.snapshot()),
        "path_replay":path_replay}),
    )
}
fn unchanged(before: &Capture, after: &Capture) -> Result<Value, String> {
    let a = replay(&before.surface)?;
    let b = replay(&after.surface)?;
    let identity = Arc::ptr_eq(&before.resource, &after.resource);
    let generation = before.surface.generation() == after.surface.generation();
    let pixels = before.resource.premultiplied_rgba() == after.resource.premultiplied_rgba();
    let path_bytes =
        before.surface.current_path_byte_len() == after.surface.current_path_byte_len();
    let replay_bytes = a.snapshot().premultiplied_rgba() == b.snapshot().premultiplied_rgba();
    Ok(
        json!({"snapshot_identity_unchanged":identity,"generation_unchanged":generation,
        "premultiplied_rgba_unchanged":pixels,"current_path_byte_len_unchanged":path_bytes,
        "current_path_replay_exact":replay_bytes,"all":identity&&generation&&pixels&&path_bytes&&replay_bytes}),
    )
}

fn expected_error(index: usize, error: &CanvasEditError) -> bool {
    match (index, error) {
        (0..=2, CanvasEditError::Pixels(CompositeError::TextRasterization { reason, .. })) => {
            reason.contains("Binding(UnboundFont)")
        }
        (3..=4, CanvasEditError::Pixels(CompositeError::TextRasterization { reason, .. })) => {
            reason.contains("Binding(StyleMismatch)") || reason.contains("Binding(UnboundFont)")
        }
        (5, CanvasEditError::Pixels(CompositeError::UnsupportedTextStyle(_))) => true,
        (6, CanvasEditError::Pixels(CompositeError::Superseded)) => true,
        (7, CanvasEditError::Pixels(CompositeError::InvalidGeometry(reason))) => {
            reason == "Canvas restore without save"
        }
        _ => false,
    }
}

struct Lab {
    view: Entity<SceneView>,
    bridge: Arc<GpuiTextRasterizer>,
    system: Arc<TextSystem>,
    fonts: [Arc<PreparedFont>; 2],
    descriptors: [Font; 2],
    trace: Trace,
    scenario: usize,
    status: String,
    origin: &'static str,
    last_exports: String,
}
impl Lab {
    fn new(
        bridge: Arc<GpuiTextRasterizer>,
        fonts: [Arc<PreparedFont>; 2],
        trace: Trace,
        cx: &mut Context<Self>,
    ) -> Result<Self, String> {
        let system = cx.text_system().clone();
        if !bridge.matches_text_system(&system) {
            return Err("startup App owner mismatch".into());
        }
        let descriptors: [Font; 2] = bridge
            .static_outline_descriptors()
            .map_err(|e| e.to_string())?
            .try_into()
            .map_err(|_| "expected exactly two registered faces")?;
        for (i, font) in fonts.iter().enumerate() {
            write_new(
                &trace.directory.join(format!("{i}-source.font")),
                [TTF, CFF][i],
            )?;
            write_new(
                &trace.directory.join(format!("{i}-registered.font")),
                font.bytes(),
            )?;
        }
        let view = cx.new(|cx| {
            let mut v = SceneView::new(document(), cx).unwrap();
            v.show_grid = false;
            v.set_editable(false, cx);
            v.set_text_rasterizer(Some(bridge.clone()), cx).unwrap();
            v
        });
        let mut lab = Self {
            view,
            bridge,
            system,
            fonts,
            descriptors,
            trace,
            scenario: 0,
            status: String::new(),
            origin: "startup",
            last_exports: String::new(),
        };
        let folder = lab.trace.begin("startup")?;
        lab.trace.finish(&folder,"startup",lab.origin,json!({"factory":"application_with_static_outlines",
            "same_app_owner":true,"factory_direct_to_app_identity":"verified by factory; direct native IDs are not exposed",
            "app_ids":lab.descriptors.iter().map(|f|lab.system.resolve_font(f).0).collect::<Vec<_>>(),
            "fonts":lab.fonts.iter().map(|f|identity(f)).collect::<Vec<_>>(),"source_lengths":[TTF.len(),CFF.len()],
            "native_shaping_diagnostics":lab.shaping_probes()?,"scenario_count":SCENARIOS.len(),
            "scope":"native Canvas resources; PNG/raw pixels exclude scene backdrop/border/labels"}))?;
        lab.run_action("regenerate", cx);
        lab.origin = "physical_button";
        Ok(lab)
    }
    fn probe(&self, face: usize, content: &str) -> Result<Value, String> {
        let lines = WindowTextSystem::new(self.system.clone())
            .shape_text(
                content.to_owned().into(),
                px(42.),
                &[TextRun {
                    len: content.len(),
                    font: self.descriptors[face].clone(),
                    color: rgb(0xffffff).into(),
                    ..Default::default()
                }],
                None,
                None,
            )
            .map_err(|e| e.to_string())?;
        let mut runs = Vec::new();
        let mut bound = 0;
        let mut unbound = 0;
        let mut other = 0;
        let mut missing = 0;
        for line in &lines {
            for run in &line.unwrapped_layout.runs {
                let result = self.bridge.static_outline_font(
                    &self.system,
                    run.font_id,
                    self.fonts[face].identity().style,
                    self.bridge.epoch(),
                    &AtomicBool::new(false),
                );
                let lookup = match result {
                    Ok(f) => {
                        bound += 1;
                        json!({"bound":true,"identity":identity(f)})
                    }
                    Err(e) => {
                        if e == OutlineBindingError::UnboundFont {
                            unbound += 1
                        } else {
                            other += 1
                        };
                        json!({"bound":false,"error":format!("{e:?}")})
                    }
                };
                missing += run.glyphs.iter().filter(|g| g.id.0 == 0).count();
                runs.push(json!({"font_id":run.font_id.0,"lookup":lookup,"glyphs":run.glyphs.iter().map(|g|json!({"id":g.id.0,
                "index":g.index,"x":f32::from(g.position.x),"y":f32::from(g.position.y),"emoji":g.is_emoji})).collect::<Vec<_>>()}));
            }
        }
        Ok(
            json!({"text":content,"face":face,"runs":runs,"bound_runs":bound,"unbound_runs":unbound,"other_errors":other,
            "missing_glyphs":missing,"widths":lines.iter().map(|l|f32::from(l.unwrapped_layout.width)).collect::<Vec<_>>(),
            "scope":"independent same-App diagnostic shaping; actual Canvas commands use production adapter"}),
        )
    }
    fn shaping_probes(&self) -> Result<Value, String> {
        let mut out = Vec::new();
        for face in 0..2 {
            for s in ["fi", "f i", "AV", "A V", "m\u{301}", "m", "é", "O B"] {
                out.push(self.probe(face, s)?);
            }
        }
        Ok(
            json!({"samples":out,"expected_source_glyph_ids":{"fi":19,"acutecomb":20,"eacute":21,"O":10,"B":22},
            "features":"source contains liga, kern and mark; observed glyphs/positions determine native support"}),
        )
    }
    fn capture_both(&self, cx: &mut Context<Self>) -> Result<Vec<Capture>, String> {
        self.view.update(cx, |view, cx| {
            IDS.iter().map(|id| capture(view, id, cx)).collect()
        })
    }
    fn export_both(
        &self,
        folder: &Path,
        stem: &str,
        captures: &[Capture],
        replay: bool,
    ) -> Result<Value, String> {
        captures
            .iter()
            .enumerate()
            .map(|(i, c)| export_capture(folder, &format!("{stem}-{}", IDS[i]), c, replay))
            .collect::<Result<Vec<_>, _>>()
            .map(|v| json!(v))
    }
    fn draw(
        &self,
        commands: &[CanvasCommand],
        cancelled: bool,
        cx: &mut Context<Self>,
    ) -> Vec<Result<(), CanvasEditError>> {
        self.view.update(cx, |view, cx| {
            IDS.iter()
                .map(|id| {
                    let h = view
                        .editor
                        .engine
                        .handle(&(*id).into())
                        .expect("fixture Canvas handle remains attached");
                    view.draw_canvas_commands_with_cancel(
                        h,
                        commands,
                        &ImageInputs::default(),
                        &AtomicBool::new(cancelled),
                        cx,
                    )
                })
                .collect()
        })
    }
    fn regenerate(&self, cx: &mut Context<Self>) -> Result<Value, String> {
        self.view.update(cx, |view, cx| {
            for id in IDS {
                let h = view
                    .editor
                    .engine
                    .handle(&id.into())
                    .ok_or("missing Canvas handle")?;
                view.edit_canvas(h, |s| s.clear(), cx)
                    .map_err(|e| e.to_string())?;
            }
            Ok::<_, String>(())
        })?;
        let commands = gallery(&self.fonts, self.scenario);
        let outcomes = self.draw(&commands, false, cx);
        if let Some(error) = outcomes.into_iter().find_map(Result::err) {
            return Err(error.to_string());
        }
        Ok(
            json!({"scenario":self.scenario,"name":SCENARIOS[self.scenario],"commands_debug":format!("{commands:#?}"),
            "accepted_resources":2,"clear_is_separate_accepted_edit":true}),
        )
    }
    fn rejects(&self, folder: &Path, cx: &mut Context<Self>) -> Result<Value, String> {
        let before = self.capture_both(cx)?;
        let before_exports = self.export_both(folder, "before", &before, true)?;
        let mut checks = Vec::new();
        let mut all = true;
        for (ix, name) in [
            "fallback-first",
            "fallback-middle",
            "fallback-last",
            "wrong-weight",
            "wrong-slant",
            "decoration",
            "cancel-entry",
            "late-restore",
        ]
        .iter()
        .enumerate()
        {
            let face = ix % 2;
            let mut t = text(
                &self.fonts[face],
                match ix {
                    0 => "WQ m O",
                    1 => "m WQ O",
                    2 => "m O WQ",
                    _ => "m O B",
                },
                42.,
                410.,
                100.,
            );
            if ix == 3 {
                t.font_weight = 700;
            }
            if ix == 4 {
                t.italic = true;
            }
            if ix == 5 {
                t.decoration = display::TextDecoration::Underline;
            }
            let probe = if ix < 3 {
                Some(self.probe(face, &t.content)?)
            } else {
                None
            };
            let mixed = probe.as_ref().is_none_or(|p| {
                p["bound_runs"].as_u64().unwrap_or(0) > 0
                    && p["unbound_runs"].as_u64().unwrap_or(0) > 0
                    && p["other_errors"] == 0
                    && p["missing_glyphs"] == 0
            });
            let fallback_position = probe.as_ref().is_none_or(|p| {
                let runs = p["runs"].as_array().expect("probe runs are an array");
                let unbound = |run: &Value| run["lookup"]["error"] == "UnboundFont";
                match ix {
                    0 => runs.first().is_some_and(unbound),
                    1 => runs.iter().enumerate().any(|(at, run)| {
                        unbound(run)
                            && runs[..at].iter().any(|r| r["lookup"]["bound"] == true)
                            && runs[at + 1..].iter().any(|r| r["lookup"]["bound"] == true)
                    }),
                    2 => runs.last().is_some_and(unbound),
                    _ => true,
                }
            });
            let wrong_style = if matches!(ix, 3 | 4) {
                Some(
                    self.bridge
                        .static_outline_font(
                            &self.system,
                            self.system.resolve_font(&self.descriptors[face]),
                            StaticStyle {
                                weight: t.font_weight,
                                italic: t.italic,
                            },
                            self.bridge.epoch(),
                            &AtomicBool::new(false),
                        )
                        .err(),
                )
            } else {
                None
            };
            let style_rejected = wrong_style
                .as_ref()
                .is_none_or(|error| *error == Some(OutlineBindingError::StyleMismatch));
            // Both prospective pixels and current geometry precede the error.
            let mut commands = vec![
                CanvasCommand::BeginPath,
                CanvasCommand::MoveTo { to: point(2., 2.) },
                CanvasCommand::LineTo { to: point(80., 8.) },
                CanvasCommand::LineTo {
                    to: point(40., 50.),
                },
                CanvasCommand::ClosePath,
                CanvasCommand::FillCurrentPath {
                    paint: Color::WHITE.into(),
                    rule: FillRule::NonZero,
                },
                stroke(t, 10., 10., ink(face), 2.),
            ];
            if ix == 7 {
                commands.push(CanvasCommand::Restore);
            }
            let outcomes = self.draw(&commands, ix == 6, cx);
            let after = self.capture_both(cx)?;
            let mut surfaces = Vec::new();
            for i in 0..2 {
                let preserved = unchanged(&before[i], &after[i])?;
                let correct_error = outcomes[i]
                    .as_ref()
                    .err()
                    .is_some_and(|error| expected_error(ix, error));
                let expected = correct_error
                    && preserved["all"] == true
                    && mixed
                    && fallback_position
                    && style_rejected;
                all &= expected;
                surfaces.push(json!({"id":IDS[i],"expected_rejection":correct_error,"error":outcomes[i].as_ref().err().map(ToString::to_string),
                    "rollback":preserved,"passed":expected,"after":export_capture(folder,&format!("{name}-after-{}",IDS[i]),&after[i],true)?}));
            }
            checks.push(json!({"name":name,"face":face,"real_mixed_runs_observed":mixed,
                "fallback_position_observed":fallback_position,"wrong_style_lookup":wrong_style.map(|error|format!("{error:?}")),"diagnostic_shaping":probe,
                "commands_debug":format!("{commands:#?}"),"surfaces":surfaces}));
        }
        let foreign = Arc::new(TextSystem::new(Arc::new(NoopTextSystem::new())));
        let wrong_owner = self
            .bridge
            .static_outline_font(
                &foreign,
                self.system.resolve_font(&self.descriptors[0]),
                StaticStyle {
                    weight: 400,
                    italic: false,
                },
                self.bridge.epoch(),
                &AtomicBool::new(false),
            )
            .err();
        all &= wrong_owner == Some(OutlineBindingError::WrongTextSystem);
        Ok(json!({"passed":all,"before":before_exports,"checks":checks,
            "wrong_owner":{"expected":wrong_owner==Some(OutlineBindingError::WrongTextSystem),"error":format!("{wrong_owner:?}"),
                "scope":"binding lookup only; no second native App and no invalid bridge installed in SceneView"},
            "cancellation":"already cancelled on synchronous entry; no timing injection", "recovery":"press Regenerate after this suite"}))
    }
    fn config(&self, action: &str, cx: &mut Context<Self>) -> Result<Value, String> {
        self.view.update(cx, |view, cx| {
            if action == "undo" {
                let changed = view.editor.undo();
                cx.notify();
                return Ok(json!({"undo_changed":changed}));
            }
            view.editor
                .transaction(|scene| {
                    for id in IDS {
                        scene.update_object(&id.into(), |object| {
                            if let Shape::Canvas(c) = &mut object.shape {
                                c.safe_resize = action != "reset";
                                if action == "dpr" {
                                    c.pixel_ratio = if c.pixel_ratio == 1. { 2. } else { 1. };
                                } else {
                                    c.size.width = if c.size.width > 300. { 280. } else { WIDTH };
                                }
                            }
                        })?;
                    }
                    Ok(())
                })
                .map_err(|e| e.to_string())?;
            cx.notify();
            Ok(json!({"safe_resize":action!="reset","pixels_are_not_undo_history":true}))
        })
    }
    fn action(&mut self, action: &str, cx: &mut Context<Self>) -> Result<(), String> {
        let folder = self.trace.begin(action)?;
        let before = self.capture_both(cx)?;
        let detail = match action {
            "next" => {
                self.scenario = (self.scenario + 1) % SCENARIOS.len();
                self.regenerate(cx)?
            }
            "regenerate" => self.regenerate(cx)?,
            "reject" => self.rejects(&folder, cx)?,
            "safe" | "reset" | "dpr" | "undo" => self.config(action, cx)?,
            "epoch" => {
                let old = self.bridge.epoch();
                self.view
                    .update(cx, |v, cx| v.invalidate_text_resources(cx))
                    .map_err(|e| e.to_string())?;
                let old_rejected = self
                    .bridge
                    .static_outline_font(
                        &self.system,
                        self.system.resolve_font(&self.descriptors[0]),
                        self.fonts[0].identity().style,
                        old,
                        &AtomicBool::new(false),
                    )
                    .err();
                let recovery = self.regenerate(cx)?;
                json!({"old_epoch":old,"new_epoch":self.bridge.epoch(),"old_lookup_error":format!("{old_rejected:?}"),
                    "old_epoch_rejected":old_rejected==Some(OutlineBindingError::StaleSnapshot),"valid_recovery":recovery})
            }
            _ => json!({"explicit_export":true}),
        };
        let after = self.capture_both(cx)?;
        let before_exports = if matches!(action, "safe" | "reset" | "dpr" | "undo") {
            Some(self.export_both(&folder, "config-before", &before, true)?)
        } else {
            None
        };
        let exports = self.export_both(&folder, "settled", &after, false)?;
        let status = if action == "reject" {
            if detail["passed"] == true {
                "PASS: 8 rejected batches/resource; exact snapshots, generations, bytes and path replays retained"
            } else {
                "FAIL: rejection evidence did not meet all checks; inspect receipt"
            }
        } else {
            "Accepted and exported both committed resources"
        };
        self.trace.finish(&folder,action,self.origin,json!({"scenario":self.scenario,"scenario_name":SCENARIOS[self.scenario],
            "detail":detail,"before":before_exports,"settled":exports,"status":status,
            "resource_transactions":"two independent transactions, not a cross-resource atomic batch"}))?;
        self.last_exports = folder.to_string_lossy().into();
        self.status = format!(
            "{status}\nLeft: {}×{} DPR {} gen {} · Right: {}×{} DPR {} gen {}",
            after[0].resource.width(),
            after[0].resource.height(),
            after[0].surface.config().pixel_ratio,
            after[0].surface.generation(),
            after[1].resource.width(),
            after[1].resource.height(),
            after[1].surface.config().pixel_ratio,
            after[1].surface.generation()
        );
        Ok(())
    }
    fn run_action(&mut self, action: &str, cx: &mut Context<Self>) {
        if let Err(error) = self.action(action, cx) {
            self.status = format!("FAILED {action}: {error}");
            let failure = json!({"fixture":"canvas-stroke-text","action":action,"origin":self.origin,
                "failed":error,"sequence":self.trace.sequence,"scenario":self.scenario});
            eprintln!("{failure}");
            let folder = self
                .trace
                .directory
                .join(format!("{:03}-{action}", self.trace.sequence));
            if let Ok(bytes) = serde_json::to_vec_pretty(&failure)
                && let Err(write_error) = write_new(&folder.join("failed.json"), &bytes)
            {
                eprintln!("Could not save failure receipt: {write_error}");
            }
        }
        cx.notify();
    }
}
impl Render for Lab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().flex().flex_col().bg(rgb(0x0d1421)).text_color(rgb(0xe6edf5))
            .child(div().flex_shrink_0().px_4().py_2().text_lg().child(format!("Canvas stroke text · {} / {} · {}",self.scenario+1,SCENARIOS.len(),SCENARIOS[self.scenario])))
            .child(div().h(px(44.)).flex_shrink_0().px_4().flex().items_center().gap_2()
                .child(ControlButton::new("next","Next scenario").on_click(cx.listener(|s,_,_,cx|s.run_action("next",cx))))
                .child(ControlButton::new("regenerate","Regenerate").on_click(cx.listener(|s,_,_,cx|s.run_action("regenerate",cx))))
                .child(ControlButton::new("reject","Reject suite").on_click(cx.listener(|s,_,_,cx|s.run_action("reject",cx))))
                .child(ControlButton::new("epoch","Invalidate + recover").on_click(cx.listener(|s,_,_,cx|s.run_action("epoch",cx))))
                .child(ControlButton::new("export","Export settled").on_click(cx.listener(|s,_,_,cx|s.run_action("export",cx)))))
            .child(div().h(px(44.)).flex_shrink_0().px_4().flex().items_center().gap_2()
                .child(ControlButton::new("safe","Safe shrink/grow").on_click(cx.listener(|s,_,_,cx|s.run_action("safe",cx))))
                .child(ControlButton::new("reset","Reset shrink/grow").on_click(cx.listener(|s,_,_,cx|s.run_action("reset",cx))))
                .child(ControlButton::new("dpr","Swap DPR").on_click(cx.listener(|s,_,_,cx|s.run_action("dpr",cx))))
                .child(ControlButton::new("undo","Undo config").on_click(cx.listener(|s,_,_,cx|s.run_action("undo",cx)))))
            .child(div().flex_1().min_h_0().overflow_hidden().child(self.view.clone()))
            .child(div().flex_shrink_0().px_4().py_2().text_sm().flex().flex_col().gap_1()
                .child(div().child(NOTES[self.scenario]))
                .child(div().child("Gold = quadratic TTF; cyan = cubic CFF. PNGs preserve alpha; dark panel is scene backdrop only."))
                .child(div().child("Resize/DPR → Undo config → Regenerate. Reject suite → Regenerate. Every action exports both resources."))
                .children(self.status.lines().map(|line|div().child(line.to_owned())))
                .child(div().text_xs().child(format!("Files: {}",self.last_exports))))
    }
}

fn helper_tour() -> Result<Value, String> {
    let fonts = [
        Arc::new(PreparedFont::prepare(TTF).map_err(|e| e.to_string())?),
        Arc::new(PreparedFont::prepare(CFF).map_err(|e| e.to_string())?),
    ];
    let mut faces = Vec::new();
    for (i, f) in fonts.iter().enumerate() {
        let mut quadratics = 0;
        let mut cubics = 0;
        let mut closes = 0;
        for glyph in [3, 10, 21, 22] {
            let o = f
                .outline_glyph(glyph, &mut OutlineBudget::default())
                .map_err(|e| e.to_string())?;
            for c in o.commands {
                match c {
                    OutlineCommand::QuadTo(..) => quadratics += 1,
                    OutlineCommand::CurveTo(..) => cubics += 1,
                    OutlineCommand::Close => closes += 1,
                    _ => (),
                }
            }
        }
        if f.glyph_count() != 24
            || closes < 8
            || (i == 0 && quadratics == 0)
            || (i == 1 && cubics == 0)
        {
            return Err("fixture outline coverage changed".into());
        }
        faces.push(
            json!({"identity":identity(f),"quadratics":quadratics,"cubics":cubics,"closes":closes}),
        );
    }
    let image = StaticImage::from_premultiplied_rgba(
        3,
        1,
        vec![0, 0, 0, 0, 64, 32, 16, 128, 255, 80, 0, 255],
        Default::default(),
    )
    .map_err(|e| e.to_string())?;
    let png = image
        .encode_png(Default::default())
        .map_err(|e| e.to_string())?;
    let decoded = image::load_from_memory_with_format(&png, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?
        .to_rgba8();
    if decoded.as_raw() != &[0, 0, 0, 0, 128, 64, 32, 128, 255, 80, 0, 255] {
        return Err("PNG demultiplication evidence failed".into());
    }
    let mut paths = Vec::new();
    for dpr in [1., 2.] {
        let mut c = display::Canvas::new(WIDTH, HEIGHT);
        c.pixel_ratio = dpr;
        let mut surface = CanvasSurface::new(c, Default::default()).map_err(|e| e.to_string())?;
        surface
            .draw_commands(&sentinel())
            .map_err(|e| e.to_string())?;
        let before = surface.clone();
        let rejected = surface.draw_commands(&[CanvasCommand::BeginPath, CanvasCommand::Restore]);
        if rejected.is_ok()
            || !Arc::ptr_eq(&before.snapshot(), &surface.snapshot())
            || replay(&before)?.snapshot().premultiplied_rgba()
                != replay(&surface)?.snapshot().premultiplied_rgba()
        {
            return Err("helper path rollback failed".into());
        }
        paths.push(json!({"dpr":dpr,"path_bytes":surface.current_path_byte_len(),"rejected":true}));
    }
    for scenario in 0..SCENARIOS.len() {
        let commands = gallery(&fonts, scenario);
        if commands.len() > gpui_flow::composite::MAX_CANVAS_COMMANDS {
            return Err("gallery command limit".into());
        }
        for command in commands {
            if let CanvasCommand::StrokeTextBox { text, .. } = command {
                text.validate()?;
            }
        }
    }
    Ok(
        json!({"status":"passed","native_constructed":false,"faces":faces,"path_rollback":paths,
        "png_demultiplication_verified":true,"scenarios":SCENARIOS,"scope":"headless source outlines, helpers, PNG and command validation; no native shaping, Canvas stroke or upload"}),
    )
}
fn run() -> Result<(), String> {
    if std::env::args().any(|s| s == "--helper-tour") {
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
    let trace = Trace::new()?;
    app.with_assets(gpui_kit::assets::Assets).run(move |cx| {
        gpui_kit::init(cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1100.), px(800.)),
                    cx,
                ))),
                ..Default::default()
            },
            move |window, cx| {
                window.set_window_title("gpui-flow · Canvas stroke text");
                cx.new(|cx| {
                    Lab::new(bridge, fonts, trace, cx).expect("initialize Canvas stroke fixture")
                })
            },
        )
        .expect("open Canvas stroke fixture");
        cx.activate(true);
    });
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{}", json!({"fixture":"canvas-stroke-text","failed":error}));
        std::process::exit(1);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[::core::prelude::v1::test]
    fn helper_tour_uses_real_sources_and_lossless_pixel_encoder() {
        assert_eq!(helper_tour().unwrap()["native_constructed"], false);
    }
    #[::core::prelude::v1::test]
    fn receipt_checksum_uses_named_fnv1a_vectors() {
        assert_eq!(checksum(b""), "cbf29ce484222325");
        assert_eq!(checksum(b"a"), "af63dc4c8601ec8c");
    }
}
