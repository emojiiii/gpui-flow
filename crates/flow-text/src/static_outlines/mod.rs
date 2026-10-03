//! Bounded, CPU-only outlines for immutable host-authorized static SFNT bytes.
//!
//! This module does **not** register fonts, bind GPUI FontIds, shape text, or
//! implement Canvas strokeText. A private alias/hash is not proof of a native
//! font binding. Consumers must separately prove same-App identity and reject
//! every unbound fallback FontId before using shaped glyph IDs with this API.
mod cff;
mod glyf;
mod sfnt;

use sha2::{Digest, Sha256};
use std::{fmt, ops::Range, sync::Arc};
use ttf_parser::{Face, GlyphId, OutlineBuilder};

/// Maximum input and rewritten font size, independently (16 MiB).
pub const MAX_FONT_BYTES: usize = 16 * 1024 * 1024;
const MAX_GLYPHS: u16 = 16_384;
const MAX_GLYPH_COMMANDS: usize = 16_384;
const MAX_GLYPH_WORK: usize = 65_536;
const MAX_OUTLINE_BYTES: usize = 16 * 1024 * 1024;

/// Deliberate capability limits, distinct from malformed input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unsupported {
    Container,
    Collection,
    Variable,
    AlternateGlyphs,
    SignedFont,
    NameFormat,
    StaticStyle,
    GlyphFormat,
    CompositeTransform,
    CffFormat,
    CffProgram,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutlineError {
    Unsupported(Unsupported),
    Malformed(&'static str),
    Limit(&'static str),
    MissingProbeGlyph,
    GlyphOutOfRange,
    InvalidOutline,
    Allocation,
    Randomness,
}
impl fmt::Display for OutlineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "static font outline: {self:?}")
    }
}
impl std::error::Error for OutlineError {}

/// The actual static face's requested-style identity. No synthesis is allowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StaticStyle {
    pub weight: u16,
    pub italic: bool,
}
/// Exact byte identity, not a name match or a platform FontId binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontIdentity {
    pub source_sha256: [u8; 32],
    pub registration_sha256: [u8; 32],
    pub face_index: u32,
    pub style: StaticStyle,
}
#[derive(Debug)]
enum Source {
    Glyf(Vec<Range<usize>>),
    Cff(Vec<Range<usize>>),
}
/// Prepared bytes are immutable, single-face and privately renamed. The host
/// must have permission to use/modify them; Flow neither fetches nor licenses fonts.
#[derive(Debug)]
pub struct PreparedFont {
    bytes: Arc<[u8]>,
    alias: String,
    identity: FontIdentity,
    source: Source,
    units_per_em: u16,
    glyph_count: u16,
    /// Repeated Face parsing scans the directory and CFF dictionaries.
    face_parse_work: usize,
}
impl PreparedFont {
    pub fn prepare(bytes: &[u8]) -> Result<Self, OutlineError> {
        let mut nonce = [0; 16];
        getrandom::fill(&mut nonce).map_err(|_| OutlineError::Randomness)?;
        Self::prepare_with_nonce(bytes, nonce)
    }
    fn prepare_with_nonce(bytes: &[u8], nonce: [u8; 16]) -> Result<Self, OutlineError> {
        let tables = sfnt::parse(bytes)?;
        for t in &tables {
            let kind = match &t.tag {
                b"fvar" | b"gvar" | b"avar" | b"HVAR" | b"VVAR" | b"MVAR" | b"CFF2" | b"VARC" => {
                    Some(Unsupported::Variable)
                }
                b"COLR" | b"CPAL" | b"SVG " | b"sbix" | b"CBDT" | b"CBLC" | b"EBDT" | b"EBLC"
                | b"EBSC" | b"bdat" | b"bloc" => Some(Unsupported::AlternateGlyphs),
                b"DSIG" => Some(Unsupported::SignedFont),
                _ => None,
            };
            if let Some(kind) = kind {
                return Err(OutlineError::Unsupported(kind));
            }
        }
        for tag in [
            b"head", b"maxp", b"hhea", b"hmtx", b"cmap", b"name", b"OS/2", b"post",
        ] {
            sfnt::table(bytes, &tables, tag)?;
        }
        let face = Face::parse(bytes, 0).map_err(|_| OutlineError::Malformed("face"))?;
        let os2 = sfnt::table(bytes, &tables, b"OS/2")?;
        let style = StaticStyle {
            weight: sfnt::u16_at(os2, 4)?,
            italic: face.is_italic(),
        };
        if face.tables().os2.is_none()
            || face.tables().post.is_none()
            || !(1..=1000).contains(&style.weight)
            || sfnt::u16_at(os2, 6)? != 5
            || face.is_oblique()
        {
            return Err(OutlineError::Unsupported(Unsupported::StaticStyle));
        }
        let count = face.number_of_glyphs();
        if count == 0 || count > MAX_GLYPHS {
            return Err(OutlineError::Limit("glyph count"));
        }
        let head = sfnt::table(bytes, &tables, b"head")?;
        let hhea = sfnt::table(bytes, &tables, b"hhea")?;
        let hmtx = sfnt::table(bytes, &tables, b"hmtx")?;
        let maxp = sfnt::table(bytes, &tables, b"maxp")?;
        let metrics = sfnt::u16_at(hhea, 34)?;
        if sfnt::u32_at(head, 0)? != 0x0001_0000
            || sfnt::u32_at(head, 12)? != 0x5F0F_3CF5
            || sfnt::u16_at(head, 52)? != 0
            || sfnt::u32_at(hhea, 0)? != 0x0001_0000
            || sfnt::u16_at(hhea, 32)? != 0
            || metrics == 0
            || metrics > count
            || hmtx.len() < usize::from(metrics) * 4 + usize::from(count - metrics) * 2
            || face.tables().hmtx.is_none()
            || face.tables().cmap.is_none()
        {
            return Err(OutlineError::Malformed("required header or metrics"));
        }
        let (maxp_version, maxp_len) = if &bytes[..4] == b"OTTO" {
            (0x0000_5000, 6)
        } else {
            (0x0001_0000, 32)
        };
        if sfnt::u32_at(maxp, 0)? != maxp_version || maxp.len() < maxp_len {
            return Err(OutlineError::Malformed("maxp outline format"));
        }
        let probe = face
            .glyph_index('m')
            .filter(|id| id.0 != 0)
            .ok_or(OutlineError::MissingProbeGlyph)?;
        if face.glyph_hor_advance(probe).is_none_or(|v| v == 0) {
            return Err(OutlineError::MissingProbeGlyph);
        }
        let mut alias = String::from("FlowOutline");
        for byte in nonce {
            use fmt::Write;
            write!(&mut alias, "{byte:02x}").unwrap();
        }
        let rewritten = sfnt::rewrite(bytes, &tables, &alias, style.italic)?;
        let rewritten_tables = sfnt::parse(&rewritten)?;
        let face =
            Face::parse(&rewritten, 0).map_err(|_| OutlineError::Malformed("rewritten face"))?;
        let glyf_table = rewritten_tables.iter().find(|t| t.tag == *b"glyf");
        let cff_table = rewritten_tables.iter().find(|t| t.tag == *b"CFF ");
        let source = match (glyf_table, cff_table) {
            (Some(glyf_table), None) if rewritten[..4] == [0, 1, 0, 0] => {
                let head = sfnt::table(&rewritten, &rewritten_tables, b"head")?;
                let loca = sfnt::table(&rewritten, &rewritten_tables, b"loca")?;
                if face.tables().glyf.is_none() {
                    return Err(OutlineError::Malformed("glyf table"));
                }
                Source::Glyf(glyf::ranges(
                    loca,
                    glyf_table.range.clone(),
                    sfnt::u16_at(head, 50)?,
                    count,
                )?)
            }
            (None, Some(t)) if &rewritten[..4] == b"OTTO" => {
                let cff = face
                    .tables()
                    .cff
                    .as_ref()
                    .ok_or(OutlineError::Malformed("CFF table"))?;
                let matrix = cff.matrix();
                let scale = 1. / f32::from(face.units_per_em());
                if matrix.sx != scale
                    || matrix.sy != scale
                    || [matrix.kx, matrix.ky, matrix.tx, matrix.ty]
                        .iter()
                        .any(|v| *v != 0.)
                {
                    return Err(OutlineError::Unsupported(Unsupported::CffFormat));
                }
                Source::Cff(cff::ranges(
                    &rewritten[t.range.clone()],
                    t.range.start,
                    count,
                )?)
            }
            _ => return Err(OutlineError::Unsupported(Unsupported::GlyphFormat)),
        };
        let face_parse_work =
            rewritten_tables.len() + cff_table.map_or(0, |table| table.range.len());
        let units_per_em = face.units_per_em();
        let identity = FontIdentity {
            source_sha256: Sha256::digest(bytes).into(),
            registration_sha256: Sha256::digest(&rewritten).into(),
            face_index: 0,
            style,
        };
        let result = Self {
            bytes: rewritten.into(),
            alias,
            identity,
            source,
            units_per_em,
            glyph_count: count,
            face_parse_work,
        };
        // GPUI's pinned Linux loader requires a usable `m`; prove real outline
        // geometry, not merely a cmap entry, without using this as native proof.
        if result
            .outline_glyph(u32::from(probe.0), &mut OutlineBudget::default())?
            .commands
            .is_empty()
        {
            return Err(OutlineError::MissingProbeGlyph);
        }
        Ok(result)
    }
    pub fn bytes(&self) -> &Arc<[u8]> {
        &self.bytes
    }
    pub fn private_alias(&self) -> &str {
        &self.alias
    }
    pub fn identity(&self) -> &FontIdentity {
        &self.identity
    }
    pub fn units_per_em(&self) -> u16 {
        self.units_per_em
    }
    pub fn glyph_count(&self) -> u16 {
        self.glyph_count
    }
    pub fn require_style(&self, style: StaticStyle) -> Result<(), OutlineError> {
        if style != self.identity.style {
            return Err(OutlineError::Unsupported(Unsupported::StaticStyle));
        }
        Ok(())
    }
    /// Extract a glyph ID from an already-proven same-face shaped run. This API
    /// never maps text to glyphs or substitutes a glyph from another font.
    /// Commands are unhinted design units, y-up. Empty means verified empty
    /// geometry; it makes no assertion about the source character being whitespace.
    /// Both partial commands and budget changes are discarded on failure.
    pub fn outline_glyph(
        &self,
        glyph: u32,
        budget: &mut OutlineBudget,
    ) -> Result<GlyphOutline, OutlineError> {
        if glyph >= u32::from(self.glyph_count) {
            return Err(OutlineError::GlyphOutOfRange);
        }
        if budget.glyphs == 0 {
            return Err(OutlineError::Limit("job glyphs"));
        }
        let mut work = Work {
            left: budget.work.min(MAX_GLYPH_WORK),
            used: 0,
        };
        // Charge every repeated face parse, including empty/repeated glyphs.
        // CFF parsing can scan its Top/Private dictionaries before outlining.
        // The full CFF byte length conservatively covers those scans and also
        // rejects oversized CFF input under the per-glyph hard work ceiling.
        work.charge(self.face_parse_work)?;
        let empty = match &self.source {
            Source::Glyf(ranges) => glyf::preflight(
                &self.bytes,
                ranges,
                glyph as u16,
                &mut work,
                &mut Vec::new(),
            )?,
            Source::Cff(ranges) => {
                cff::preflight(&self.bytes[ranges[glyph as usize].clone()], &mut work)?
            }
        };
        let max_commands = budget
            .commands
            .min(MAX_GLYPH_COMMANDS)
            .min(budget.bytes / std::mem::size_of::<OutlineCommand>());
        let mut builder = Builder {
            commands: Vec::new(),
            max_commands,
            failed: None,
            open: false,
        };
        let face =
            Face::parse(&self.bytes, 0).map_err(|_| OutlineError::Malformed("prepared face"))?;
        let bounds = match &self.source {
            Source::Cff(_) => match face
                .tables()
                .cff
                .as_ref()
                .unwrap()
                .outline(GlyphId(glyph as u16), &mut builder)
            {
                Ok(bounds) => Some(bounds),
                Err(ttf_parser::CFFError::ZeroBBox) if empty && builder.commands.is_empty() => None,
                Err(_) => return Err(OutlineError::InvalidOutline),
            },
            Source::Glyf(_) => face.outline_glyph(GlyphId(glyph as u16), &mut builder),
        };
        if let Some(error) = builder.failed {
            return Err(error);
        }
        if builder.open
            || (bounds.is_none() && (!empty || !builder.commands.is_empty()))
            || (bounds.is_some() && builder.commands.is_empty())
        {
            return Err(OutlineError::InvalidOutline);
        }
        let bytes = builder.commands.capacity() * std::mem::size_of::<OutlineCommand>();
        if bytes > budget.bytes || builder.commands.capacity() > max_commands {
            return Err(OutlineError::Limit("outline allocation capacity"));
        }
        budget.glyphs -= 1;
        budget.commands -= builder.commands.len();
        budget.bytes -= bytes;
        budget.work -= work.used;
        Ok(GlyphOutline {
            commands: builder.commands,
            bounds: bounds.map(|b| {
                [
                    f32::from(b.x_min),
                    f32::from(b.y_min),
                    f32::from(b.x_max),
                    f32::from(b.y_max),
                ]
            }),
        })
    }
}
/// Shared across a complete job, including repeated glyphs and empty glyphs.
/// Lower host limits are clamped to hard ceilings and cannot expand capability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineBudget {
    glyphs: usize,
    commands: usize,
    bytes: usize,
    work: usize,
}
impl Default for OutlineBudget {
    fn default() -> Self {
        Self::new(8192, 262_144, MAX_OUTLINE_BYTES, 2_000_000)
    }
}
impl OutlineBudget {
    pub fn new(glyphs: usize, commands: usize, bytes: usize, work: usize) -> Self {
        Self {
            glyphs: glyphs.min(8192),
            commands: commands.min(262_144),
            bytes: bytes.min(MAX_OUTLINE_BYTES),
            work: work.min(2_000_000),
        }
    }
    pub fn remaining(&self) -> [usize; 4] {
        [self.glyphs, self.commands, self.bytes, self.work]
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OutlineCommand {
    MoveTo([f32; 2]),
    LineTo([f32; 2]),
    QuadTo([f32; 2], [f32; 2]),
    CurveTo([f32; 2], [f32; 2], [f32; 2]),
    Close,
}
#[derive(Debug, PartialEq)]
pub struct GlyphOutline {
    pub commands: Vec<OutlineCommand>,
    /// Parser bounds in unhinted design units, [min_x, min_y, max_x, max_y].
    /// These can include control points; not promised browser ink metrics.
    pub bounds: Option<[f32; 4]>,
}
struct Work {
    left: usize,
    used: usize,
}
impl Work {
    fn charge(&mut self, n: usize) -> Result<(), OutlineError> {
        self.left = self
            .left
            .checked_sub(n)
            .ok_or(OutlineError::Limit("glyph or job work"))?;
        self.used += n;
        Ok(())
    }
}
struct Builder {
    commands: Vec<OutlineCommand>,
    max_commands: usize,
    failed: Option<OutlineError>,
    open: bool,
}
impl Builder {
    fn push(&mut self, command: OutlineCommand, points: &[[f32; 2]]) {
        if self.failed.is_some() {
            return;
        }
        if points
            .iter()
            .flatten()
            .any(|v| !v.is_finite() || v.abs() > 1_000_000.)
        {
            self.failed = Some(OutlineError::InvalidOutline);
            return;
        }
        if self.commands.len() == self.max_commands {
            self.failed = Some(OutlineError::Limit("outline commands or bytes"));
            return;
        }
        if self.commands.len() == self.commands.capacity() {
            let extra = 128.min(self.max_commands - self.commands.len());
            if self.commands.try_reserve_exact(extra).is_err() {
                self.failed = Some(OutlineError::Allocation);
                return;
            }
        }
        if self.commands.capacity() > self.max_commands {
            self.failed = Some(OutlineError::Limit("outline allocation capacity"));
            return;
        }
        self.commands.push(command);
    }
}
impl OutlineBuilder for Builder {
    fn move_to(&mut self, x: f32, y: f32) {
        if self.open {
            self.failed = Some(OutlineError::InvalidOutline);
        }
        self.open = true;
        self.push(OutlineCommand::MoveTo([x, y]), &[[x, y]]);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        if !self.open {
            self.failed = Some(OutlineError::InvalidOutline);
        }
        self.push(OutlineCommand::LineTo([x, y]), &[[x, y]]);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        if !self.open {
            self.failed = Some(OutlineError::InvalidOutline);
        }
        self.push(
            OutlineCommand::QuadTo([x1, y1], [x, y]),
            &[[x1, y1], [x, y]],
        );
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        if !self.open {
            self.failed = Some(OutlineError::InvalidOutline);
        }
        self.push(
            OutlineCommand::CurveTo([x1, y1], [x2, y2], [x, y]),
            &[[x1, y1], [x2, y2], [x, y]],
        );
    }
    fn close(&mut self) {
        if !self.open {
            self.failed = Some(OutlineError::InvalidOutline);
        }
        self.open = false;
        self.push(OutlineCommand::Close, &[]);
    }
}

#[cfg(test)]
mod tests;
