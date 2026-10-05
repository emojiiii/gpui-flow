# Text under rounded and affine ancestor clips

> 旧示例说明：本文涉及 Studio 的示例名称、源码路径、操作和验证结果均为历史内容，相关演示已移除；库 API 契约继续保留。当前工作台入口见[入门](getting-started.md)，迁移范围见[说明](studio-retirement.md)。

Status: bounded Linux tests and native acceptance passed on 2026-10-02. This closes automatic domain selection for an existing glyph/mask
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
transform. That cancellation must leave a supported resulting matrix; tiny
nonzero off-diagonal residues are still rejected by the existing strict Text
contract. This is not rotated/sheared glyph support.

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

## Validation (2026-10-02)

- Eight new tests cover automatic/native domain selection, opposite and nested
  corners, synthetic mask equivalence, domain budgets, actual GPUI glyphs and
  padding, missing backend/unsupported transform, stale jobs with Undo, and
  upright glyphs beneath a binary-exact affine ancestor
- Affected library suite: 117 UI + 200 composite tests passed, zero failed or
  ignored. After adding the affine fixture, all four UI clip tests passed again.
  There are 318 distinct passing tests across these runs; rerun overlap is not
  additive. Unchanged workspace modules were not broadly rerun
- Strict two-crate all-targets Clippy, final UI/studio Clippy, formatting and real
  one-job native linking passed. Existing caches and dependencies were reused
- Early test runs caught the curved mask-edge coverage issue and a historical
  Image test expectation that Text stayed native beneath rounded clips. Both
  were corrected. The initial generic rotation/inverse demo left a tiny
  unsupported Text shear residue; the fixture now uses exact shear cancellation
  rather than weakening the established transform validation
- Native cloud window ran 09:37:07–09:38:50 UTC, exited 0, and had empty stderr.
  Actual glyph pixels showed rectangular native paint, nested rounded clips and
  upright text under a sheared ancestor. Corner swaps, padding, clip disable
  (native return), font-bridge removal (explicit TextBackendRequired), recovery,
  repeated edits and Undo all passed. Final window inventory confirmed closure
- The host opt-in font bridge remains required. This does not claim default
  arbitrary-font support, rotated/sheared Text, cross-platform acceptance,
  bitmap-alpha hit testing, or browser Canvas context parity

The final native/test source snapshot is tree
`06113a40b0c4a750a66ca680ec949fcb335387a4`, archived with SHA-256
`bc673559f98b5a12010f6892d35674820ee1a789f19239f86a4e432a05e695f8`.
The Linux example binary SHA-256 is
`274698cb33477da99b68e3f5e4f189b6b0e7be389d77c280f660ad228b427f23`.
Only documentation changed after final focused checks and native acceptance.

`cargo run -p flow-studio --bin text_clips` shows rectangular native text,
automatic rounded CPU text and upright text under an affine container clip.
Controls swap corners, toggle both ancestor clips/padding, withdraw/restore the
font bridge and Undo document edits.

Later [affine Text rendering](text-affine.md) extends the earlier positive-uniform
glyph restriction. This document's original test/window receipt remains historical.
