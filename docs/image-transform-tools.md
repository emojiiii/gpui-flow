# Image outer resize and rotation

Status: source checkpoint, final validation and native gestures pending. This
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

## Validation checkpoint

The initial 64 focused resize, rotation and shared-resize tests passed after a
new stale-source regression exposed the older single-resize capture weakness.
That fix is included. Two additional derived-scene/locked-descendant regressions,
the complete interaction suite and the new native example are being checked.
This section is updated with final exact-source evidence before completion.

Run `cargo run -p flow-studio --bin image_transforms -- --trace` for the real
SceneView demo. Presets cover Image, Image/vector, affine-parent Image,
parent/child deduplication and mixed Text; Undo/Redo, read-only and reset reuse the
public scene commands. The optional trace prints accepted transforms only.
