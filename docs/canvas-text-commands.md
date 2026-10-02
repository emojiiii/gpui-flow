# Canvas text-box commands and native measurement

Status: implementation and 8 new regressions pass; 119 distinct affected checks,
strict Clippy and native linking pass. Real-window acceptance is pending. This is a bounded text-box API, not full browser CanvasText.

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
StrokeText, baseline variants, persistent current paths and image patterns remain
unimplemented; ELM-10 stays partial/in progress until its fixed table scope closes.
