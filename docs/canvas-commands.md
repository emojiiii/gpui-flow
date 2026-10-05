# Canvas vector command batches

> 旧示例说明：本文涉及 Studio 的示例名称、源码路径、操作和验证结果均为历史内容，相关演示已移除；库 API 契约继续保留。当前工作台入口见[入门](getting-started.md)，迁移范围见[说明](studio-retirement.md)。

Status: implemented and validated on Linux. This is a bounded vector context
subset, not a complete browser Canvas API.

`CanvasSurface::draw_commands` accepts owned `CanvasCommand` values: Save,
Restore, Transform (postmultiply), SetTransform, SetOpacity, FillPath and
StrokePath. State begins at identity / opacity 1 on every call. The save stack
must balance; underflow and a leftover Save reject the whole batch. Owned Path
coordinates and paint geometry are evaluated under the transform at that draw.
This follows the pinned core [Canvas path, transform and state contracts](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/interface/src/canvas/ICanvas.ts),
with explicit transaction-local state rather than a persistent browser context.

The independent `canvas_surface/commands.rs` compiler builds a bounded sibling
domain and uses the existing compositor and Canvas draw transaction. It avoids
repeated scene insertion, whole-scene cloning or validation of earlier commands.
It reuses scene Path Q/C geometry, nonzero/evenodd fills, solid/linear/radial
paints, and the shared Kurbo/Lyon centered stroke mesh for cap/join/miter/dash.
No scan converter, stroke expander, paint engine or dependency is added. One draw
has one paint; alpha is applied per draw and overlapping draws compose in order.
A stroke's triangle union is painted once, preserving translucent edge alpha.

There are at most 256 commands and 16 saved states (or a lower caller limit).
Existing node/source-point/triangle/gradient/dash/depth and surface-byte/work
ceilings remain enforced. Invalid transforms (including singular), opacity,
geometry, stack operations and exhausted budgets reject without publishing any
pixels. Matrices are finite and each coefficient is bounded to one million.
Source geometry is independently bounded, not charged as pixel-surface memory.
There is no new process-wide memory guarantee.

Coordinates are logical Canvas units. The existing effective physical-size/DPR
mapping is applied once. Successful visible output publishes one immutable
surface generation, even for many commands. Empty/state-only/invisible batches
return false. Adapter no-op edits now keep the existing upload identity as well
as the unchanged generation. Failed edits retain the previous snapshot and do
not alter the scene, history or resource generation. Pixel edits are runtime
content, so configuration Undo does not replay commands.

A later [per-draw clip-path slice](canvas-clips.md) adds batch-local clipping.
The later [current-path slice](canvas-current-path.md) adds persistent geometry;
this original batch slice did not implement current paths, clearRect, image/text commands,
patterns, filters/shadows, non-source-over modes,
inside/outside strokes, hit testing, persistent context state or browser API
compatibility. Existing explicit clear/image/subtree/pixel APIs remain available.
Stroke defaults are Flow's documented Round/Round, not browser Butt/Miter; callers
can explicitly select the latter through the existing Stroke type.

## Validation

Eleven new regressions cover actual curve/hole pixels, postmultiply order,
save/restore and state isolation, DPR/gradient placement, per-draw alpha, shared
stroke union/dashes/caps, immutable snapshots, invalid late commands, input/stack/
work/memory limits, resize behavior and resource generation/lifecycle. A no-op
adapter edit now preserves its existing upload rather than replacing it.

Affected tests: **360 passed, zero failed or ignored** (142 UI + 218 composite).
Strict affected all-targets Clippy, example Clippy and the actual native example
build passed. The exact production source stayed unchanged through these checks.

The native `canvas_commands` example ran 2026-10-02 11:41:04–11:43:20 UTC and
closed normally with exit 0 and empty stdout/stderr. Actual DPR 1/2 surfaces
showed matching logical placement for curves, gradient, rotated evenodd holes,
dashed curves and per-draw alpha. Checks covered repeated origin overlays, late
invalid restore rollback, safe crop/regrow, reset resize, configuration Undo,
Clear and regeneration. Saved artwork-region pixel comparisons were identical
before/after rejection and between the initial and final settled redraw.

Exact source commit `6361740e476d714c5722a8efc124f2f2d5d82f18`, tree
`14de502a0cfa67fdc79ddf33611ffbe5771c5e90`; archive SHA-256
`2ec7088e6b130ab499e4b526fa8e0c87490a455222beb442582ec742f92fbaa8`;
binary SHA-256
`3c6090d6a9218573f0a9af88fcd0e163182f651ac9487e6b09a366c14564fed0`.
Screenshots and close receipts were retained with the validation artifacts.
No browser/cross-platform pixel-equivalence claim is made.

Reproduce with `cargo run --locked -p flow-studio --bin canvas_commands` on a
native GPUI desktop. The example's two Canvas edits are separate transactions;
the atomic guarantee applies to each surface batch, not to multiple surfaces.
