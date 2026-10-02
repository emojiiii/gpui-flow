# Bounded nested property timelines (ANI-03)

`gpui-flow-animation::TimelineController` drives finite serial/parallel property
children from one host-supplied parent clock. It uses the existing `Schedule`,
sequence keyframe sampler, tween value validation and engine/editor batch APIs.
No UI timer, independent child controller, new interpolation library or geometry
solver is introduced.

## Frozen scope and reference

The frozen ANI-03 acceptance is: serial and parallel AnimateList-style children,
explicit offsets, parent seek/pause/reverse, ending styles, and one Undo step.
The frozen exclusions remain jump/join, physics, unlimited nesting and callback
replay for all skipped frames. Motion-path children are not supported; ANI-02's
independent motion controller remains separate. This is the finite Flow contract,
not full Leafer API/source compatibility.

The pinned [AnimateList implementation](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/animate/src/AnimateList.ts)
was read through the GitHub connector. It aggregates child ending styles and
forwards play, pause, stop and seek operations. Flow explicitly defines bounded
nesting, serial capture, ownership and atomic editor history below rather than
claiming that its transaction model comes from upstream. The task ledger is not
changed by this implementation document.

## Runtime API

```rust
use gpui_flow_animation::{
    AnimationSpec, EndingStyle, TimelineChild, TimelineController, TimelineSpec, Tween,
};
use gpui_flow_scene::Point;
use std::time::Duration;

// editor is a SceneEditor with live card and badge handles.
let token = editor.begin_playback()?;
let mut timelines = TimelineController::new(&editor.engine);
let id = timelines.animate(&editor.engine, TimelineSpec::serial(vec![
    TimelineChild::animate(card, AnimationSpec::new(200, vec![
        Tween::translation(Point::new(100.0, 0.0)),
    ])),
    TimelineChild::group(TimelineSpec::parallel(vec![
        TimelineChild::animate(card, AnimationSpec::new(300, vec![
            Tween::translation(Point::new(200.0, 0.0)),
        ])),
        TimelineChild::animate(badge, AnimationSpec::new(150, vec![
            Tween::opacity(0.0),
        ])).with_offset(50),
    ])).with_offset(25),
]).with_ending_style(EndingStyle::Hold))?;

timelines.tick_editor(&mut editor, token, Duration::from_millis(250))?;
timelines.pause(&editor.engine, id)?;
timelines.seek_editor(&mut editor, token, id, Duration::from_millis(400))?;
timelines.reverse(&editor.engine, id)?; // Changes clock travel, not the current pose.
timelines.resume(&editor.engine, id)?;
timelines.tick_editor(&mut editor, token, Duration::from_millis(100))?;
timelines.finish_editor(&mut editor, token)?; // Exactly one history transaction.
```

The target handles are runtime-only and are not serialized. A clip accepts an
existing `AnimationSpec`, `AnimationSequence` or `AnimationClip`. Each group has
its own `AnimationTiming`; child clips use their clip timing. Existing delay,
finite repeats, repeat gaps, four playback directions and rational speeds apply
at every level. All children must be finite. Nodes cannot be changed after
registration; cancel/remove and register a new specification instead.

## Clock and capture semantics

- Serial offsets are nonnegative gaps after the previous child finishes. The next
  child captures the previous child's **ending pose**. Sparse keyframes retain
  the existing cumulative-property semantics
- Parallel offsets are nonnegative positions relative to the group start. All
  parallel children capture the group's entry pose. Their complete property unions
  must be disjoint, even if their offset windows do not overlap. Use a serial
  subgroup to intentionally animate the same property more than once
- The group's cycle duration is the latest child end, including offsets and child
  delays/repeats/rates. A child duration in its parent clock is the ceiling of its
  logical duration divided by its rational rate, to whole nanoseconds. Child
  sampling uses exact integer rational arithmetic and clamps at its endpoint
- Properties hold their entry pose before the first child; they hold the preceding
  ending pose during serial gaps and after shorter parallel children finish
- A root `seek` is absolute logical root time, independent of root playback speed.
  It clamps to the finite total and reconstructs the entire nested pose, so seeking
  backward cannot leave stale properties from later children. Rewinding a completed
  record pauses it; cancelled records cannot restart
- Root `reverse` reverses future clock advancement without changing elapsed time
  or writing the scene. Completed records reactivate paused after ownership is
  checked and the current document is observed again. A later tick samples the
  original captured path, so explicitly restarting after an external post-completion
  edit can replace that edit's pose. Resume traverses toward the opposite boundary. Reverse travel
  retraces the same pose function. `AnimationTiming.direction` is a separate
  declarative traversal orientation, using existing travel-direction easing
- Root tick fractions below one nanosecond carry across ticks and pause/resume.
  Seek, explicit reverse and playback-rate changes reset that fraction. Child
  clocks are sampled absolutely, so arbitrary tick partitioning does not accumulate
  child rounding error
- `EndingStyle::Hold` retains the last sampled pose, respecting reverse/alternate
  timing. `Restore` returns that node to its captured entry pose at its logical
  completion. The serial successor starts from that restored pose. A restoring
  root also returns to its entry pose when reverse clock travel reaches zero.
  A child completed in parent coordinates keeps its ending style while the parent
  continues; seeking inside that child samples it again
- Skipped frames generate only the final sampled pose. There are no per-child,
  per-keyframe or skipped-iteration callbacks

## Ownership, lifetime and atomicity

Registration validates the whole tree, all clip keyframes/endpoints and finite
clock lengths before reserving anything. It does not mutate the scene. Per-clip
endpoint validation conservatively uses the engine snapshot; it does not promise
acceptance of otherwise-invalid custom shapes made valid only by another parallel
child's simultaneous endpoint. Every actual frame still validates its complete
combined candidate through the engine.

One `TimelineController` owns every registered active parent's complete union of
`(ElementHandle, TweenProperty)` pairs, including future children, delays and
pauses. Another parent in that controller cannot reserve an overlapping pair.
Completed/cancelled parents release ownership, and completed rewind/reverse checks
ownership again. Separate controllers, including `AnimationController` and
`MotionPathAnimation`, are independent ownership domains: hosts must not assign
the same properties to separate controllers. An observed conflicting write from
one is an external override for the other, as in the existing controller APIs.

All descendants are sampled into one virtual pose. Multiple parents in one tick
are staged together. There is exactly one `set_properties_batch(Animation, ...)`
or `apply_playback_properties(token, ...)` call, never a child commit. Any invalid
intermediate geometry/schema value rejects the complete frame: no target,
revision, notification, parent clock, status or fractional carry partially advances.
Expected cancellations discovered by synchronization are lifecycle decisions and
remain effective even if a different parent's proposed frame fails.

Destroy, detach/reattach, engine reset or an external write to an owned property
cancels the whole affected parent. Old handles never resolve to a replacement
object with the same scene ID. Engine-level cancellation retains current values.
The controller has no timer, subscription or callback to clean up on drop.

An editor uses one `PlaybackToken` for every nested child. Ticks/seeks create no
individual Undo entries. `finish_editor` commits the current aggregate pose once;
it does not jump to an endpoint. `cancel_editor` restores the pre-playback document
and preserves previous Undo/Redo, selection and viewport. External edits, destroy,
reset or Undo invalidate the token before an old frame or cancel can overwrite
newer work. A failed frame keeps the valid token and previous history intact.
A playback that returns to its baseline creates no empty Undo entry.

## Bounds

- 64 retained timeline records per controller; `remove` frees completed/cancelled records
- 256 total nodes per parent, including the root and every group/clip
- Depth 8 including the root at depth 1 and leaf clips
- 256 distinct target-property pairs per parent
- Existing per-clip limits: 64 keyframes and 32 distinct tween properties
- Maximum 24 hours for every node's cycle, total logical duration and parent-clock duration
- Infinite iterations, empty groups, zero-length clips, overflow and excessive
  depth/node/property/time budgets are rejected without registration or scene writes

## Validation

`crates/flow-animation/tests/timeline.rs` covers nested serial/parallel offsets,
exact boundaries and gaps, cumulative capture, sparse properties, repeated and
reversed clocks/easing, child rates, fractional nanosecond carry, Restore/Hold,
property conflicts, limits, lifetime invalidation and multi-parent atomic rollback.
`tests/timeline_editor.rs` covers one-token history, finish/cancel, seeks and reverse,
prior Undo/Redo, direct/editor external writes, destroy/reset, stale tokens and
failed-frame clock/history rollback. The existing animation, sequence, timing,
motion and editor playback suites also exercise the shared sampler refactor.

Local Linux validation on 2026-10-02, after the completed-reverse reactivation fix:

- `cargo test --locked -p gpui-flow-animation --lib --tests -j 1`: 135 passed,
  0 failed, 0 ignored. This includes 25 timeline and 9 timeline-editor regressions
- `cargo clippy --locked -p gpui-flow-animation --lib --tests -j 1 -- -D warnings`: passed
- `rustfmt --edition 2024 --check` on the five changed Rust source/test files: passed
- An independent read-only review found the completed-reverse stale observation
  edge case. It was fixed and verified with/without an intervening idle tick;
  the final test and Clippy run include that correction

These deterministic headless APIs require no new rendering path. No real-window
claim, GitHub Actions run, deployment or full-workspace test claim is implied.
