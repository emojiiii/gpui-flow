# Explicit undoable path placement

This is a one-shot placement API over [motion-path queries](motion-path-queries.md),
not a persistent follower or animation controller. The semantic reference is the
pinned Leafer [motion-path plugin](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/motion-path/src/index.ts):
`getMotionPoint` samples distance, applies optional rotation/around adjustments,
and `updateMotion` writes a target pose. Rust makes the source, target, anchor and
rotation choice explicit and reuses the existing affine/transaction/history APIs.

## Contract

`PathPlacement` contains:

- `distance`: local arc-length units of the source Path/Pen, not world distance.
  Finite values clamp to the source endpoints, as in the query API.
- `anchor`: an explicit point in the target's local coordinates. It is mapped to
  the sampled world position. No implicit bounds-relative or named anchor lookup.
- `rotation`: `None` preserves the target's world linear transform;
  `Some(offset_degrees)` points the target's world +x axis along the sampled world
  tangent plus that offset. The entire existing world linear matrix is rotated,
  preserving its scale, shear and handedness without decomposing/rebuilding it.

`PathPlacement::new(distance)` defaults to a zero anchor and **no rotation**.
This conservative default differs from the upstream plugin's default rotation.
Large finite offsets are reduced modulo 360 before adding the tangent angle.

`SceneEngine::place_on_path(target, path, options)` performs one atomic edit.
`SceneEditor::place_on_path` shares existing Undo/Redo, grouped edit cancellation,
selection preservation and owned-session/playback history boundaries. A repeated
numerically equivalent request creates no extra revision/history entry.

## Identity, dependency and affine safety

- Both handles must be attached, live and owned by the same engine. Stale handles
  never retarget a replacement object reusing its string ID.
- A target cannot be its source path or an ancestor containing that path. No
  persistent dependency edges are installed, so this API cannot create a stored
  follower cycle. Repeated calls remain independent explicit edits.
- Source queries require a built-in Path or Pen and measurable geometry. Empty
  paths, collapsed world tangents, singular target-parent transforms, nonfinite
  options and unrepresentable affine results reject without partial changes.
- Tangent rotation also requires a nonzero target world +x direction. Translation
  alone can preserve an otherwise valid singular target matrix.
- The target parent inverse converts the intended world pose to local transform.
  A post-layout validation rejects changes that move the source path or override
  the intended target pose/anchor before any revision, observer or history commit.
- No-op/roundtrip checks permit eight scaled f32 epsilons. Linear columns use
  separate relative tolerances, so tiny nonzero axes are not mistaken for zero;
  the final world anchor is checked independently to catch error amplification.
- Only the target transform is directly changed. Shape, style, metadata, tree
  position and instance identity remain intact; normal derived layout may update
  other supported fields if it still satisfies the placement contract.

This is an explicit data API, like setting a transform directly. Host interaction
permissions/locked policy and native renderer transform restrictions remain the
host's responsibility. No new native rendering capability is implied for Text,
Image or custom elements.

## Remaining scope

There is no continuous re-following after source edits, stored binding graph,
motion clock/tween, normal offset, path reveal, text-on-path or native handle/UI.
A future controller must own source/target lifetime and property conflicts rather
than treating repeated one-shot calls as an automatic animation system.

## Validation

Fresh Linux/Rust 1.98.1 validation on 2026-10-02:

- 17 new focused tests pass: 12 engine and 5 editor tests. They cover Path/Pen,
  local anchors/world positions, full affine parents, scale/shear/reflection,
  explicit tangent offsets, very large finite offsets and very small nonzero
  axes, no-op notifications, self/ancestor constraints, invalid/stale/detached
  handles, empty/collapsed paths, singular parents, layout rejection, clamping,
  single Undo/Redo, grouped cancellation, redo preservation and owned sessions.
- Four affected headless crates: 345 tests including doctests pass, zero failures,
  one existing ignored radial-raster performance benchmark. Strict Clippy
  `--all-targets -- -D warnings`, workspace rustfmt and `git diff --check` pass.
- The [isolated headless validation method](path-data.md#validation-status)
  uses byte-identical source/test copies and checks retained dependency versions
  and checksums against the unchanged repository lockfile. No dependency added.

Full workspace/native UI/cross-platform checks were not rerun. No GitHub Actions,
deployment or merge was launched.
