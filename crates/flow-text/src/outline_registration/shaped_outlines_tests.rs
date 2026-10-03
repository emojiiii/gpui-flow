//! Headless protocol/placement tests. This model is not native identity evidence.
use super::super::*;
use crate::gpui_outlines::{OutlinedText, PositionedGlyphOutline, TextOutlineError};
use crate::static_outlines::{OutlineBudget, OutlineCommand, OutlineError};
use crate::{
    TextCase, TextDecoration, TextMeasureRequest, TextOverflow, TextRasterRequest, VerticalAlign,
};
use gpui::{FontRun, GlyphId, Pixels, PlatformTextSystem, RenderGlyphParams, point, px, size};
use std::{
    borrow::Cow,
    sync::{
        Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
};

struct Model {
    fonts: Vec<Arc<PreparedFont>>,
    raster_calls: Mutex<Vec<RenderGlyphParams>>,
    cancel_on_shape: Option<Arc<AtomicBool>>,
    invalidate_on_shape: Mutex<Option<Weak<GpuiTextRasterizer>>>,
    layout_scale: f32,
    panic_on_shape: AtomicBool,
}
impl PlatformTextSystem for Model {
    fn add_fonts(&self, _: Vec<Cow<'static, [u8]>>) -> gpui::Result<()> {
        unreachable!()
    }
    fn all_font_names(&self) -> Vec<String> {
        self.fonts
            .iter()
            .map(|f| f.private_alias().into())
            .collect()
    }
    fn font_id(&self, f: &Font) -> gpui::Result<FontId> {
        Ok(FontId(
            self.fonts
                .iter()
                .position(|font| font.private_alias() == f.family.as_ref())
                .unwrap_or(0)
                + 7,
        ))
    }
    fn font_metrics(&self, id: FontId) -> gpui::FontMetrics {
        gpui::NoopTextSystem.font_metrics(id)
    }
    fn typographic_bounds(&self, f: FontId, g: GlyphId) -> gpui::Result<gpui::Bounds<f32>> {
        gpui::NoopTextSystem.typographic_bounds(f, g)
    }
    fn advance(&self, _: FontId, _: GlyphId) -> gpui::Result<gpui::Size<f32>> {
        Ok(size(600., 0.))
    }
    fn glyph_for_char(&self, id: FontId, c: char) -> Option<GlyphId> {
        ttf_parser::Face::parse(self.fonts[id.0.saturating_sub(7)].bytes(), 0)
            .ok()?
            .glyph_index(c)
            .map(|g| GlyphId(u32::from(g.0)))
    }
    fn glyph_raster_bounds(
        &self,
        p: &RenderGlyphParams,
    ) -> gpui::Result<gpui::Bounds<gpui::DevicePixels>> {
        self.raster_calls.lock().unwrap().push(p.clone());
        Ok(gpui::Bounds::new(
            point(gpui::DevicePixels(0), gpui::DevicePixels(0)),
            size(gpui::DevicePixels(1), gpui::DevicePixels(1)),
        ))
    }
    fn rasterize_glyph(
        &self,
        _: &RenderGlyphParams,
        b: gpui::Bounds<gpui::DevicePixels>,
    ) -> gpui::Result<(gpui::Size<gpui::DevicePixels>, Vec<u8>)> {
        Ok((b.size, vec![255]))
    }
    fn layout_line(&self, text: &str, font_size: Pixels, runs: &[FontRun]) -> gpui::LineLayout {
        assert!(
            !self.panic_on_shape.load(Ordering::Relaxed),
            "modeled shape panic"
        );
        if let Some(cancel) = &self.cancel_on_shape {
            cancel.store(true, Ordering::Relaxed);
        }
        if let Some(bridge) = self
            .invalidate_on_shape
            .lock()
            .unwrap()
            .as_ref()
            .and_then(Weak::upgrade)
        {
            bridge.invalidate();
        }
        let font_size = font_size * self.layout_scale;
        let primary = runs.first().map_or(FontId(7), |run| run.font_id);
        let mut result = gpui::LineLayout {
            font_size,
            ascent: font_size * 0.75,
            descent: font_size * 0.25,
            len: text.len(),
            ..Default::default()
        };
        let mut x = 0.;
        for (index, c) in text.char_indices() {
            let font_id = match c {
                '#' => FontId(999),
                '@' => FontId(8),
                _ => primary,
            };
            let id = match c {
                '@' => self.glyph_for_char(font_id, 'O').unwrap(),
                '!' => GlyphId(0),
                '?' => GlyphId(99_999),
                '#' | '$' => self.glyph_for_char(primary, 'm').unwrap(),
                _ => self.glyph_for_char(primary, c).unwrap_or(GlyphId(0)),
            };
            if result.runs.last().is_none_or(|run| run.font_id != font_id) {
                result.runs.push(gpui::ShapedRun {
                    font_id,
                    glyphs: vec![],
                });
            }
            // A real native glyph offset is deliberately nonzero/fractional.
            let offset = if c == '\u{301}' { -2.25 } else { 0. };
            result
                .runs
                .last_mut()
                .unwrap()
                .glyphs
                .push(gpui::ShapedGlyph {
                    id,
                    position: point(px(x), px(offset)),
                    index,
                    is_emoji: c == '$',
                });
            if c != '\u{301}' {
                x += f32::from(font_size) * 0.6;
            }
        }
        result.width = px(x);
        result
    }
    fn recommended_rendering_mode(&self, _: FontId, _: Pixels) -> gpui::TextRenderingMode {
        gpui::TextRenderingMode::Grayscale
    }
}
struct Fixture {
    bridge: Arc<GpuiTextRasterizer>,
    system: Arc<TextSystem>,
    model: Arc<Model>,
    cancel: Arc<AtomicBool>,
}
impl Fixture {
    fn new() -> Self {
        Self::with_options(false, 1.)
    }
    fn with_options(cancel_on_shape: bool, layout_scale: f32) -> Self {
        let fonts = [
            include_bytes!("../../tests/fixtures/stroke-quadratic.ttf").as_slice(),
            include_bytes!("../../tests/fixtures/stroke-cubic.otf").as_slice(),
        ]
        .into_iter()
        .map(|bytes| Arc::new(PreparedFont::prepare(bytes).unwrap()))
        .collect::<Vec<_>>();
        let cancel = Arc::new(AtomicBool::new(false));
        let model = Arc::new(Model {
            fonts: fonts.clone(),
            raster_calls: Mutex::new(vec![]),
            cancel_on_shape: cancel_on_shape.then(|| cancel.clone()),
            invalidate_on_shape: Mutex::new(None),
            layout_scale,
            panic_on_shape: AtomicBool::new(false),
        });
        let system = Arc::new(TextSystem::new(model.clone()));
        let bindings = OutlineBindings {
            owner: system.clone(),
            fonts: fonts
                .into_iter()
                .enumerate()
                .map(|(i, font)| Binding {
                    id: FontId(i + 7),
                    descriptor: gpui::font(font.private_alias().to_owned()),
                    font,
                    registered_epoch: 0,
                })
                .collect(),
        };
        let bridge = Arc::new(GpuiTextRasterizer::from_outline_registration(
            system.clone(),
            model.clone(),
            bindings,
        ));
        Self {
            bridge,
            system,
            model,
            cancel,
        }
    }
    fn request(&self, text: &str, face: usize) -> TextRasterRequest {
        TextRasterRequest {
            text: TextMeasureRequest {
                content: text.into(),
                font_family: self.model.fonts[face].private_alias().into(),
                font_size: 20.,
                font_weight: 400,
                italic: false,
                line_height: 24.,
                text_case: TextCase::None,
                para_spacing: 0.,
                wrap_width: None,
            },
            align: gpui::TextAlign::Left,
            vertical_align: VerticalAlign::Top,
            overflow: TextOverflow::Clip,
            decoration: TextDecoration::None,
            bounds: [-3.125, -4.25, 140., 90.],
            clip: [-100., -100., 300., 300.],
            device_scale: 1.,
            color: [255, 0, 0, 255],
        }
    }
    fn outline(
        &self,
        r: &TextRasterRequest,
        b: &mut OutlineBudget,
    ) -> Result<OutlinedText, TextOutlineError> {
        self.bridge
            .outline_text(&self.system, r, self.bridge.epoch(), &self.cancel, b)
    }
}
fn close_count(g: &PositionedGlyphOutline) -> usize {
    g.commands
        .iter()
        .filter(|c| matches!(c, OutlineCommand::Close))
        .count()
}

#[test]
fn real_quadratic_cubic_composite_counter_and_period_commands_keep_unsnapped_coordinates() {
    for face in 0..2 {
        let f = Fixture::new();
        let r = f.request("OmB.é\u{301} ", face);
        let out = f.outline(&r, &mut OutlineBudget::default()).unwrap();
        assert_eq!(out.glyphs.len(), 7);
        assert_eq!(out.rows, 1);
        assert_eq!(out.glyphs[0].baseline_origin, [-3.125, 12.75]);
        assert_eq!(close_count(&out.glyphs[0]), 2);
        assert_eq!(close_count(&out.glyphs[2]), 3);
        assert_eq!(close_count(&out.glyphs[3]), 1);
        assert!(close_count(&out.glyphs[4]) >= 2);
        assert!(out.glyphs[6].commands.is_empty());
        assert_eq!(out.glyphs[5].baseline_origin[1], 10.5);
        assert!(out.glyphs[0].commands.iter().any(|c| if face == 0 {
            matches!(c, OutlineCommand::QuadTo(..))
        } else {
            matches!(c, OutlineCommand::CurveTo(..))
        }));
        let font = &f.model.fonts[face];
        let original = font
            .outline_glyph(out.glyphs[0].glyph_id.0, &mut OutlineBudget::default())
            .unwrap();
        for (source, target) in original.commands.iter().zip(&out.glyphs[0].commands) {
            if let (OutlineCommand::MoveTo(a), OutlineCommand::MoveTo(b)) = (source, target) {
                assert_eq!(*b, [-3.125 + a[0] * 0.02, 12.75 - a[1] * 0.02]);
            }
        }
        let mut dpr = r.clone();
        dpr.device_scale = 2.;
        let at2 = f.outline(&dpr, &mut OutlineBudget::default()).unwrap();
        assert_eq!(out.glyphs[0].commands, at2.glyphs[0].commands);
    }
}
#[test]
fn actual_layout_font_size_controls_outline_scale() {
    let f = Fixture::with_options(false, 1.25);
    let out = f
        .outline(&f.request("m", 0), &mut OutlineBudget::default())
        .unwrap();
    let source = f.model.fonts[0]
        .outline_glyph(out.glyphs[0].glyph_id.0, &mut OutlineBudget::default())
        .unwrap();
    match (source.commands[0], out.glyphs[0].commands[0]) {
        (OutlineCommand::MoveTo(a), OutlineCommand::MoveTo(b)) => assert_eq!(
            b,
            [
                out.glyphs[0].baseline_origin[0] + a[0] * 0.025,
                out.glyphs[0].baseline_origin[1] - a[1] * 0.025
            ]
        ),
        _ => panic!("expected real contour"),
    }
}
#[test]
fn shared_placement_preserves_wrapping_paragraphs_alignment_padding_and_fill_quantization() {
    let f = Fixture::new();
    let mut r = f.request("mm mm\nO", 0);
    r.text.wrap_width = Some(35.);
    r.text.para_spacing = 7.;
    r.bounds = [10.125, 20.25, 70., 120.];
    for align in [
        gpui::TextAlign::Left,
        gpui::TextAlign::Center,
        gpui::TextAlign::Right,
    ] {
        for vertical in [
            VerticalAlign::Top,
            VerticalAlign::Middle,
            VerticalAlign::Bottom,
        ] {
            r.align = align;
            r.vertical_align = vertical;
            let shaped = crate::gpui_overflow::shape_text_in_box(
                &gpui::WindowTextSystem::new(f.system.clone()),
                &r.text,
                gpui::Hsla::default(),
                r.overflow,
                r.bounds[2],
                r.bounds[3],
            )
            .unwrap();
            let mut placements = vec![];
            let stats = crate::gpui_placement::visit_glyphs::<String>(&shaped, &r, |p| {
                placements.push(p.origin);
                Ok(())
            })
            .unwrap();
            let out = f.outline(&r, &mut OutlineBudget::default()).unwrap();
            assert_eq!(
                out.glyphs
                    .iter()
                    .map(|g| g.baseline_origin)
                    .collect::<Vec<_>>(),
                placements
            );
            assert!(stats.rows >= 3);
            assert_eq!(out.rows, stats.rows);
            let last = placements.last().unwrap();
            let first = placements.first().unwrap();
            assert_eq!(last[1] - first[1], (stats.rows - 1) as f32 * 24. + 7.);
            let fill = f
                .bridge
                .rasterize(&r, 0, &f.cancel, 1_000_000, &mut 1_000_000)
                .unwrap()
                .unwrap();
            assert_eq!(fill.glyphs, out.glyphs.len());
            assert_eq!(fill.rows, out.rows);
            // Fill snaps through its existing native consumer; vector origins retain fractions.
            assert!(
                out.glyphs
                    .iter()
                    .all(|g| g.baseline_origin[1].fract() != 0.)
            );
        }
    }
    assert!(!f.model.raster_calls.lock().unwrap().is_empty());
}
#[test]
fn ellipsis_and_case_use_existing_helpers_and_validate_discarded_fallback() {
    let f = Fixture::new();
    let mut r = f.request("mmmmmm\nO", 0);
    r.overflow = TextOverflow::Ellipsis;
    r.bounds[2] = 60.;
    r.bounds[3] = 24.;
    r.text.text_case = TextCase::Upper;
    let out = f.outline(&r, &mut OutlineBudget::default()).unwrap();
    let period = f.model.glyph_for_char(FontId(7), '.').unwrap();
    assert_eq!(
        out.glyphs
            .iter()
            .rev()
            .take(3)
            .filter(|g| g.glyph_id == period)
            .count(),
        3
    );
    assert_eq!(
        out.glyphs[0].glyph_id,
        f.model.glyph_for_char(FontId(7), 'M').unwrap()
    );
    r.text.content = "mmmmmm\n#".into();
    let mut budget = OutlineBudget::default();
    let before = budget.clone();
    assert_eq!(
        f.outline(&r, &mut budget).unwrap_err(),
        TextOutlineError::Binding(OutlineBindingError::UnboundFont)
    );
    assert_eq!(budget, before);
}
#[test]
fn every_mixed_run_order_and_fully_clipped_candidate_is_atomic() {
    let f = Fixture::new();
    for text in ["#m", "m#", "m#m", "!m", "m!", "?m", "m?", "$m", "m$"] {
        let mut r = f.request(text, 0);
        r.clip = [800., 800., 1., 1.];
        let mut budget = OutlineBudget::default();
        let before = budget.clone();
        assert!(f.outline(&r, &mut budget).is_err(), "{text}");
        assert_eq!(budget, before);
    }
}
#[test]
fn owner_style_decoration_cancel_epoch_and_panics_never_publish_partial_output() {
    let f = Fixture::new();
    let r = f.request("m", 0);
    let mut budget = OutlineBudget::default();
    let before = budget.clone();
    let wrong = Arc::new(TextSystem::new(f.model.clone()));
    assert_eq!(
        f.bridge
            .outline_text(&wrong, &r, 0, &f.cancel, &mut budget)
            .unwrap_err(),
        TextOutlineError::Binding(OutlineBindingError::WrongTextSystem)
    );
    for (weight, italic) in [(700, false), (400, true)] {
        let mut styled = r.clone();
        styled.text.font_weight = weight;
        styled.text.italic = italic;
        assert_eq!(
            f.outline(&styled, &mut budget).unwrap_err(),
            TextOutlineError::Binding(OutlineBindingError::StyleMismatch)
        );
    }
    for decoration in [
        TextDecoration::Underline,
        TextDecoration::Strikethrough,
        TextDecoration::UnderlineStrikethrough,
    ] {
        let mut decorated = r.clone();
        decorated.decoration = decoration;
        assert_eq!(
            f.outline(&decorated, &mut budget).unwrap_err(),
            TextOutlineError::UnsupportedDecoration
        );
    }
    f.cancel.store(true, Ordering::Relaxed);
    assert!(f.outline(&r, &mut budget).is_err());
    f.cancel.store(false, Ordering::Relaxed);
    f.bridge.invalidate();
    assert!(
        f.bridge
            .outline_text(&f.system, &r, 0, &f.cancel, &mut budget)
            .is_err()
    );
    assert_eq!(budget, before);
    let panics = Fixture::new();
    panics.model.panic_on_shape.store(true, Ordering::Relaxed);
    assert_eq!(
        panics
            .outline(&panics.request("mm", 0), &mut budget)
            .unwrap_err(),
        TextOutlineError::NativePanicked
    );
    assert_eq!(budget, before);
    let during = Fixture::with_options(true, 1.);
    assert!(
        during
            .outline(&during.request("mm", 0), &mut budget)
            .is_err()
    );
    assert_eq!(budget, before);
    let during = Fixture::new();
    *during.model.invalidate_on_shape.lock().unwrap() = Some(Arc::downgrade(&during.bridge));
    assert!(
        during
            .outline(&during.request("mm", 0), &mut budget)
            .is_err()
    );
    assert_eq!(budget, before);
}
#[test]
fn aggregate_budget_charges_repeated_empty_glyphs_output_capacities_and_live_offsets() {
    let f = Fixture::new();
    let r = f.request("m ", 0);
    let mut budget = OutlineBudget::new(4, 262144, 1_000_000, 2_000_000);
    let initial = budget.remaining();
    let a = f.outline(&r, &mut budget).unwrap();
    let after = budget.remaining();
    assert_eq!(initial[0] - after[0], 2);
    assert!(initial[3] - after[3] > 0);
    let capacity = a.glyphs.capacity() * std::mem::size_of::<PositionedGlyphOutline>()
        + a.glyphs
            .iter()
            .map(|g| g.commands.capacity() * std::mem::size_of::<OutlineCommand>())
            .sum::<usize>();
    assert_eq!(a.allocated_bytes, capacity);
    assert_eq!(initial[2] - after[2], capacity);
    assert!(a.peak_bytes >= capacity + 4);
    let b = f.outline(&r, &mut budget).unwrap();
    assert_eq!(budget.remaining()[0], 0);
    let before = budget.clone();
    assert!(f.outline(&r, &mut budget).is_err());
    assert_eq!(budget, before);
    let minimum = a.glyphs.len() * std::mem::size_of::<PositionedGlyphOutline>()
        + a.glyphs
            .iter()
            .map(|g| g.commands.len() * std::mem::size_of::<OutlineCommand>())
            .sum::<usize>();
    drop((a, b));
    for mut low in [
        OutlineBudget::new(8192, 262144, 0, 2_000_000),
        OutlineBudget::new(8192, 262144, 1_000_000, 0),
        OutlineBudget::new(8192, 0, 1_000_000, 2_000_000),
    ] {
        let before = low.clone();
        assert!(f.outline(&r, &mut low).is_err());
        assert_eq!(low, before);
    }
    let mut low = OutlineBudget::new(8192, 262144, minimum, 2_000_000);
    let before = low.clone();
    assert!(f.outline(&r, &mut low).is_err());
    assert_eq!(low, before);
}
#[test]
fn external_conversion_budget_debits_are_atomic_and_keep_lower_work_limit() {
    use crate::gpui_outlines::{charge_outline_budget, release_outline_bytes};
    let mut budget = OutlineBudget::new(10, 10, 80, 50);
    let before = budget.clone();
    assert_eq!(
        charge_outline_budget(&mut budget, 90, 1),
        Err(OutlineError::Limit("job storage"))
    );
    assert_eq!(budget, before);
    assert!(charge_outline_budget(&mut budget, 10, 51).is_err());
    assert_eq!(budget, before);
    charge_outline_budget(&mut budget, 70, 25).unwrap();
    assert_eq!(budget.remaining(), [10, 10, 10, 25]);
    release_outline_bytes(&mut budget, 70);
    assert_eq!(budget.remaining(), [10, 10, 80, 25]);
}

#[test]
fn fill_keeps_empty_paragraph_behavior_while_outlines_bound_rows() {
    let f = Fixture::new();
    let r = f.request(&"\n".repeat(8192), 0);
    let fill = f
        .bridge
        .rasterize(&r, 0, &f.cancel, 1_000_000, &mut 1_000_000)
        .unwrap()
        .unwrap();
    assert_eq!(fill.glyphs, 0);
    assert_eq!(fill.rows, 8193);
    let mut budget = OutlineBudget::default();
    let before = budget.clone();
    assert_eq!(
        f.outline(&r, &mut budget).unwrap_err(),
        TextOutlineError::Outline(OutlineError::Limit("job rows"))
    );
    assert_eq!(budget, before);
}

#[test]
fn every_bound_run_uses_its_own_face_and_glyph_outline() {
    let f = Fixture::new();
    let out = f
        .outline(&f.request("O@O", 0), &mut OutlineBudget::default())
        .unwrap();
    assert_eq!(
        out.glyphs.iter().map(|g| g.font_id).collect::<Vec<_>>(),
        [FontId(7), FontId(8), FontId(7)]
    );
    assert!(
        out.glyphs[0]
            .commands
            .iter()
            .any(|c| matches!(c, OutlineCommand::QuadTo(..)))
    );
    assert!(
        out.glyphs[1]
            .commands
            .iter()
            .any(|c| matches!(c, OutlineCommand::CurveTo(..)))
    );
    assert!(
        out.glyphs[2]
            .commands
            .iter()
            .any(|c| matches!(c, OutlineCommand::QuadTo(..)))
    );
}

#[test]
fn fill_keeps_device_space_position_limit_at_subunit_dpr() {
    let f = Fixture::new();
    let mut r = f.request("mm", 0);
    r.bounds = [999_999., 0., 140., 90.];
    r.clip = r.bounds;
    r.device_scale = 0.5;
    let raster = f
        .bridge
        .rasterize(&r, 0, &f.cancel, 1_000_000, &mut 1_000_000)
        .unwrap()
        .unwrap();
    assert_eq!(raster.glyphs, 2);
    let mut budget = OutlineBudget::default();
    let before = budget.clone();
    assert!(matches!(
        f.outline(&r, &mut budget),
        Err(TextOutlineError::Outline(OutlineError::InvalidOutline))
    ));
    assert_eq!(budget, before);
}

#[test]
fn newline_heavy_offsets_and_simultaneous_ellipsis_offsets_reject_tiny_budgets_atomically() {
    let f = Fixture::new();
    for overflow in [TextOverflow::Clip, TextOverflow::Ellipsis] {
        let mut request = f.request(&"\n".repeat(4096), 0);
        request.overflow = overflow;
        let mut budget = OutlineBudget::new(8192, 262144, 4, 2_000_000);
        let before = budget.clone();
        assert_eq!(
            f.outline(&request, &mut budget).unwrap_err(),
            TextOutlineError::Outline(OutlineError::Limit("line-offset storage"))
        );
        assert_eq!(budget, before);
    }
    // Original offsets fit exactly. The single ellipsized row needs another
    // offset while all three original paragraph offsets are still live.
    let mut request = f.request("mm\nmm\nmm", 0);
    request.overflow = TextOverflow::Ellipsis;
    request.bounds[3] = request.text.line_height;
    let mut budget = OutlineBudget::new(8192, 262144, 12, 2_000_000);
    let before = budget.clone();
    assert_eq!(
        f.outline(&request, &mut budget).unwrap_err(),
        TextOutlineError::Outline(OutlineError::Limit("line-offset storage"))
    );
    assert_eq!(budget, before);
}
