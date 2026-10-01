# Explicit Box/Frame paint order

Status, 2026-10-01: implementation passed the final local Linux aggregate and
targeted real-window smoke checks. This checkpoint includes 807 headless
tests/doctests (8 pre-existing ignored), 90 UI tests, strict workspace/docs
Clippy, executable documentation tests, format checks and the native example
build. See [validation status](#validation-status) for the exact scope and
remaining limits. Earlier checkpoints in [compositing.md](compositing.md)
remain unchanged historical results.

## Scope and defaults

`display::Box` and `display::Frame` expose `paint_order: ContainerPaintOrder`:

| Rust value | JSON value | Rendering order |
| --- | --- | --- |
| `BeforeChildren` | `before_children` | Own fills → own strokes → children |
| `StrokeAfterChildren` | `stroke_after_children` | Own fills → children → own strokes |

`BeforeChildren` is the enum default, the result of existing Box/Frame
constructors and the deserialization default when the field is omitted. A
rounded Frame, clipping, a change in opacity or a change in blend mode does not
implicitly switch paint order. The new behavior requires an explicit choice:

```rust
use gpui_flow_scene::{display, ContainerPaintOrder};

let frame = display::Frame::new(300.0, 200.0)
    .rounded(12.0)
    .with_paint_order(ContainerPaintOrder::StrokeAfterChildren);
let box_shape = display::Box::new(300.0, 200.0)
    .with_paint_order(ContainerPaintOrder::StrokeAfterChildren);
```

This changes only placement of existing paints. Fill layers retain their order,
stroke layers retain their order, and all existing solid/gradient and supported
stroke-geometry rules apply. Transparent fills and zero-width/invisible strokes
remain skipped. Empty phase ranges paint nothing. A child can cover an inner
stroke in legacy mode; the final stroke can cover that child in opt-in mode.

Box still shows descendant overflow by default. Frame still clips descendants
by default and keeps Flow's existing Style defaults. This option does not add
Leafer's white Frame fill, automatic isolation decisions, scrolling, path-input
containers, container shadows, inner shadows, filters or new mask modes. It
does not modify the separate optional layout system.

## Pinned Leafer reference

These are inspected source pins, not claims of a tested combined upstream build:
`leafer-ui` at `1e11641d3ea6b0211c83447268a90b8d58feec25` and core `leafer` at
`1d703303ff0759ce4973d54c87eeb8192dddc570`. See the broader
[source map](leafer-source-map.md#1-provenance-and-license-boundary).

- [BoxData.ts, lines 9–13](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display-module/data/src/BoxData.ts#L9-L13)
  defines `__drawAfterFill` as `__single || __clipAfterFill`. The latter also
  requires overflow other than `show`, children and the relevant overflow/clip state.
  The ordering is conditional; it is not universal to every Box or Frame.
- [Box.ts, lines 152–169](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display/src/Box.ts#L152-L169)
  draws content through the rectangle phase when that condition is true;
  otherwise it draws the rectangle and then the Group content. After content,
  it restores the container's render path for the remaining paints.
- [UIRender.ts, lines 55–85](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display-module/render/src/UIRender.ts#L55-L85)
  places content after fills and before the ordinary remaining stroke path.
  [Lines 107–113](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display-module/render/src/UIRender.ts#L107-L113)
  save, apply the content clip, draw content and restore it. Other upstream
  effect and fill-after-stroke branches are outside this bounded option.
- [Frame.ts, lines 11–27](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display/src/Frame.ts#L11-L27)
  inherits Box and declares upstream white-fill/hidden-overflow defaults. Flow
  retains its own existing defaults.
- [Picker.ts, lines 53–83](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/partner/selector/src/Picker.ts#L53-L83)
  explicitly prefers descendants when Box/Frame and a descendant collide.
  [Lines 148–165](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/partner/selector/src/Picker.ts#L148-L165)
  traverse children before the branch's own hit, with the upstream clipped-Box
  gate. Flow preserves descendant-first structural picking; it does not adopt
  all upstream through/proxy/pixel-picking behavior.

Flow implements an explicit order selected by the document. It does not infer
or reproduce Leafer's `__single`/`__clipAfterFill` state machine, and it does not
import Leafer JSON.

## One scene tree, separate render phases

`PaintPhase::{All, Fill, Stroke}` is a runtime render operation. The public
`Shape::paint_phases()` helper returns `(All, None)` for legacy containers and
other shapes, or `(Fill, Some(Stroke))` for opted-in containers. The enum itself
is not additional document state.

`Scene::paint_list()` and `Scene::paint_order()` remain structural traversals:
one item/ID per visible object, stable sibling z-order, parent before its
descendants. They are still used for identity, ancestry, selection and picking.
They are not expanded into two copies of a container. Renderers that need the
new appearance must consume its entry/exit phases, not paint every complete
object once at its structural position.

### Native SceneView

`scene_units` checks compositing boundaries before scheduling native paints:

- Legacy own paints remain `SceneUnit::Native(id)`
- An opted-in native container emits `NativePhase { id, phase: Fill }`, then
  its complete ordered child units, then `NativePhase { id, phase: Stroke }`
- A CPU boundary emits one `Domain` for its complete subtree and returns
  without also emitting native fills, strokes or descendants
- A rejected boundary stays rejected under the existing domain policy;
  phase splitting is not a partial native fallback

The final parent stroke stays within its parent's sibling position: it is
after all that container's descendants and before the next later sibling.
Nested containers apply the same rule independently.

SceneView reuses the same immutable presented RenderObject for both native
phases. It does not consume the object after Fill, strip style layers or alter
canonical state. The vector producer selects only the paints in the requested
phase and retains each original fill/stroke layer index in gradient cache keys.
A delayed Stroke does not clear a gradient diagnostic raised by Fill in the
same frame. Normal standalone `DisplayElement::new` and
`DisplayElement { object }` still paint the object's own complete appearance; they do not render
or schedule its scene children.

### CPU domains and OwnedCanvas

Each container remains one captured DomainNode and one PreparedNode. Geometry
is prepared once in the existing ordered paint vector. The partition is after
the actually emitted fills, not the count of declared style slots. Legacy mode
places the entire vector before children. Opt-in mode renders its fill prefix,
children, then its stroke suffix.

The partition executes inside the existing `Raster::node` isolation decision.
The phases do not become separate layers, separate domains or separate
applications of object opacity/blending. OwnedCanvas uses the same CPU node
path inside its complete canvas domain. Both modes preserve hidden-subtree
handling and whole-domain rejection for unavailable or unsupported producers.

## Clips, masks, opacity and blending

Both own-paint phases use only the container's inherited ancestor clip chain.
`clip_children` adds a clip for descendants; it does not cut off the container's
own centered/outside stroke. The descendant clip is released before Stroke.
An ancestor clip can still trim that stroke. Nested rounded clips keep their
existing affine and antialiasing semantics.

Preparation unions all own-paint bounds, including the final stroke, with the
clipped descendant bounds. It must not allocate/cull using fills alone or clip
own-stroke bounds to the child region. Output allocation, transformed-ink
culling and fractional-DPR crops retain the existing antialias guard and
large-world-translation handling.

Alpha masks still require a direct Group parent. A Box/Frame may be such a
Group's mask, using its complete painted silhouette, or it may be a member of
the following masked sibling run. It may also contain a Group whose children
form mask runs. It cannot gain direct mask children merely by selecting a new
paint order.

An enclosing run masks the complete container. Runs inside the container's
children finish before its final own stroke; no child run leaks onto that
stroke. Invisible/offscreen mask markers retain their zero-coverage run
semantics. Opaque versus translucent run flushing is unchanged.

Container opacity is applied once after its complete isolated contents,
including overlapping fill/child/stroke pixels. Pass-through containers and
clips do not invent isolation. Children therefore read the same actual earlier
CPU backdrop, which in opt-in mode includes parent fills but not its future
stroke. A Normal/opacity-isolated container still composites its complete
contents once. Unsupported blends against arbitrary native/host backgrounds
remain explicit errors; OwnedCanvas supplies only its existing owned backdrop.

## Picking, history and cache identity

Picking is structural and geometric. A hittable descendant remains ahead of
its parent even if the parent's final stroke visually covers it. Own stroke
outside the child clip may pick the parent when no descendant is eligible.
`hit_children`, hit policies, ancestor clips, lock/visibility filtering and
event paths keep their existing meanings. Paint phases create no duplicate hit
targets, handles, listeners or resource identities.

The order is part of Shape data. Scene edits and `SceneEditor::transaction`
preserve the existing atomic/history behavior. Engine/editor property edits
use `PropertyKey::Shape`; no separate paint-order property or tween track is
introduced. Undo/Redo restores the enum with the rest of the document while
unchanged attached objects retain their existing live identities.

Exact domain equality includes the full Shape. `DomainSource::placement_eq`
also compares it recursively, so a root or descendant paint-order change is
incompatible with stale placement. It must withdraw the old accepted frame,
cancel obsolete work under the existing request policy and await a compatible
complete result. The fact that geometric extents are unchanged does not make
the old overlap order a safe stale image. Native gradient cache keys retain the
complete original RenderObject and the original layer slot; phased paints do
not alias fill and stroke textures.

## Limits and failure behavior

Ordering alone adds no CPU intermediate surface, node, depth level or mask
run. Each prepared paint is submitted once. Existing per-paint surface work,
child-mask/intersection allocations, node/isolation depth, cancellation and
atomic output accounting still apply. No budget is doubled to accommodate a
duplicated phase.

Current composite caps remain 4,096 pixels per axis, 4,194,304 pixels per
domain, 128 MiB of explicit simultaneous surfaces/retained outputs and
32,000,000 conservative surface-write/filter-work units per request. Existing
limits on domains, nodes, mask runs, points, triangles, gradients and paint
layers are unchanged; see [compositing budgets](compositing.md#budget-invalidation-and-lifecycle).
These are explicit adapter budgets, not a whole-process or GPU-memory ceiling.
Native gradient texture budgets also remain in force.

Unsupported producers and geometry, exhausted budgets and cancelled work do
not publish a partially painted CPU frame. This feature adds no tiling,
resolution reduction, native fallback for a rejected domain, GPU readback or
frame-rate guarantee. Native and CPU rasterization are not promised to be
byte-identical at antialiased edges.

## Rust API and serialization compatibility

- Box/Frame `paint_order` is serialized inside `shape.geometry`. Omission
  decodes as `before_children`; `stroke_after_children` is the explicit opt-in
- Invalid variants, wrong types and null are rejected rather than normalized
  to the legacy mode. New serialization includes the field
- Old JSON remains readable. This is not byte-identical JSON or forward visual
  compatibility: an older reader may ignore the new field and render the old
  order. No Leafer document import is implied
- Exhaustive Rust Box/Frame literals need the new field. Existing `new`,
  `rounded` and Box `clipped` calls remain available and preserve legacy order;
  `.with_paint_order(...)` is the opt-in builder
- Adding `SceneUnit::NativePhase` requires downstream exhaustive matches to be
  updated, although `SceneUnit::Native(id)` remains available. Handle the phase
  rather than silently treating it as a complete duplicate object
- `RenderObject` and `DisplayElement { object }` literal construction remain
  unchanged. Native phase wrapping is an internal SceneView integration detail

## Validation status

The final local Linux aggregate completed on 2026-10-01 with exit code 0:

- 807 headless tests/doctests passed; 8 pre-existing ignored tests stayed ignored
- 90 UI library tests passed, including the accepted-frame/order-change regression
- Workspace/all-targets Clippy and documentation-example Clippy passed with
  `-D warnings`
- Executable documentation tests and workspace/documentation format checks passed
- The native `composited_containers` example build passed

Counts describe each final aggregate once; they are not sums of repeated focused
runs. Commands used locked/offline dependencies. Recorded minimum available
disk was 8,096,276,480 bytes. The run record is named `container-phase-final`
(`.log` and `.json`) in the local validation output; no CI result is claimed.

Scene/engine/editor fixtures cover legacy omitted JSON and constructor defaults,
enum/factory roundtrip and invalid-value rejection, phase schedules, unchanged
structural clips/picking, Shape-property mutation, Undo/Redo, stable identities
and failure atomicity. The scene/editor test files are
`crates/flow-scene/tests/container_paint_order.rs` and
`crates/flow-editor/tests/container_paint_order.rs`.

CPU/native-adapter regressions cover real overlap pixels, transparent/multiple
paint-layer partitioning without duplicate alpha, nested/mixed native-domain
order, whole-domain rejection, mask silhouettes and child mask-run completion,
single container opacity, pass-through blend backdrops, rounded/inherited clips,
fractional DPR, offscreen centerlines with visible stroke ink, output bounds,
unchanged phase-only memory/work accounting and budget rollback. Native vector
tests retain original layer/cache slots and actual clipped triangle geometry;
the gradient test preserves Fill failures through the delayed Stroke.

The controller regression
`root_and_nested_container_order_edits_hide_accepted_frames_and_reject_late_work`
checks root Frame and nested Box order-only edits at unchanged bounds and live
identity. It hides/retires the incompatible accepted image, cancels pending work,
rejects a successfully rendered obsolete completion, then accepts fresh output
and clears loading. This exercises the accepted-frame seam in addition to
recursive `DomainSource::placement_eq` checks.

Cloud Linux native verification used target-window screenshots of the freshly
built `composited_containers` example. It visibly checked both legacy and
stroke-after-children order in the native Box and CPU Frame, Undo, opacity-driven
native ↔ CPU promotion, Frame clipping and alpha, Multiply ↔ Screen, pan/zoom
and repeated toggles. Normal close completed with process exit code 0 and an
empty native log. These observations are smoke evidence, not screenshot pixel
goldens or performance measurements.

Reopening was not run in this batch; close-during-worker behavior remains
unverified. CI, macOS, Windows, live multi-monitor/DPR changes, live GPU-failure
injection, a full old-example interaction sweep and the pre-existing ignored
tests were not exercised. Automated fractional-DPR and controller-failure tests
do not substitute for those live checks. The unsupported features, resource
boundaries and bounded-memory guarantees above are unchanged.

See the earlier [fixed-container checkpoint](compositing.md#container-validation-2026-10-01)
and [outer-shadow checkpoint](compositing.md#outer-shadow-validation-2026-10-01)
for already recorded work and its explicit limits.
