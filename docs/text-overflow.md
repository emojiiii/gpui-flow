# Text fixed-box end ellipsis

Status: bounded Linux model, real-glyph and native-window acceptance passed on
2026-10-02. This is a fixed-box end-ellipsis slice, not full typography parity.

## Contract

Text.overflow adds opt-in `ellipsis`; its default `clip` is omitted from JSON and
preserves existing documents. The original content, explicit bounds, padding
and intrinsic measurement are never rewritten by paint. Font/resource keys
include the shape policy and ancestor/context snapshots as before.

The independent shared flow-text GPUI adapter:

1. Shapes the original text with the existing backend and actual wrapping
2. Uses GPUI's wrap boundaries, not a new line breaker, to identify visual rows
3. Retains the rows fitting the padded height, with at least one row for a box
   shorter than the line height. Hidden following rows mark the last retained
   row with `...`; unwrapped over-wide rows receive their own end marker
4. Selects a grapheme-prefix candidate using the existing locked
   unicode-segmentation 1.13.3 crate and actual GPUI candidate shaping. It measures
   prefix plus marker together rather than summing character advances
5. Feeds identical final shaped rows to native glyph painting and CPU raster.
   Original block height keeps vertical overflow top-aligned. Existing left,
   center and right alignment operate on the painted rows

The first-fit unchanged path retains original native WrappedLine values. A box
narrower than `...` paints the marker under the existing real clip; it does not
invent smaller glyphs. Prefix search is bounded and conservative under unusual
nonmonotonic kerning. Wrap boundaries that split a grapheme or have unsupported
logical order are rejected rather than cutting a cluster.

Ellipsis admits at most 64KiB UTF-8 source, 8192 source/output glyphs and 8192
visual rows. Scene mutation rejects the byte overflow atomically; backend-only
limits become explicit font-resource/paint errors. Pixel/cache/work budgets and
font epoch cancellation remain in the existing raster pipeline. Ordinary legacy
Clip retains its previous native/CPU differences and does not gain this new
source cap.

## Alignment and limits

Pinned Leafer [TextClip](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/partner/text/src/TextClip.ts)
uses `...` for its ellipsis mode, clips overflow rows and distinguishes nowrap
from the final wrapped row. Its
[TextLayout](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/partner/text/src/TextLayout.ts)
keeps clipped vertical overflow top aligned. This bounded Flow implementation
reuses that semantic intent with GPUI's real metrics and grapheme boundaries.

Differences remain explicit: Flow preserves its clipped default rather than
Leafer's `show`; there is no arbitrary custom overflow string, marquee,
start/middle ellipsis, rich text, paragraph spacing, vertical writing or promise
of browser layout equality. Flow may retain no source grapheme when only the
marker fits; Leafer's pinned routine tries to retain one character. Intrinsic
Natural/AutoHeight measurement always reads original content and cannot shrink
itself because the previous paint happened to be truncated.

The contracts-only flow-text default remains dependency-free. Its optional GPUI
feature reuses the already locked Unicode crate, with no version update or new
font/shaping library. TextRasterRequest gains an `overflow` field; downstream
Rust struct literals must choose Clip to retain the old behavior. Repository
callers were migrated.

## Validation (2026-10-02)

- 16 new regressions: 3 scene/default/budget, 2 factory/resource/atomicity,
  1 editor history, 1 intrinsic layout, 8 real GPUI shaping/raster fixtures and
  1 late composite-job invalidation case
- 152 affected tests passed: 127 UI, 9 optional-GPUI text and 16 targeted model,
  text-box/layout/history tests. Zero failed/ignored. Focused earlier runs
  overlap these totals and are not additive; unchanged whole-workspace tests
  were not broadly rerun
- Strict six-crate all-targets Clippy, final example Clippy, formatting, locked
  checks and one-job native linking passed. The contracts-only cargo tree still
  contains flow-text alone. The Unicode dependency version was reused unchanged
- Native window ran 10:02:01–10:04:21 UTC and exited 0 with empty stderr.
  Side-by-side native/CPU actual glyphs matched for wrapped last-row markers,
  nowrap paragraph markers, centered/bottom rows, vertical-overflow top alignment,
  combining accents, family ZWJ emoji, flag emoji and CJK. Tiny boxes clipped the
  real marker without scaling it. A taller box restored the complete original
  Unicode content; six Undo steps restored the initial wrapped scene
- Screenshots are preserved in the native tool record; final inventory confirmed
  closure. No browser layout, all-font, all-bidi or cross-platform claim is made
- The initial compile required exporting the new scene enum. Strict example
  Clippy caught ambiguous assignment spacing, corrected before final link and
  native acceptance. No production input changed during the passing final run

Final native source tree: `b30f2d66f3d9167fb7240ab150d6eaea984872f6`.
Source archive SHA-256:
`801a9772212f90c7fc9bd3ac82b14ac5f94bc4592560425289cfa77ea8a9ca7c`.
Linux example binary SHA-256:
`d33689d558c3f4661d4c5d195f7cc2f42f79ddedfe9e82b93b3e4bdd90cb6c7a`.
Only documentation changed after that exact native snapshot.

Run `cargo run -p flow-studio --bin text_overflow` to compare both paths and
exercise clip/ellipsis, wrapping, height, vertical alignment, Unicode, tiny width
and Undo.
