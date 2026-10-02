# Bounded continuous motion-path animation

`gpui-flow-animation::MotionPathAnimation` is one explicit runtime binding from an
attached Path/Pen to an attached target. A host advances it with `Duration` deltas
or seeks its logical timeline. Each successful frame applies a local arc distance
through the existing [path placement](path-placement.md) engine contract. There
is no render-loop hook, timer, callback, global registry or serialized follower.

## Reference and reuse

The fixed Leafer reference is
[`packages/motion-path/src/index.ts` at `3eb445bf34debb638766615282224b77108bf610`](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/motion-path/src/index.ts).
Its `Transition.register('motion')` interpolates a scalar motion distance,
`getMotionPoint` queries geometry and optional rotation/around, and `updateMotion`
writes the target pose. That source was read for this implementation. Flow uses
explicit engine handles and host clocks instead of upstream property decorators,
sibling discovery and automatic layout hooks. This is a bounded contract alignment,
not full plugin parity or a source port.

Reuse is intentional:

- `SceneEngine::measure_path` validates the existing bounded Path/Pen geometry
- `place_on_path_with_origin` delegates the same placement implementation as
  `place_on_path`, preserving its dependency, affine and post-layout validation
- Existing Kurbo-backed [motion-path queries](motion-path-queries.md) do all curve
  measurement and inversion; no second length solver or flattening algorithm
- Existing `AnimationTiming` / internal `Schedule` provide exact integer logical
  clocks, rational speed, repeats, reverse/alternate direction and delay semantics
- `TweenValue::Number::interpolate` provides the existing finite-input scalar
  interpolation and easing semantics, including exact endpoints
- Existing `PlaybackToken` sessions provide cancellation and Undo/Redo ownership

No dependency, manifest or lockfile change is required.

## Minimal editor example

```rust
use gpui_flow_animation::{MotionPathAnimation, MotionPathSpec};
use gpui_flow_editor::SceneEditor;
use gpui_flow_scene::{display::{Path, Rect}, DisplayObject, Point, Scene};
use std::time::Duration;

let mut scene = Scene::new();
scene.insert(DisplayObject::new("route", Path::from_svg("M0 0Q50 80 100 0")?), None, None)?;
scene.insert(DisplayObject::new("card", Rect::new(10., 20.)), None, None)?;
let mut editor = SceneEditor::new(scene)?;
let route = editor.engine.handle(&"route".into()).unwrap();
let card = editor.engine.handle(&"card".into()).unwrap();
let total = editor.engine.motion_total(route)?;
let mut spec = MotionPathSpec::new(1000, 0., total);
spec.anchor = Point::new(5., 10.);
spec.rotation = Some(0.); // optional world tangent alignment
let token = editor.begin_playback()?;
let mut motion = MotionPathAnimation::new_editor(&mut editor, token, card, route, spec)?;
motion.tick_editor(&mut editor, token, Duration::from_millis(500))?;
motion.pause(&editor.engine)?;
motion.seek_editor(&mut editor, token, Duration::from_millis(800))?;
motion.resume(&editor.engine)?;
motion.tick_editor(&mut editor, token, Duration::from_millis(200))?;
motion.finish_editor(&mut editor, token)?;
assert!(editor.undo()); // one playback step, not one step per frame
# Ok::<(), Box<dyn std::error::Error>>(())
```

Use `MotionPathAnimation::new`, `tick`, `seek` and `cancel` for a bare engine when
the host owns history. An editor binding is created with `new_editor` and the
original active token. It cannot switch to bare-engine writes or a later token;
an engine binding cannot borrow an editor token. Dropping a binding removes only
its runtime data. It neither restores a pose nor ends a shared editor session.

## Geometry and timing contract

`MotionPathSpec::new(duration_ms, from_distance, to_distance)` defaults to a zero
local anchor, no rotation, linear easing and one forward traversal at 1×.

- Endpoints are finite source-local arc distances, with engine endpoint clamping.
  A source scaled 3× still uses the same local distances, not world-space length.
  Negative/decreasing distances and very large finite endpoints are supported
- `anchor` is an explicit target-local point. `rotation: None` retains the world
  linear matrix; `Some(offset)` aligns world +x to the world tangent plus degrees,
  preserving world scale, shear and handedness without decomposition
- Registration writes nothing. The first active tick or seek places the anchor
  at the sampled distance, even a zero-delta tick. It does not interpolate from
  an arbitrary off-path original pose toward the path
- Initial delay holds the captured complete original affine pose, including in
  reverse playback. Seeking back into delay restores it with post-layout
  validation. The delay boundary starts at the chosen direction's first distance
- Reverse easing applies in the direction of travel. Repeat gaps hold the last
  traversal's endpoint. Finite completion uses the exact final traversal endpoint;
  infinite clips never naturally complete. The existing
  [timing contract](animation-timing.md) defines all boundaries
- `tick` deltas are host time scaled by rational speed. `seek` is absolute logical
  time including delays and repeats, with no speed scaling. Subnanosecond carry
  survives pause/resume and is reset by seek or `set_playback_rate`
- Paused ticks do not advance or place. Seeking a paused binding retains pause;
  seeking a completed binding backward pauses it until resumed. Cancellation is
  terminal. `MotionPathInfo.distance` is the last requested, unclamped distance;
  `None` denotes initial registration or the captured delay pose
- `timing_sample` describes the clock; `info` describes lifecycle. A cancelled
  binding's timing sample does not authorize it to run again

Measurements obey existing path budgets (16,384 commands and 1 MiB normalized SVG).
Each binding stores one source-shape snapshot and two ancestry chains, bounded by
scene depth (256), plus constant-size timing/pose state. It never accumulates
frames or per-iteration data. The host owns the number of bindings it creates.

## Lifetime, edits and ownership

A target may not be the path or an ancestor containing it. Both handles must be
attached and engine-local; their attachment epochs and opaque identities survive
no destroy/recreate or detach/remount substitution. String IDs are never used to
retarget an old binding.

This version makes path edits a cancellation boundary, not automatic re-following:

- Source geometry/kind changes, source or source-ancestor transform changes, or
  reparenting cancel with `SourceChanged` (or `SourceUnavailable` if identity ends)
- Target-parent chain or transform changes cancel with `TargetParentChanged`
- An observed external write to any target affine coefficient cancels with
  `TargetOverridden`, including during delay, pause and after completion
- Target/source destruction, detach/remount, kind replacement and engine document
  reset cannot be used to resume on a replacement handle
- Reparenting is a dependency change even when the numeric world pose is equal
- Bare-engine style edits that leave these geometry/transform dependencies intact
  can coexist. Editor mutations are stricter: any successful external document
  edit interrupts the whole owned playback session, as with other animations

`sync`, `info` and playback operations perform these checks. They keep current
values when cancelling and never overwrite an external edit. To follow a revised
path, finish/cancel the old session as appropriate and construct a new binding.

There is no global transform reservation shared with `AnimationController` or
other bindings. Hosts must orchestrate them. If one writes a transform another
binding captured, the latter cancels on its next observation. An identical-value
write that leaves no observable state difference is not treated as an override.
There is no implicit blending or multi-binding atomic transaction. Each binding
frame is atomic independently.

## Failure atomicity and editor history

A frame plans clock/carry/distance first and commits them only after successful
placement. Empty geometry, collapsed world tangents, singular target parents,
unrepresentable affine results, nonfinite inputs, clock overflow or a layout
that moves the source/overrides the intended pose reject without advancing the
clock or changing the document, revision, observers or history. Initial-delay
restoration has the same post-layout guard. Registration checks static inputs,
measurability and endpoint tangent viability; it cannot promise every interior
sample or future layout will succeed. A failing frame can be retried or a seek
can choose a valid position.

The placement kernel's conservative f32 roundtrip/anchor representability guards
remain unchanged. Strong cancellation in inverse affine arithmetic may reject a
frame rather than silently accept a misplaced anchor. This slice does not loosen
those guards or claim all finite affine combinations can be represented.

Derived layout may change other supported document fields when the placement
contract holds. The existing layout provider contract rejects changes to element
kind before commit, keeping the old handle and clock intact. A successful tick emits
`ChangeOrigin::Animation`; existing one-shot placement still emits `Edit`.

Editor behavior is session-scoped:

- `tick_editor` / `seek_editor` use the original active token and add no frame-level
  Undo entries. A failed frame leaves the token and prior history available
- Natural completion retains the endpoint; call `finish_editor` to end the session
  and commit at most one Undo entry for the entire playback
- `cancel_editor` restores the session's original document and prior Undo/Redo
  branch. It applies to the entire token session, including any other animations
  sharing it; the original session may have begun before this binding was created
- External edits revoke the original token before stale frame/finish/cancel calls
  can act. A newly created valid token does not revive an old binding
- Bare-engine `cancel` retains the current pose. It is deliberately separate from
  editor rollback. No-op final documents create no extra history step

## Remaining scope

No automatic path discovery/re-following, stored binding graph, percentage or
world-distance endpoints, normal offset, animated rotation offsets, path reveal,
text-on-path, shared global property reservations, multi-target atomic frame,
full nested timeline, per-loop callbacks, automatic scheduling or native UI was
added. No renderer transform support is implied for Text, Image or custom nodes.
Host locking/interaction permissions remain separate from this explicit data API.

## Validation

Linux / Rust 1.98.1, 2026-10-02, shared cached single-job Cargo environment:

- `cargo test -p gpui-flow-animation --offline`: 102 tests including the existing
  doctest pass, zero failures. The 29 new regressions are 19 in `motion_path.rs`
  and 10 in `motion_editor.rs`
- `cargo test -p gpui-flow-engine -p gpui-flow-editor --test path_placement --offline`:
  all 17 existing engine/editor placement regressions pass
- `cargo clippy -p gpui-flow-animation -p gpui-flow-engine -p gpui-flow-editor
  --all-targets --offline -- -D warnings`: passes
- Changed Rust files pass `rustfmt --edition 2024 --check`; `git diff --check` passes

New deterministic tests cover fake clocks, affine/reflected Path/Pen curves,
tangent/anchor placement, endpoint clamping, degenerate curves and disjoint
subpaths, source/target/ancestor edits, lifetime replacement, layout atomicity,
fractional-clock failure rollback, delay/reverse/repeat semantics, history
restoration, original-token ownership and Animation change origin. They also
verify an interior collapsed tangent rejects without clock commit and forbidden
layout-driven kind replacement preserves the old identity.

No native window, full workspace/cross-platform suite, GitHub Actions or deployment
is claimed for this headless slice.
