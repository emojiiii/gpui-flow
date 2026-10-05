# Image outer resize and rotation

> 旧示例说明：本文涉及 Studio 的示例名称、源码路径、操作和验证结果均为历史内容，相关演示已移除；库 API 契约继续保留。当前工作台入口见[入门](getting-started.md)，迁移范围见[说明](studio-retirement.md)。

Status: bounded Linux tests and native gestures validated on 2026-10-02. This
extends the established bounded outer tools after the exact Image rendering
pipeline landed; it does not introduce an inner image editor or a crop tool.

## Contract

Images now participate in ordinary nonuniform local-axis single resize and
center-pivot rotation. Image/vector shared selection uses the existing world-axis
union and deduplicated independent roots. Rotated, skewed, nonuniform and
reflected Image ancestors are supported when finite and invertible. The actual
Image pixels automatically use the bounded CPU geometry domain when GPUI's
native image transform is insufficient, including rounded Contain and ancestor
masks; see [Image geometry](image-corners.md).

- Shape size, fit, source, corner radii and instance identity remain unchanged.
  These tools change affine matrices, not intrinsic image dimensions
- Shift requests uniform resize or the established 45-degree rotation snap.
  Existing opposite-anchor, minimum-axis, no-flip and matrix-budget policies stay
  in force. Shared selections retain their spacing and transform each root once
- Text descendants still require positive uniform scale plus translation for
  resize and suppress pointer rotation. Custom, locked descendants, singular
  matrices and zero-area bounds suppress the corresponding named handles
- Gesture release creates one Undo entry. Repeated moves advance the capture's
  accepted revision; Escape/focus loss/policy cancellation restore only a
  still-owned baseline. Undo and Redo preserve component geometry and handles
- Named single resize now uses the same revision/selection capture guard as
  shared resize and rotation. A changed source/shape/ancestor, selection, engine,
  document, attachment or target invalidates it before the next default action.
  Cleanup does not replay an old baseline over an external same-instance edit
- Typed single-resize writes now validate the derived scene after layout, just
  like shared writes. A provider that turns the target/subtree into unsupported
  Text, Custom, locked or singular geometry is rejected atomically
- The legacy untyped ObjectResize formula and generic SetTransform remain
  separate compatibility paths; they do not acquire the named tool's complete
  selection-revision or post-layout contract

## Source reference and reuse

The pinned Leafer
[TransformTool](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/editor/src/tool/TransformTool.ts)
uses shared world transforms, configured ratio locking/Shift and configured
rotation snapping without an Image-specific uniform-only guard. This independent
slice reuses Flow's existing capture/history, root extraction and matrix helpers;
no dependency, parallel editor framework or copied upstream implementation is
added. Flow's established uniform-corner projection and bounded no-flip behavior
remain explicit differences from Leafer. Crop, skew/pivot handles, arbitrary
inner-tool factories and the pinned stub point editor are not claimed.

## Validation (2026-10-02)

- 10 new tests: 5 named-single-resize, 3 rotation and 2 shared-resize cases.
  They cover affine/reflected parents, unchanged shape and identity, one-step
  Undo/Redo, root deduplication, Text mixed selections, cancellation, external
  source/retype changes, locked descendants and post-layout atomic rejection
- Complete interaction suite: 242 passed, zero failures/ignored. UI library:
  114 passed, zero failures/ignored. Total 356 affected tests; focused earlier
  runs overlap and are not additive. This is not a fresh whole-workspace run
- Strict interaction all-targets Clippy and example Clippy, formatting, example
  check and one-job native link passed. No dependency or system change was needed
- The first new stale-source test exposed the old named-single capture weakness.
  The full interaction run also caught accidental removal of the legacy Image
  uniform default. The capture fix was retained and legacy behavior restored
  before final passing runs; named Image handles use their new geometry policy
- Real cloud native window ran 09:15:37–09:19:33 UTC and exited 0. Verified actual
  rounded pixels during Image nonuniform side resize, 90-degree rotation,
  Shift-uniform resizing, Image/vector shared resize and 45-degree snap,
  nonuniform-parent Image resize/rotation, parent+child root deduplication,
  mixed Text uniform-only handles, viewport zoom and repeated Undo/Redo
- Read-only hid handles; a drag panned the viewport through its existing
  read-only behavior without editing object transforms. Reset restored the
  scene/viewport. Final Undo and zoom restore showed the original Image matrix
- Async CPU frames briefly displayed the explicit `Compositing...` status;
  validation waited for actual settled pixels. Screenshots remain in the native
  tool record; the process trace contains only accepted matrices and no runtime
  error lines. Window inventory confirmed closure
- In-flight native Escape/focus-loss/outside-release timing, cross-platform
  behavior and arbitrary media/crop editing were not manually accepted here.
  Cancellation, stale identity and rejection paths have headless regressions

The tested source checkpoint is tree
`f019b1ae3935360fee8dce8994a433b08aec3b20` (remote WIP
`9e1a7dde9d89463b6494bcda9a73d4073d91232a`). Its deterministic archive SHA-256 is
`1314857bf79e9f70f62f86bae42260150a51d57571c460302a9da194646c350d`;
Linux example binary SHA-256 is
`a323463679f3b2df36a6960a73a19dd384f39e1b44f9335bf10678501f70c52f`.
Only documentation changed after the passing checks and native run.

Run `cargo run -p flow-studio --bin image_transforms -- --trace` for the real
SceneView demo. Presets cover Image, Image/vector, affine-parent Image,
parent/child deduplication and mixed Text; Undo/Redo, read-only and reset reuse the
public scene commands. The optional trace prints accepted transforms only.

Later [Text transform capabilities](text-transform-tools.md) allow the supported
plain Text subset in mixed resize/rotation selections when a matching glyph
bridge is enabled. Default bridge-free Text constraints remain unchanged.
