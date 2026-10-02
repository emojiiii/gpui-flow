# Canvas vector command batches

Status: source checkpoint; nine focused CPU pixel tests and two adapter tests
pass. Broader checks and native acceptance are pending. This is a bounded vector context subset.

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

This does not implement current-path construction, arbitrary clipping stacks,
clearRect, image/text commands, patterns, filters/shadows, non-source-over modes,
inside/outside strokes, hit testing, persistent context state or browser API
compatibility. Existing explicit clear/image/subtree/pixel APIs remain available.
Stroke defaults are Flow's documented Round/Round, not browser Butt/Miter; callers
can explicitly select the latter through the existing Stroke type.
