//! Same-App shaped glyph outlines. No native platform is constructed here.
use crate::outline_registration::OutlineBindingError;
use crate::static_outlines::{OutlineBudget, OutlineCommand, OutlineError, StaticStyle};
use crate::{GpuiTextRasterizer, TextDecoration, TextRasterRequest};
use gpui::{FontId, GlyphId, TextSystem, WindowTextSystem, WrappedLine};
use std::{
    fmt,
    mem::size_of,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{Arc, atomic::AtomicBool},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextOutlineError {
    Binding(OutlineBindingError),
    Outline(OutlineError),
    InvalidRequest(String),
    UnsupportedDecoration,
    MissingGlyph,
    EmojiGlyph,
    NativePanicked,
}
impl fmt::Display for TextOutlineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "shaped text outline: {self:?}")
    }
}
impl std::error::Error for TextOutlineError {}
impl From<String> for TextOutlineError {
    fn from(error: String) -> Self {
        Self::InvalidRequest(error)
    }
}
impl From<OutlineError> for TextOutlineError {
    fn from(error: OutlineError) -> Self {
        Self::Outline(error)
    }
}
impl From<OutlineBindingError> for TextOutlineError {
    fn from(error: OutlineBindingError) -> Self {
        Self::Binding(error)
    }
}

/// Actual shaped glyph geometry in isotropic raster-logical coordinates, y down,
/// BEFORE device scale. Commands already include font_size/units_per_em and the
/// unsnapped paragraph/wrap/alignment baseline plus actual GPUI glyph offsets.
/// A consumer maps this space back to Text-local space; its existing compositor
/// applies the scene affine and DPR once. Empty verified glyphs remain present.
#[derive(Debug)]
pub struct PositionedGlyphOutline {
    pub font_id: FontId,
    pub glyph_id: GlyphId,
    pub baseline_origin: [f32; 2],
    pub commands: Vec<OutlineCommand>,
}
#[derive(Debug)]
pub struct OutlinedText {
    pub glyphs: Vec<PositionedGlyphOutline>,
    pub rows: usize,
    /// Retained output Vec capacities, including the outer glyph vector.
    pub allocated_bytes: usize,
    /// Peak additional accounted storage for this request, including live line
    /// offsets and glyph command vectors. Not total process RSS: immutable font
    /// bytes, host shaping caches and shared shaping helper temporaries are separate.
    pub peak_bytes: usize,
}

/// Charge additional caller-owned output/scratch and work to the SAME whole-job
/// budget (e.g. Path lowering while glyph vectors are still live). Atomic on error.
/// Drop temporary storage before returning its byte reservation with
/// `release_outline_bytes`; glyph/command/work charges are never refunded.
pub fn charge_outline_budget(
    budget: &mut OutlineBudget,
    bytes: usize,
    work: usize,
) -> Result<(), OutlineError> {
    let [glyphs, commands, remaining_bytes, remaining_work] = budget.remaining();
    let bytes = remaining_bytes
        .checked_sub(bytes)
        .ok_or(OutlineError::Limit("job storage"))?;
    let work = remaining_work
        .checked_sub(work)
        .ok_or(OutlineError::Limit("job work"))?;
    *budget = OutlineBudget::new(glyphs, commands, bytes, work);
    Ok(())
}
/// Release only a byte reservation previously charged by this job, after the
/// corresponding storage is dropped. Does not restore parsing/conversion work.
pub fn release_outline_bytes(budget: &mut OutlineBudget, bytes: usize) {
    let [glyphs, commands, remaining_bytes, work] = budget.remaining();
    *budget = OutlineBudget::new(
        glyphs,
        commands,
        remaining_bytes.saturating_add(bytes),
        work,
    );
}

impl GpuiTextRasterizer {
    /// Extract all actual shaped glyphs atomically, using this bridge's immutable
    /// startup bindings. Never culls before identity validation, never substitutes
    /// a fallback or missing glyph, and never constructs an independent shaper.
    /// Source, ellipsis probes and final output runs are all checked, even outside
    /// the clip. Only the final output's glyphs/commands are charged as extraction;
    /// every shape's run/glyph validation also consumes whole-job work.
    ///
    /// Caller passes the consuming App's TextSystem, a snapshot epoch and one
    /// mutable OutlineBudget shared by the entire Canvas batch. On any failure,
    /// both output and budget changes are discarded. Decorations explicitly fail.
    /// Hinting/subpixel fill quantization is not applied and pixel identity with
    /// hinted native fill is not promised.
    pub fn outline_text(
        &self,
        system: &Arc<TextSystem>,
        request: &TextRasterRequest,
        epoch: u64,
        cancelled: &AtomicBool,
        budget: &mut OutlineBudget,
    ) -> Result<OutlinedText, TextOutlineError> {
        let mut pending = budget.clone();
        let output = catch_unwind(AssertUnwindSafe(|| {
            self.outline_text_inner(system, request, epoch, cancelled, &mut pending)
        }))
        .map_err(|_| TextOutlineError::NativePanicked)??;
        *budget = pending;
        Ok(output)
    }
    fn outline_text_inner(
        &self,
        system: &Arc<TextSystem>,
        r: &TextRasterRequest,
        epoch: u64,
        cancelled: &AtomicBool,
        budget: &mut OutlineBudget,
    ) -> Result<OutlinedText, TextOutlineError> {
        self.check(epoch, cancelled)
            .map_err(|_| OutlineBindingError::StaleSnapshot)?;
        if !self.matches_text_system(system) {
            return Err(OutlineBindingError::WrongTextSystem.into());
        }
        if !self.has_static_outline_bindings() {
            return Err(OutlineBindingError::Unsupported.into());
        }
        r.text.validate()?;
        if r.decoration != TextDecoration::None {
            return Err(TextOutlineError::UnsupportedDecoration);
        }
        if !r.device_scale.is_finite()
            || r.device_scale <= 0.
            || r.text.font_size * r.device_scale > 512.
            || r.bounds
                .iter()
                .chain(&r.clip)
                .any(|v| !v.is_finite() || v.abs() > 1_000_000.)
            || r.bounds[2..].iter().chain(&r.clip[2..]).any(|v| *v <= 0.)
        {
            return Err(TextOutlineError::InvalidRequest(
                "unsupported outline dimensions".into(),
            ));
        }
        let style = StaticStyle {
            weight: r.text.font_weight,
            italic: r.text.italic,
        };
        // Validate the requested face even when shaping yields no runs/glyphs.
        let mut descriptor = gpui::font(r.text.font_family.clone());
        descriptor.weight = gpui::FontWeight(f32::from(style.weight));
        descriptor.style = if style.italic {
            gpui::FontStyle::Italic
        } else {
            gpui::FontStyle::Normal
        };
        descriptor.fallbacks = Some(gpui::FontFallbacks::default());
        let primary = system.resolve_font(&descriptor);
        self.static_outline_font(system, primary, style, epoch, cancelled)?;
        charge_outline_budget(budget, 0, 1)?;
        let start_bytes = budget.remaining()[2];
        let mut peak_bytes = 0;
        let mut validation_error = None;
        let shaped = crate::gpui_overflow::shape_text_in_box_checked(
            &WindowTextSystem::new(system.clone()),
            &r.text,
            gpui::Hsla::default(),
            r.overflow,
            r.bounds[2],
            r.bounds[3],
            &mut |lines, live_offset_bytes| {
                let result = self
                    .validate_outline_runs(system, lines, style, epoch, cancelled, budget)
                    .and_then(|()| {
                        if live_offset_bytes > budget.remaining()[2] {
                            return Err(OutlineError::Limit("line-offset storage").into());
                        }
                        peak_bytes =
                            peak_bytes.max(start_bytes - budget.remaining()[2] + live_offset_bytes);
                        Ok(())
                    });
                if let Err(error) = result {
                    let message = error.to_string();
                    validation_error = Some(error);
                    Err(message)
                } else {
                    Ok(())
                }
            },
        )
        .map_err(|error| validation_error.unwrap_or_else(|| error.into()))?;
        let stats = crate::gpui_placement::counts(&shaped)?;
        if stats.rows > 8192 {
            return Err(OutlineError::Limit("job rows").into());
        }
        let offset_bytes = shaped.line_offsets.capacity() * size_of::<f32>() + shaped.layout_bytes;
        charge_outline_budget(budget, offset_bytes, 0)?;
        let mut glyphs = Vec::new();
        let outer_estimate = stats
            .glyphs
            .checked_mul(size_of::<PositionedGlyphOutline>())
            .ok_or(OutlineError::Limit("job storage"))?;
        if outer_estimate > budget.remaining()[2] {
            return Err(OutlineError::Limit("job storage").into());
        }
        glyphs
            .try_reserve_exact(stats.glyphs)
            .map_err(|_| OutlineError::Allocation)?;
        charge_outline_budget(
            budget,
            glyphs.capacity() * size_of::<PositionedGlyphOutline>(),
            0,
        )?;
        crate::gpui_placement::visit_glyphs::<TextOutlineError>(&shaped, r, |placed| {
            // Outline coordinates have a logical-space cap. Bitmap fill retains
            // its historical cap after device_scale instead.
            if placed.origin.iter().any(|v| v.abs() > 1_000_000.) {
                return Err(OutlineError::InvalidOutline.into());
            }
            let font = self.static_outline_font(system, placed.font_id, style, epoch, cancelled)?;
            // Validation above covers every run. Repeat these guards at extraction
            // so a future caller cannot weaken missing/emoji handling accidentally.
            if placed.glyph_id.0 == 0 {
                return Err(TextOutlineError::MissingGlyph);
            }
            if placed.is_emoji {
                return Err(TextOutlineError::EmojiGlyph);
            }
            let mut outline = font.outline_glyph(placed.glyph_id.0, budget)?;
            let scale = f32::from(placed.font_size) / f32::from(font.units_per_em());
            charge_outline_budget(budget, 0, outline.commands.len().saturating_mul(8))?;
            // Transform the retained glyph Vec in place: no duplicate command
            // conversion Vec/scratch is live beside it.
            for command in &mut outline.commands {
                self.check(epoch, cancelled)
                    .map_err(|_| OutlineBindingError::StaleSnapshot)?;
                transform_command(command, scale, placed.origin)?;
            }
            glyphs.push(PositionedGlyphOutline {
                font_id: placed.font_id,
                glyph_id: placed.glyph_id,
                baseline_origin: placed.origin,
                commands: outline.commands,
            });
            peak_bytes = peak_bytes.max(start_bytes - budget.remaining()[2]);
            Ok(())
        })?;
        self.check(epoch, cancelled)
            .map_err(|_| OutlineBindingError::StaleSnapshot)?;
        drop(shaped);
        release_outline_bytes(budget, offset_bytes);
        Ok(OutlinedText {
            glyphs,
            rows: stats.rows,
            allocated_bytes: start_bytes - budget.remaining()[2],
            peak_bytes,
        })
    }
    fn validate_outline_runs(
        &self,
        system: &Arc<TextSystem>,
        lines: &[WrappedLine],
        style: StaticStyle,
        epoch: u64,
        cancelled: &AtomicBool,
        budget: &mut OutlineBudget,
    ) -> Result<(), TextOutlineError> {
        self.check(epoch, cancelled)
            .map_err(|_| OutlineBindingError::StaleSnapshot)?;
        for line in lines {
            charge_outline_budget(budget, 0, 1)?;
            for run in &line.unwrapped_layout.runs {
                let font =
                    self.static_outline_font(system, run.font_id, style, epoch, cancelled)?;
                charge_outline_budget(budget, 0, 1)?;
                for glyph in &run.glyphs {
                    self.check(epoch, cancelled)
                        .map_err(|_| OutlineBindingError::StaleSnapshot)?;
                    charge_outline_budget(budget, 0, 1)?;
                    if glyph.id.0 == 0 {
                        return Err(TextOutlineError::MissingGlyph);
                    }
                    if glyph.id.0 >= u32::from(font.glyph_count()) {
                        return Err(OutlineError::GlyphOutOfRange.into());
                    }
                    if glyph.is_emoji {
                        return Err(TextOutlineError::EmojiGlyph);
                    }
                }
            }
        }
        Ok(())
    }
}
fn transform_command(
    command: &mut OutlineCommand,
    scale: f32,
    origin: [f32; 2],
) -> Result<(), OutlineError> {
    let point = |point: &mut [f32; 2]| {
        *point = [origin[0] + point[0] * scale, origin[1] - point[1] * scale];
        if point.iter().any(|v| !v.is_finite() || v.abs() > 1_000_000.) {
            Err(OutlineError::InvalidOutline)
        } else {
            Ok(())
        }
    };
    match command {
        OutlineCommand::MoveTo(p) | OutlineCommand::LineTo(p) => point(p),
        OutlineCommand::QuadTo(a, b) => {
            point(a)?;
            point(b)
        }
        OutlineCommand::CurveTo(a, b, c) => {
            point(a)?;
            point(b)?;
            point(c)
        }
        OutlineCommand::Close => Ok(()),
    }
}
