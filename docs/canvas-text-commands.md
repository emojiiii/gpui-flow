# Canvas text-box commands and native measurement

Status: implementation and 8 new regressions pass; 119 distinct affected checks,
strict Clippy, native linking and real Linux-window acceptance pass. This is a bounded text-box API, not full browser CanvasText.

`CanvasCommand::FillTextBox { text, position, color }` reuses the retained plain
Text configuration in an imperative pixel batch. Position is the logical top-left
of its explicit box, before current transform and Canvas DPR. It is not an
alphabetic baseline overload. Font family/size/weight/italic, padding, alignment,
wrap, clip/ellipsis and decorations keep their existing Text semantics. A solid
color is explicit; batch opacity is applied once to the resulting glyph layer.
Existing Save/Restore and frozen Path clips apply before per-draw source-over.

`SceneView::draw_canvas_commands(target, commands, images, cx)` uses the same-App
font bridge already validated by `set_text_rasterizer`. The independent adapter
reuses the existing TextPreparation and actual GPUI glyph backend, including
local affine raster space, exact content clips, superseded font epochs and glyph
budgets. It does not create a new font system, substitute rectangles for ink or
fetch image inputs. Missing font backends fail explicitly before pixel publication.
Commands are synchronous and bounded; this is not a replacement frame scheduler.

`SceneView::measure_canvas_text(request, cx)` reuses GpuiTextMeasurer against the
App's TextSystem. Returned width, height, rows and line height are logical shaping
metrics, excluding command transform, DPR, ellipsis and clipping. The request has
explicit typography and optional wrap width. Measurement does not need the CPU
raster bridge. Its metrics are line-box layout metrics, not promised browser
actualBoundingBox ink metrics. Platform fallback remains the native backend's job.

A headless host can use `CanvasSurface::draw_commands_with_text` with an explicit
font producer. It receives the admitted domain, physical Canvas viewport, limits
and retained-byte charge, and returns TextInputs plus actual raster work. This is
a trusted host backend contract, like existing TextInputs, not an untrusted-code
sandbox. Batch-local IDs must not be used as persistent cache identities. All
returned images are charged; reported raster work shares the compositor's write
budget. The library validates commands before invoking the producer, and producer
errors, missing ink, late commands or composition budgets preserve generation and
pixels. Existing vector/image entry points remain compatible and provide no font
backend.

Per-command text is at most 64KiB UTF-8, family at most 512 bytes, aggregate text
and family bytes at most 256KiB per batch, with existing 256-command/node/clip
budgets. Position and box dimensions must be finite and within 1,000,000 logical
units. Native glyph count, output pixels and bytes retain existing hard ceilings;
reduced Canvas working-set/dimension/work limits still apply. Pixel edits do not
enter document/history. Configuration Undo remains separate.

Pinned reference: [CanvasText and CanvasTextDrawingStyles](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/interface/src/canvas/ICanvas.ts).
Flow deliberately exposes an explicit Text box rather than parsing a CSS font
string or pretending top-left coordinates are browser alphabetic baselines.
Later increments provide [persistent current paths](canvas-current-path.md),
[image patterns](canvas-patterns.md) and genuine
[same-App outline StrokeTextBox](canvas-stroke-text.md). Their source and native
evidence are separate from the fill/measure checkpoint below. Browser baseline
variants remain outside the explicit top-left-box contract.


## Validation

Eight new regressions pass (six synthetic-ink compositor protocol tests and two
UI adapter tests). The affected commands passed **119 distinct checks**: 69
composite Canvas tests plus UI filters Canvas=20 and Text=32, with their two new
CanvasText tests counted once. Zero failures or ignored tests. Strict composite/UI
all-targets Clippy, example Clippy, formatting and actual native linking pass.
Synthetic test ink validates protocol/alpha/clip/atomic budgets, not real fonts;
real-font evidence comes from the window below.

The frozen `canvas_text` executable ran 2026-10-02 14:37:06–14:39:36 UTC, closed
normally with exit 0 and empty stdout/stderr. Both DPR1 and DPR2 rendered actual
Latin ligatures/accents and CJK fallback, clipped ellipsis, underline, rotated
italic text and alpha. Curved and nested even-odd clips affected glyph ink, not
layout rectangles. Late-command rejection and missing font bridge kept existing
pixels. Measurement still returned the same native shaping metrics without the
raster bridge: 240.0 × 110.4 logical units, four lines; the bounded painted box
correctly showed fewer lines with ellipsis. Safe resize/configuration Undo,
Clear and redraw retained their documented runtime-pixel semantics.

Four artwork comparisons were pixel-identical to the initial image: late error,
missing backend, restored unclipped redraw, and Clear+redraw. This is not a
browser-baseline or cross-platform/font-installation guarantee.

Exact source checkpoint `003288b3a9b4178890c162a4f1271f3b8dd41f4f`, tree
`c7c272c21301ca63c3ccc51cf90927d3990dac0e`; archive SHA-256
`2a8f547983d00a8c9ad2b015ce56c8e262af1f1747dfcd15e5ed9526fe432073`;
binary SHA-256
`78d7c2397f69a93c5ae24a5797e349caff92d7e3156a0f5e68f1f0c2698fdb38`.
The already-linked executable was frozen before parallel Image-hit source work
began. That later source is not claimed as covered by this receipt. Screenshots
and the normal close receipt are retained with validation artifacts. Reproduce:
`cargo run --locked -p flow-studio --bin canvas_text`.
