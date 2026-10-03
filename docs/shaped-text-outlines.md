# Same-App shaped text outlines

`gpui-flow-text` exposes `GpuiTextRasterizer::outline_text` under
`static-outline-registration`. The bridge must come from the audited startup
factory. The caller supplies the consuming App's `Arc<TextSystem>`, the captured
bridge epoch, cancellation flag and one mutable `OutlineBudget` shared across
its complete Canvas batch. Generic bridges have no outline capability.

The request is the existing `TextRasterRequest`. It retains exactly the fill
route's isotropic shaping-space contract: font size, absolute line height,
paragraph spacing, wrapping width, alignment bounds and clip are expressed in
one raster-logical coordinate system. Insets/padding are already represented by
that caller-supplied alignment box. This API does not invent another layout or
apply scene affine transforms.

## Placement and geometry

The bitmap and outline consumers use the same `gpui_placement` visitor over
`shape_text_in_box` results. It retains the paragraph offsets, wrapped-row
alignment, vertical block alignment, centered leading and each actual native
glyph offset. The bitmap consumer alone performs the prior native integer and
quarter-pixel quantization. The vector consumer uses the unsnapped baseline.

Each returned `PositionedGlyphOutline` contains the actual GPUI `FontId` and
`GlyphId`, the unsnapped baseline origin and real outline commands. Coordinates
are y-down isotropic raster-logical values **before device scale**:

- x = baseline_x + design_x × actual_layout_font_size / units_per_em
- y = baseline_y − design_y × actual_layout_font_size / units_per_em

Quadratic, cubic and close commands remain explicit. The CPU extractor's real
compound outlines retain their component geometry. Empty verified glyphs remain
in the returned glyph vector and consume glyph/parse work. A downstream Canvas
producer maps coordinates back into the Text box's local logical space, joins
all glyph contours into its Path, then lets the existing scene compositor apply
the affine transform and DPR exactly once. This is unhinted vector geometry;
matching native hinted bitmap fill pixels is not promised.

## Atomic failure and identity

The request's primary face, every actual shaped run and every glyph must pass
same-owner, bound native ID, exact static style, epoch, cancellation and glyph
range checks. Glyph zero and emoji runs fail explicitly. There is no fallback
replacement, character-to-glyph remapping, alpha dilation or glyph-bounds stroke.

The checked shared overflow helper validates the original shape, every ellipsis
width probe and the final shape. Therefore an unbound fallback in a clipped-away
row or an ellipsis-discarded suffix still rejects the entire request. Clip
intersection and alpha do not bypass identity checks. Decorations other than
`None` fail explicitly for outlines; ordinary fill decoration behavior remains
on its existing path.

Output and all pending budget mutations are private until the whole request
succeeds, including the final cancellation/epoch check. Any validation, geometry,
allocation, budget or native panic failure returns no partial output and leaves
the caller's budget unchanged. Whole-batch publication and any final epoch check
still belong to the Canvas caller.

## Budget scope

`OutlineBudget` is shared across the complete batch, including repeated and
empty glyphs. Extraction consumes the existing bounded parser glyph, command and
work quotas. Every original/probe/final run and glyph validation consumes work;
command transformation consumes additional work. The command vector is
transformed in place, so there is no second glyph-conversion command vector.

Byte accounting covers the actual retained capacity of the outer glyph vector,
each command vector, and simultaneously live line-offset vector capacities.
Both original paragraph offsets and final ellipsis offsets are admitted before
allocation using checked count-to-byte arithmetic and fallible exact reservation.
Final offsets include the still-live original offsets in that admission, and
filling the reserved vectors never grows them. Actual reported capacities are
checked as well. Temporary offsets are released only after they are dropped. Returned
`allocated_bytes` is the retained output storage; `peak_bytes` is additional
accounted storage for this request, including its live offsets. Caller-supplied
lower byte/work quotas stay effective.

`gpui_outlines::charge_outline_budget` lets the Canvas producer charge Path
storage and conversion/concatenation scratch while the glyph vectors remain
live. `release_outline_bytes` releases only storage already charged by that job,
after it has actually been dropped; parsing/validation/conversion work is never
refunded. The caller must retain charges for all still-live outputs. Immutable
per-font bytes, GPUI-owned shape/font caches and other shared shaping-helper
scratch are separately scoped; these counters are not an exact process-RSS claim.

## Verification boundary

Headless modeled-platform tests exercise placement and consumer rejection with
real first-party TTF/CFF fixture geometry, including counters, period, composite
accents and mark offsets. They cover paragraph/wrap/alignment, case/ellipsis,
actual layout font size, DPR-independent vector coordinates, mixed invalid run
order, fully clipped candidates, unsupported decorations, wrong owner/style,
stale/cancelled snapshots and aggregate capacity/work budgets.

These tests do not establish native font identity or native GSUB/GPOS behavior.
The separately sealed startup identity gate is independent. Subsequent native
Canvas strokeText and publication evidence is recorded in the
[combined native report](canvas-stroke-text-native.md), with its original
failed run preserved.

The focused registration-only unit graph passed all 68 tests (12 new consumer
cases and two offset-admission helper tests) with no native platform construction:

```sh
cargo test --locked --offline -p gpui-flow-text --no-default-features \
  --features static-outline-registration --lib
```

The native-X11 isolated-package attempt was stopped by the storage watchdog
while rebuilding a different dependency graph, before this crate or its tests
ran. The successful run reused the existing registration-only graph, with
`CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0`, one Cargo job and
incremental compilation disabled. Native Canvas acceptance remains separate.

Strict all-target Clippy for the same registration-only graph also passed with
`-D warnings`; targeted source formatting passed. The successful test and Clippy
runs both had identical source hashes before and after execution. The checked
summary is in `docs/validation/2026-10-03-shaped-text-outlines-summary.json`.

Parent review preserved fill’s device-space origin limit for sub-unit DPR: the
shared visitor checks finiteness, while only outline extraction applies the
logical-space cap. A dedicated regression passes with the final 68-test run.
