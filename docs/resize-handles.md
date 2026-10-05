# Bounded single-selection resize handles

> 旧示例说明：本文涉及 Studio 的示例名称、源码路径、操作和验证结果均为历史内容，相关演示已移除；库 API 契约继续保留。当前工作台入口见[入门](getting-started.md)，迁移范围见[说明](studio-retirement.md)。

Status: bounded implementation with focused Linux regression and native-window
validation on 2026-10-01. Exact coverage and remaining limits are below; this
is not a full editor or cross-platform acceptance claim.

The later [Image transform slice](image-transform-tools.md) enables affine Image
resize/rotation and strengthens named single-handle capture and derived-scene
validation. The original dated validation receipt below is historical.

## Contract

SceneView now draws eight named transform handles for one eligible selected
object: top-left, top, top-right, right, bottom-right, bottom, bottom-left, left.
The outline and handles use the existing local bounds and world transform,
including negative origins, strokes and existing subtree/clip bounds semantics.
Each square is 8 screen pixels; picking uses a 10-pixel circle and chooses the
nearest center when small-object hit regions overlap. Ties use clockwise order.
Four native resize cursors follow the transformed anchor-to-handle direction.
Painting and picking share the same geometry helper.

These handles change the object's affine transform. They do not rewrite shape
width/height, font size, path points or child geometry. Existing SceneEditor
transactions, engine validation/derivation, AccessPolicy and command middleware
remain in the mutation path. No geometry, input, history or font dependency was
added.

- A corner fixes its opposite corner; a side fixes the opposite edge midpoint
- Pointer motion is measured from the actual press in the initial object-local
  coordinate space, using the initial inverse world matrix. An off-center press
  or a zero-motion move does not jump. Parent transforms and viewport zoom/pan
  are included in the coordinate conversion
- Ordinary sides change one local axis. Ordinary corners change both axes
- Shift requests uniform scaling. Uniform sides use their active-axis factor;
  the perpendicular axis scales around the opposite midpoint, keeping that
  midpoint fixed. Uniform corners project signed local displacement onto the
  initial anchor-to-corner diagonal. For signed diagonal `(u, v)`, the factor is
  `1 + (dx*u + dy*v)/(u*u + v*v)`. This supports single-axis inward drags on
  non-square objects. It deliberately differs from Leafer's geometric-mean
  corner ratio locking
- Text targets and any subtree containing Text always scale uniformly.
  Their current and proposed world matrices must be supported by the existing
  native renderer: finite, axis-aligned, positive uniform scale plus translation.
  The post-middleware candidate is checked too, so a normalization cannot make
  a previously working native descendant rotated, skewed or nonuniform
- Positive incremental factors preserve the initial orientation. Dragging across
  the anchor clamps rather than silently flipping. Existing reflected vectors and Images
  stay reflected; no new flip operation is introduced

## Exact limits

At press time, let `Lx = width * hypot(world.a, world.b)` and
`Ly = height * hypot(world.c, world.d)`. These are lengths of the transformed
local axes, not world AABB sizes or screen pixels. Rotation, ancestor nonuniform
scale and zoom therefore do not change the definition of the threshold.

Each axis factor has lower bound `max(0.05, 1 / max(L, 1))` and upper bound `100`.
Uniform scaling uses the larger of the two lower bounds. Thus:

- Initially at least one world unit: the resized axis stays at least the larger
  of one world unit and 5% of its initial length
- Initially smaller than one world unit: it can stay unchanged or grow but
  cannot shrink further during that gesture. Pressing alone never enlarges it
- The limits apply per gesture to the selected bounds, not to every descendant
- Non-finite coordinates, singular transforms, zero-area bounds or overflowing
  candidates are rejected. A rejected in-progress mutation rolls back the owned
  gesture through the existing host cleanup path

## Input, tools and history

`HitTarget::ObjectResizeHandle { id, handle }` is the new typed entry point.
`ResizeHandle`, `ResizeOptions` and `resize_transform_with_options` live in
flow-editor; `SceneResizeGeometry` and `hit_resize_handle` are platform-neutral
flow-interaction helpers. A typed handle requires that exact sole selection.
Shift does not toggle the handle's target out of selection. Space-pan wins over
resize and preserves the selection.

The existing tool registry supplies outer loaded-state availability. SceneView
shows built-in handles only for the active default selection outer tool. An
active Text content panel hides the outer decoration and owns native keyboard
input. A manually supplied resize hit also cannot steal an owned Text session.
Switching, disabling or disposing the outer controller cancels its capture.
Custom hosts must honor `outer_tool_active`; registration is still declarative,
not an arbitrary outer-widget factory.

Many moves commit as one Undo step on release; an unchanged gesture preserves
Redo. Escape, focus loss, pointer cancellation, button loss, policy changes and
plugin disable/uninstall restore the still-owned baseline. Other pointer IDs
cannot operate a capture. Prevented tree events retain mandatory cleanup.

Target-aware capture also records the engine-scoped handle, attachment epoch
and document epoch. Deletion/recreation, detach/reattach or document replacement
invalidates capture before the next default action. Cleanup never replays an old
snapshot over that new target/document. A same-document ordinary editor change
that invalidates the target seals the generic gesture's accepted state; a new
engine/document abandons its obsolete pending snapshot. Existing raw-engine
external-write reconciliation and its history boundary remain unchanged.

Typed moves use `SetResizeTransform`: middleware may normalize its matrix but
cannot retarget or replace the command. Legacy `SetTransform` is unchanged.
The legacy `resize_transform` function and `HitTarget::ObjectResize` retain
their old bottom-right formula, max-factor uniform behavior and relative limits;
the new absolute-axis floor and supported-subtree policy apply to named handles.
Legacy capture gains the same stale-target safety guard.

## Explicit exclusions

The later [shared-selection resize slice](selection-resize.md) adds a world-axis
box for multiple selections. This original slice adds no path/line control-point editor, rotation
handle, skew handle, pivot/Alt-center editing, flip UI, intrinsic-size rewrite,
custom resize callback or arbitrary tool view factory is added. The existing R
rotation shortcut is unchanged and does not expand native Text transform
support. Custom subtrees, empty/zero-area targets, singular subtree matrices and
currently unsupported native matrices have no named handles. Layout-owned
transforms can still reject edits through their existing provider contract.
Native clip/style limitations and cross-platform behavior are unchanged.

## Source alignment

Compared read-only with the pinned official Leafer plugin commit
`3eb445bf34debb638766615282224b77108bf610`:

- [EditBox: eight resize points and tool lifecycle](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/editor/src/display/EditBox.ts)
- [EditDataHelper: direction, opposite anchor, ratio, flip and size limits](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/editor/src/helper/EditDataHelper.ts)
- [EditTool: world-space scale versus intrinsic resize](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/editor/src/tool/EditTool.ts)

This is an independent bounded Rust implementation, not a source import or a
full Leafer parity claim.

## Local validation (2026-10-01)

- 26 new focused tests passed: 7 editor math and 19 interaction regressions
- The complete affected editor/interaction test sets passed, 205 tests total,
  including the new fixtures and existing Text ownership, history, router,
  keyboard, focus and state-style checks. These counts are not added together
- 98 UI library tests passed, including the existing native double-click test
  expanded to reject typed resize handles as Text activation targets
- Workspace/all-targets strict Clippy, workspace formatting and Linux native
  `resize_handles` build/link passed with locked, offline dependencies and one
  build job. Existing profiles/caches were reused; the 3 GiB disk guard stayed
  clear. No new dependency, CI run, deployment or user-computer work was needed
- Native cloud-desktop pointer input exercised all eight rectangle handles,
  eight complete Undo steps back to the exact baseline and a one-step Redo,
  Shift-side shrinking, a rotated negative-origin polygon under a nonuniform
  parent with side and uniform-corner drags, uniform Text/Image/container-side
  resizing, and nearest-handle selection on a 12-by-8 target
- Native Text double-click opened its panel and hid the outer handles. After
  explicitly focusing the input, R/Space/Delete remained input-local; Escape
  restored content. Read-only hid handles, and Space-pan started from a handle
  while preserving selection and the selected object's matrix
- Final window closure was confirmed by the native window inventory. The
  launcher did not capture a process exit status, so no exit-code claim is made

The initial focused compile found one moved-value error in a new test fixture;
it was corrected before the passing runs. Production source remained unchanged
through the passing checks and native acceptance. The window-bound background
drag API did not deliver usable GPUI input; the same CUA tool's physical desktop
pointer path was used and final frames/matrices were verified. Some immediate
screenshots showed an earlier queued frame, so acceptance used fresh final
screenshots as well as the accepted-transform trace.

Not manually validated in this slice: cancellation by Escape/focus loss during
an active resize, release outside the view, post-resize zoom/DPI combinations,
immediate keyboard focus after double-click activation, Windows/macOS, actual
IME candidate flows, and large-document performance. Relevant pure cancellation
and zoom/floor fixtures passed, but they do not replace native coverage. Unchanged
whole-workspace headless/media/document suites were not rerun.

### Reproducing the focused checks

Focused fixtures are in `flow-editor/tests/resize_handles.rs` and
`flow-interaction/tests/resize_handles.rs`. Existing text-tools, scene input,
router and keyboard regressions must remain passing. The native double-click
activation regression includes the new typed handle.

Run `cargo run --locked -p flow-studio --bin resize_handles -- --trace`.
The example includes an ordinary
rectangle, a negative-origin rotated polygon under a nonuniform parent, Text,
Image, a container with a native child, and overlapping small-object handles.
Further native acceptance should extend the covered cases with off-center
presses, in-flight Escape/focus interruption, zoom, and release outside the view.
Trace output records matrices only.
