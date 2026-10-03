# Bounded static-font outline foundation

Status: CPU-only foundation behind `gpui-flow-text/static-outlines`. This does
not enable Canvas `strokeText`, register a font, prove a native FontId binding,
reshape text, or replace the existing GPUI fill-text raster path. Native and
cross-platform acceptance remain unperformed.

`PreparedFont::prepare` accepts bytes the host is authorized to use and modify.
Authorization and licensing are the host's responsibility; a valid font or
license string does not prove legal rights. Flow does not fetch fonts. The
included tiny quadratic/cubic test fonts contain original geometric test shapes,
with their CC0 generator alongside them; no system font is redistributed.

## Admission and identity

- One static SFNT face, index zero; at most 16 MiB and 128 sorted, unique,
  nonoverlapping tables, with table checksums and whole-SFNT checksum verified
- Complete required metrics/name/cmap tables, normal width, actual static
  weight 1–1000 and upright/italic style; an exact-style check rejects requests
  that would need synthetic weight or slant
- A nonzero `m` cmap glyph with advance and real, supported outline geometry
- TTC, WOFF, variable, color/bitmap/SVG alternate representations, signed fonts,
  ambiguous outline formats and oblique/width variants fail explicitly
- A fresh 128-bit operating-system-random private family/PostScript alias;
  the family, full-name, unique-ID and PostScript identity records are replaced,
  including localized aliases
- Copyright, trademark, designer, license, URLs and other non-identity records
  are preserved byte-for-byte, including format 1 language tags. Name tables
  and cumulative copied metadata are bounded to 65,535 bytes
- The SFNT is rebuilt with aligned tables, corrected lengths/checksums and
  `head.checkSumAdjustment`. Other table payloads are unchanged. The original
  SHA-256, exact prepared-byte SHA-256, face index and static style are exposed

The alias and hashes identify these immutable bytes only. They do not establish
that GPUI loaded those bytes. In particular, CFF internal naming remains intact;
this is a private SFNT-name alias intended for the separately audited pinned
Linux backend, not a claim about every native loader's naming rules.

## Real outlines and limits

`outline_glyph` accepts an existing shaped glyph ID. It returns genuine
unhinted design-unit paths, y-up, through pinned `ttf-parser 0.25.1`: move, line,
quadratic, cubic and close commands. It does not perform independent shaping or
map text characters to glyph IDs. The constructor's `m` lookup is solely an
admission probe for the pinned loader's requirement.

TrueType simple outlines and bounded same-face composites are supported.
Composite traversal checks component IDs, full records, cycles, depth and work
before entering the parser. One-point simple glyphs are rejected even as a
composite child, so a valid sibling cannot hide skipped geometry. Point attachment, round-to-grid flags, scaled offsets
and ambiguous transforms fail explicitly; scaling requires an explicit unscaled
offset declaration. There is no hinting emulation.

CFF support is intentionally narrow: one non-CID Type 2 font, an em-compatible
matrix, direct line/cubic/flex programs. Local/global subroutine calls, seac,
unsupported operators and alternate CharstringType declarations fail explicitly.
A bounded tokenizer proves no hidden recursive program is passed to the parser.
This subset is useful for real cubic extraction but does not claim support for
most production CFF fonts.

A shared `OutlineBudget` caps a whole job at 8,192 glyphs, 262,144 commands,
16 MiB command allocations and 2,000,000 preflight work units. Each glyph has
ceilings of 16,384 commands, 65,536 work units and 16 composite levels. Font glyph
count is capped at 16,384. Host limits can only lower these ceilings. Returned
vector capacity is charged, including reserved unused slots. Font preparation
storage is separate from this job budget and bounded by input/table/name limits.

Every glyph also charges the SFNT directory count and, for CFF, the full CFF
table byte length before reparsing the face. Empty/repeated CFF glyphs therefore
cannot bypass dictionary-parse work accounting. The 65,536 per-glyph ceiling
also limits admitted CFF table size (less than 64 KiB after other work), even
though the general SFNT input limit is 16 MiB.

Preflight bounds parser expansion even when the output budget is exhausted.
All partial callbacks are discarded on parser error, and the caller's budget is
unchanged on failure. Parser `None` does not mean whitespace: an empty result
requires separately verified empty glyph data and no emitted geometry. Empty
and repeated glyphs still consume the shared job budget. Bounds are parser
bounds that may include control points, not browser ink metrics.

## Next integration gate

A native bridge must prove sole registration of these exact bytes in the same
App's captured platform text system, with a fresh actual Flow descriptor/cache
key, then bind FontId, hash, face index, style, App identity and epoch. Family
name equality and `add_fonts` returning `Ok` are insufficient. Every unbound
fallback FontId must reject the entire candidate. Shaped placement, stroke
geometry, affine/DPR behavior, clips, alpha, stale epochs, byte/write budgets and
atomic Canvas publication still need implementation and native acceptance.

CPU verification:

```sh
cargo test --locked --offline -p gpui-flow-text --no-default-features --features static-outlines
cargo clippy --locked --offline -p gpui-flow-text --no-default-features --features static-outlines --all-targets -- -D warnings
```

## Local CPU receipt (2026-10-02)

The feature-enabled focused suite passed 28 unit tests (25 new outline tests)
and one existing doctest, with zero failures/ignored tests. The unchanged
default-feature contract passed three unit tests and one doctest. Strict
feature-enabled all-targets Clippy and workspace formatting passed. Tests cover
real quadratic/cubic coordinates, translated composites, exact byte identity,
license/language preservation, metadata-amplification limits, malformed SFNT
ranges/checksums/headers, every truncation of both fixtures, unsupported font
representations, recursive composites, CFF program restrictions, partial-parser
failure and atomic shared-budget accounting. Independent review caught and fixed
repeated CFF Face-parse work amplification and a mixed composite hiding a
one-point child; four regressions cover those fixes. Final independent reruns
passed on the amended source. These are CPU results only.
