# Text under rounded and affine ancestor clips

Status: implementation checkpoint; final affected-suite and native acceptance
are pending. This closes automatic domain selection for an existing glyph/mask
capability, rather than introducing a second font renderer.

## Contract

A Text leaf with a nonrectangular ancestor clip, or a clip whose world matrix
cannot be expressed by the native positive-uniform rectangular mask, now creates
an automatic bounded CPU domain. Box/Frame per-corner radii and the complete
nested clip chain remain exact geometry. The Text does not need an unrelated
blend mode, opacity or effect to activate the CPU path. Ordinary axis-aligned
rectangular clips continue through the native fast path.

The host must still explicitly supply the same-Application GpuiTextRasterizer via
SceneView::set_text_rasterizer. Without that bridge, the domain reports
TextBackendRequired; it never draws a rectangular approximation or untracked
font fallback. Existing one-solid-fill, no-stroke, positive-uniform Text world
transform limits remain. An affine ancestor clip is supported when the Text's
own resulting world matrix stays supported, for example with an inverse local
transform. This is not rotated/sheared glyph support.

- Reuses existing GPUI shaping, padding/vertical alignment, source snapshots,
  tiny-skia masks, resource/font epochs and atomic bounded composite jobs
- No JSON/schema/default, glyph hit policy or intrinsic bounds changes
- Ancestor corner edits change the domain key. Late jobs are cancelled and cannot
  republish stale glyph pixels. Clip disable can return the leaf to native paint
- Per-domain/scene domain count, dimensions, source bytes and pixel-work ceilings
  are unchanged. More separately clipped Text leaves may reach the existing
  eight-domain ceiling; grouping into an explicit owned container domain remains
  an existing alternative, not an automatic bypass of limits
- Ancestor-mask allocation keeps one physical pixel of conservative coverage
  guard, matching internal shape allocation. The actual path mask removes
  outside pixels before presentation. Cutting a curved path's mask surface
  exactly at its AABB had changed antialias coverage; the new regression caught
  and fixes that. Internal versus final-mask byte rounding may differ by one
  alpha byte, but no geometric corner is replaced with a rectangle

## Pinned source and reuse

Leafer's [BoxData clip-after-fill predicate](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display-module/data/src/BoxData.ts),
[Box content rendering](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display/src/Box.ts)
and [Frame hidden-overflow default](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display/src/Frame.ts)
apply to children through the container, rather than requiring each Text child
to introduce an unrelated isolation style. This independent Flow adapter closes
that behavior gap while retaining its explicit font-capability contract.
No upstream implementation or new dependency was imported.

## Validation checkpoint

Seven new tests cover automatic/native domain selection, opposite and nested
corners, synthetic mask equivalence, domain budgets, actual GPUI glyphs and
padding, missing backend/unsupported transform, and stale corner jobs with Undo.
The first explicit-container comparison exposed the mask-edge guard issue;
final broad affected tests and native acceptance are still in progress.

`cargo run -p flow-studio --bin text_clips` shows rectangular native text,
automatic rounded CPU text and upright text under an affine container clip.
Controls swap corners, toggle both ancestor clips/padding, withdraw/restore the
font bridge and Undo document edits.
