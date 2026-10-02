# Bounded center-pivot pointer rotation

This independent Leafer-aligned slice adds one circular rotation handle to the
existing outer selection tool. It reuses the shared resize root snapshots,
selection capture, atomic batch validation and gesture history. No persisted
simulated object, second selection framework, geometry dependency or document
format change is added.

The later [Image transform slice](image-transform-tools.md) extends eligibility
to Images and Image/vector subtrees using exact geometry-driven rendering.
Text and Custom exclusions remain; original dated receipts below are historical.

## Geometry and input

- One selected object uses its cached local geometric bounds center transformed
  to world coordinates. Multiple selected IDs use the existing world-axis union
  center, including parent/child selections with only one independent root
- Roots come from `SceneSelection::roots`; selected descendants never transform
  twice. Bounds include strokes/subtree geometry and internal clipping, exclude
  shadows/effects, and retain existing conservative ancestor-clip semantics
- The top-midpoint handle has a 28-screen-pixel stem offset, a 5-pixel painted
  radius and a 10-pixel hit radius, at every zoom. Single-object placement follows
  its transformed local top direction; multi-selection placement uses world top
- Pointer-down freezes the pivot, actual pointer angle, local matrices `M` and
  parent world matrices `P`. Off-center presses do not jump. Each move applies
  the same world rotation `D` to every root as `M' = inverse(P) * D * P * M`
- Existing vector/Image rotations, skews and positive/negative nonuniform parent scales
  are supported. Rotation preserves their scale/reflection and shared spacing;
  it never rewrites intrinsic shape dimensions
- Successive pointer angles accumulate the shortest signed interval in
  `[-180°,180°)`. Crossing the atan2 seam remains continuous and multiple laps
  retain snapping phase. Motion exceeding 180° between delivered samples is
  inherently ambiguous and takes the shorter arc; exact 180° takes the negative
  arc. A sample at the exact pivot has no angle and retains the previous sample
- Ordinary motion rotates freely. Shift rounds the accumulated gesture delta to
  the nearest configured gap, without discarding unsnapped motion. The default
  is 45°. Custom hosts can install `SceneRotatePlugin::with_snap_gap(degrees)`
  with a finite gap in `(0,360]`; invalid configuration returns `None`
- Zero delta, return to the exact press angle and full laps restore the original
  matrices exactly. No-op gestures preserve existing Redo. Many moves produce
  one Undo step at release

## Admission, atomicity and lifecycle

Any Text anywhere in a selected subtree suppresses and rejects rotation
for the entire selection, including otherwise-supported vectors. Their native
renderer currently cannot rotate. Hidden native descendants are also excluded.
Custom subtrees, locked roots/ancestors/descendants, singular/nonfinite matrices,
zero-area roots, overflowing unions/candidates and unsupported layout edits are
rejected. Resize remains available under its separate native-uniform rules.

`HitTarget::SelectionRotateHandle` addresses the complete current selection,
including a sole object, without assigning shared input/tree events to an
arbitrary primary target. `SceneCommand::SetRotationTransforms` uses the same
atomic apply path as `SetResizeTransforms`. Middleware may normalize matrices,
but may not change command kind or ordered root membership. Before/after layout
validation rejects unsupported subtrees before caches, observers or history see
any partial commit. A failed later move cancels the entire still-owned gesture.

The existing selection capture now accepts nonempty single selections as well.
It guards each root's engine-scoped identity and attachment epoch, selection
membership, engine/document identity and accepted revision. Only successful
owned batches advance the revision marker. External document changes invalidate
capture conservatively; cleanup never writes an obsolete baseline over them.

Read-only, plugin access policy, Space-pan, inner Text ownership and outer-tool
loaded state remain authoritative. Escape, focus loss, pointer cancel, button
loss, plugin disable/uninstall and outer-tool changes cancel the owned gesture.
Foreign pointers cannot operate capture; pointer leave alone retains it. The
existing eight resize handles and legacy resize/keyboard APIs remain unchanged.

## Pinned Leafer source and intentional differences

Compared read-only with official `leafer-in` revision
`3eb445bf34debb638766615282224b77108bf610`:

- [TransformTool.onRotate / rotateOf](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/editor/src/tool/TransformTool.ts): world pivot and shared change matrix
- [EditDataHelper.getRotateData](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/editor/src/helper/EditDataHelper.ts): center/opposite-anchor rotation geometry
- [EditBox](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/editor/src/display/EditBox.ts): rotation points and outer-tool lifecycle
- [config](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/editor/src/config.ts): default `rotateGap: 45`

This bounded UI uses one top-center handle, free rotation and Shift delta
snapping. Leafer has configurable gap attraction and diagonal rotation/pivot
behavior; those semantics are not claimed identical. No corner rotation zones,
Ctrl/Meta resize-to-rotate switching, Alt/custom pivot editing, skew controls,
intrinsic resizing, native Text rotation, custom tool widget factory or
full Leafer API compatibility is implied. Multi-selection retains its existing
world-axis box after updates. New enum variants affect exhaustive downstream
Rust matches; persisted scene data is unchanged.

## Validation

Focused fixtures: `crates/flow-interaction/tests/rotation_handles.rs`.
Run `cargo test --locked -p gpui-flow-interaction --test rotation_handles`.
The existing `selection_resize` native example now includes rotation, single and
multi-vector selections, selected vector ancestors/children, native suppression,
read-only, zoom and matrix-only `--trace` output.

Local Linux validation on 2026-10-01:

- 19 focused rotation fixtures passed; the final affected editor/interaction
  aggregate passed 242 tests, including those fixtures and all existing shared
  resize, eight-handle, Text-tool, keyboard/router/focus/history regressions
- 98 UI library tests passed with the final UI source, including rejection of
  the new hit target as a native Text double-click. A later positive-area guard
  tightened post-layout rotation admission; the final affected aggregate,
  strict workspace/all-targets Clippy and native build/link passed after it
- Workspace formatting passed. Locked offline Rust 1.98.1, the existing cache,
  one-job/debug0 profile and monitored 3 GiB disk floor were used. No dependency
  was added; no GitHub Actions, deployment or user-computer work was performed
- The real cloud-desktop window accepted shared rotation of two vectors under
  different parent transforms, then one Undo restored both exact baselines and
  one Redo restored the complete rotation
- A single-object off-center press with Shift produced the exact 45° matrix;
  its eight local-axis resize handles remained visible around the rotated shape
- Mixed Rect/Text/Image selection retained the shared resize box but showed no
  rotation control. Read-only hid the shared transform controls while preserving
  object outlines
- Space-pan starting on the rotation control preserved selection and document
  matrices. After pan and 1.5x zoom, rotating a selected vector parent, its child
  and another root transformed each independent root once; the selected child's
  local matrix remained exactly unchanged
- Final visual frames and accepted-matrix traces were checked. The titlebar close
  removed the window and the launcher recorded exit code 0

Some immediate screenshots showed an earlier queued frame; fresh final frames
and accepted matrix traces were used for verification. The shell-only first
launch lacked the desktop display environment and was interrupted; the native
app was then launched from the desktop terminal. Its paste action was unavailable,
so documented keyboard input was used. Neither failed launch/paste attempt is
counted as native coverage.

The initial test compile found an ambiguous test-only import. A regression then
exposed and fixed direct-command admission of a singular current baseline; a
zero-area test fixture was corrected to use a valid horizontal Line. All final
focused/affected runs passed. A read-only code review also caught a discriminator
scope error and a resize-disabled hover-reset edge before passing validation.

In-flight native Escape/focus-loss/outside-release timing, actual Text inner
keyboard input and multi-lap native pointer motion were not manually exercised
in this slice; their applicable state-machine/math regressions passed. Unchanged
whole-workspace media/document suites were not rerun. These results describe the
bounded contract above, not full editor parity.
