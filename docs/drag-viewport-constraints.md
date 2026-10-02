# Fixed drag constraints and clock-driven viewport navigation

This INT-06 slice is an independent, opt-in retained-scene contract. Existing
unconstrained drag, background/space/middle-button pan, ordinary wheel pan and
command-wheel zoom defaults remain unchanged. Inertia, bounds and snapping are
not installed by default. No core `Viewport` document fields changed.

## Source evidence

Compared read-only with the pinned official revisions:

- [UI DragEvent](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/event/src/DragEvent.ts): total world displacement converted into local movement before limits
- [UI DragBoundsHelper](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/event/src/DragBoundsHelper.ts): fixed/parent bounds, local axes and inner/outer containment
- [UI Dragger](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/interaction/interaction/src/Dragger.ts): saved starting positions, gesture ownership and interrupt lifecycle
- [Viewport Dragger extension](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/viewport/src/interaction/Dragger.ts): opt-in decaying movement, recent release samples and next-render scheduling
- [Scroller](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/scroller/src/Scroller.ts): viewport/content bounds and constrained movement

The Rust model deliberately uses continuous-time exponential integration instead
of Leafer's frame-count damping/flick boost. It preserves our stronger rollback
on canceled document gestures. It does not implement drag-out auto-scroll,
spring/bounce physics, nested scroller arbitration, collision or pathfinding.

## Drag containment and snapping

Install `SceneDragConstraintsPlugin { constraints: SceneDragConstraints { bounds,
axis } }`. `DragBounds::World(Rect)` constrains the selected roots' shared
world-axis paint-bounds box. `DragBounds::ParentLocal(Rect)` constrains the union
of their parent-local boxes: every root must have the same parent, with roots
sharing an identity parent. Rotation, reflection, skew and nonuniform parent
scale are supported through vector conversion. A singular parent or mixed-parent
selection is rejected atomically. Bounds are fixed runtime values, not inferred
from layout or serialized object data.

`DragAxis::{Both,X,Y}` is measured in that same frame. Without bounds it is
world-axis locking. Containment takes precedence if existing content starts
outside the rectangle. If content is too large on one axis, that axis is centered
instead of alternating between edges. Zero-sized bounds are supported. Geometry
is an axis-aligned enclosure, not a path/paint-alpha collision boundary.

`SceneSnappingPlugin` remains the independently switchable world grid.
`SceneEdgeSnappingPlugin` optionally aligns selected bounds edges/centers with
visible unrelated scene objects. Hidden subtrees, selected descendants and
selected ancestors are excluded. Ties are deterministic in paint order. Its
`include_centers` switch restricts alignment to edges when false. It performs a
scene scan, not a large-document spatial-index/performance guarantee.

Order is grid (50), edge/center (60), hard drag constraints (1000). Bounds can
reject a snapped position; any resulting false guides are cleared. Custom
middleware should run before hard constraints. These optional plugins constrain
`MoveSelectionTo`, not unrelated transform/property APIs or trusted direct
editor writes. Hosts can independently enable/disable/replace/uninstall each.

A drag uses generation/document/revision-guarded selection capture. Its accepted
revision advances only after its own successful move. Reparent, detach/reattach,
replacement, external mutation or selection change invalidates the old gesture
without restoring an obsolete snapshot. Normal cancel, Escape, focus loss,
button loss, policy change or plugin disable rolls back. Multiple moves still
form one Undo entry. A wrong pointer cannot move/release the captured gesture.

## Viewport bounds and wheel policy

Install `SceneViewportConstraintsPlugin` with `SceneViewportConstraints`:

- Optional `min_zoom` and `max_zoom` intersect the existing `Viewport` limits;
  nonfinite, nonpositive and disjoint ranges reject navigation atomically
- Optional `ViewportBounds { world: Rect, canvas: Size }` keeps the logical-pixel
  canvas inside a fixed world rectangle; smaller content centers on each axis
- Pan, zoom, fit and `SetViewport` pass through the same normalization into one
  validated atomic `SetViewport` command
- Zoom clamps before solving its cursor anchor. The anchor remains fixed except
  where a world boundary requires a correction. Fit retains its canvas center
  when its zoom needs clamping
- Navigation remains allowed under ordinary read-only editing policy; an explicit
  Navigation denial blocks it. It never creates scene edit-history entries

Canvas size is application supplied; replace constraints when layout changes.
All positions, pan, speed, drag thresholds and snap tolerances use **logical
canvas pixels**. Convert physical pixels/DPR only at a platform boundary, never
multiply these policies by DPR a second time.

`ScenePanPolicy` separately enables background, space and middle-button drag,
plus optional Shift-wheel horizontal conversion. Set its `wheel` and
`SceneZoomPlugin::with_wheel_policy` consistently to one `SceneWheelPolicy`:
`CommandZoom` (legacy), `PanOnly`, `ZoomOnly`, or `Disabled`. Normalized wheel
input does not identify mouse versus touchpad. The adapter owns unit conversion
and native default suppression. Pinch, touch contacts, device rotation, pen
metadata and operating-system momentum phases remain EVT-03 work; this slice
must not be described as multi-device gesture parity.

## Explicit clock and inertia lifecycle

Replace the pan plugin with
`ScenePanPlugin::default().with_inertia(PanInertiaOptions::default())` to opt in.
Default tuning is 8/s decay, 8 logical px/s stop speed, 4000 px/s maximum speed,
and a 100ms release window. The latest actual moving sample establishes velocity
from displacement / explicit elapsed time. A pause before release, zero elapsed
sample, invalid options or sub-threshold velocity produces no fling. Wheel input
never synthesizes another inertial tail on top of platform wheel events.

Use a fixed monotonic time origin:

1. Before each input, call `host.synchronize_navigation_time(&mut editor, now)`.
   This updates timestamps only. It does not advance an undisplayed viewport or
   invalidate a hit already picked for the currently displayed scene
2. Dispatch input normally and apply the returned report
3. While `host.needs_navigation_frame()`, schedule the application's next render
   callback and call `host.advance_navigation(&mut editor, now)` there
4. Apply that report and request another frame only while work remains

Clock reversal reports an error and cancels work; repeated timestamps do nothing.
There is no uncontrolled timer, task, sleep, or implicit wall-clock read. The
pure `PanInertia` integrator exposes velocity and deterministic advancement for
fake-clock tests. Its integrated displacement is frame-partition independent
apart from f32 rounding, including a long frame that crosses stop time.

New down/wheel/key input, focus loss, Escape, cancellation, disabling/replacing/
uninstalling plugins, policy changes, external viewport changes or document
replacement stop a pending tail. Boundary clamping stops the tail on the next
frame when actual viewport no longer equals its expected result; no bounce or
endless scheduling occurs. A completed pan and its tail retain their current
viewport; canceling an active pointer pan retains the existing rollback contract.
Only Navigation commands are accepted from frame callbacks, both before and
after middleware; editing/capture and executable clipboard effects cannot be
smuggled through a frame.

## Validation boundary

`crates/flow-interaction/tests/drag_viewport_constraints.rs` covers fixed-world and
affine parent-local containment, impossible extents, axis/grid/edge ordering,
zoom-invariant tolerance, cancellation/Undo, stale membership and identity,
viewport limits/anchor, wheel modes, fake-clock decay/stop/partitioning, clock
reversal, interruption, limits and frame capability enforcement.

Focused headless checks establish the model contract, not real device delivery.
The native SceneView frame adapter and physical-window results are recorded
separately by the integration checkpoint. Windows/macOS, touchpad/pinch and
cross-DPR hardware input require their own acceptance evidence.

### Local checkpoint: 2026-10-02

- Final focused command: `cargo test --offline --locked -p gpui-flow-interaction
  --test drag_viewport_constraints --test picked_wheel`: 28 INT-06 + 7 picked-wheel
  tests passed
- Final affected-package strict check: `cargo clippy --offline --locked
  -p gpui-flow-interaction --all-targets -- -D warnings`: passed
- The earlier full interaction suite passed 334 tests before the final seven
  INT-06 regressions and the corresponding router/clock/overflow hardening. That
  is historical coverage, not a fresh full-suite claim for the frozen final files
- Source formatting and whitespace checks passed for the owned files
- No native input, screenshot, cross-platform or physical-DPR acceptance was
  performed by this worker. Native integration is a separate checkpoint

Exact frozen source hashes and verification scopes are in
[the validation record](validation/2026-10-02-drag-viewport-summary.json).
