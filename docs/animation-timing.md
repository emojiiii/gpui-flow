# Animation timing: explicit, bounded, headless

Status: implemented source plus authored regression tests; compilation, test execution,
CI, native examples and runtime verification were deliberately **not run** for this
change. Static `rustfmt --check` and source review are not a behavioral validation claim.

## Scope and compatibility

`AnimationSpec`, `AnimationSequence`, their existing fields/JSON, and
`animate` / `animate_sequence` retain their old behavior: one forward traversal,
no delay, 1× speed. Existing `Easing` meanings remain exactly the same: linear,
cubic-in, cubic-out, and smoothstep-in-out. The shared `TweenValue::interpolate`
implementation is unchanged and remains available to presentation state transitions.

Opt-in entry points are `animate_with_timing`, `animate_sequence_with_timing`, and
`animate_clip`. `AnimationClip` combines a bounded `AnimationSequence` with
`AnimationTiming`; it is serializable and composable through `From<AnimationSpec>`
and `From<AnimationSequence>`. It does not imply a nested timeline implementation.

One sequence still has 1..=64 positive-duration keyframes and a union of at most 32
properties. A controller retains at most 256 records. Registration validates all
cumulative endpoints and timing before adding a record or reserving properties.
Registration does not apply an initial pose, including reverse playback.

## Three different clocks

1. Host delta: `tick` / `tick_editor` receive an elapsed `Duration` from the host.
   There are no timers or wall-clock reads inside the controller.
2. Logical timeline: the host delta multiplied by `PlaybackRate` is added to the
   record's `AnimationInfo.elapsed`. This includes the initial delay and repeat
   gaps. `seek` / `seek_editor` take an **absolute logical timeline position**;
   speed is not applied to a seek argument.
3. Single traversal position: `timing_sample().cycle_position` is a position within
   the original forward sequence. Reverse traversal moves this position backwards.
   `AnimationInfo.duration` remains the duration of **one sequence traversal**.
   It is not the total duration of a repeated/delayed clip.

`timing_sample()` also exposes phase, zero-based iteration, reverse direction,
clamped logical elapsed time, and `total_duration: Option<Duration>`; `None` means
infinite repetition. Lifecycle status remains in `info()`: a cancelled or paused
record can still have an active-looking clock sample and must not be restarted by
looking at the timing phase alone. Before the first tick/seek, the sample describes
the requested pose, while registration has left the document untouched.

`AnimationTiming::sample(cycle, position)` and `total_duration(cycle)` provide the
same pure mapping without an engine. They reject a zero-duration cycle.

## Timing policy and endpoints

- `delay_ms` occurs once, before the first traversal. It holds the originally
  captured pose even for reverse playback. Seeking back into the delay restores
  that captured pose. At the delay boundary, reverse playback starts at the
  sequence's final pose.
- `IterationCount::Finite(n)` means **n total traversals**, not n additional repeats.
  Zero is invalid. `Infinite` never emits natural completion.
- `Forward` repeats from start to end; `Reverse` repeats from end to start.
  `Alternate` goes forward/backward; `AlternateReverse` starts backward. Two
  alternate traversals are one complete out-and-back movement.
- `repeat_delay_ms` holds the just-finished traversal endpoint. It applies between
  traversals only, with no gap after the last finite traversal. It is not added
  to every keyframe.
- Intermediate repeated intervals are half-open. Without a repeat gap, an exact
  interior boundary belongs to the next traversal's start. A forward loop can
  therefore jump back to its start at that boundary. Alternate loops stay
  continuous there. With a gap, the exact traversal end begins the gap and holds
  the completed endpoint; the exact gap end begins the next traversal.
- At the finite terminal boundary, the last traversal's exact endpoint is held
  and lifecycle becomes `Completed`. Seeking beyond the end clamps there.
  Alternate finite completion depends on the final traversal parity.
- Reverse traversal reverses segment order and endpoints, but applies each
  segment's existing easing in its direction of travel. For example, a reverse
  cubic-in segment starts slowly at its old endpoint. It is not implemented by
  merely evaluating the forward eased value at a reversed clock.
- Missing properties still hold their cumulative segment value, including in
  reverse and repeated traversal. Every repeat uses the originally captured
  keyframes; it does not capture a progressively drifting new baseline.
- Zero-duration frames remain invalid, including in timed/infinite clips. There
  is no implicit first zero-duration pose and no divide-by-zero or instantaneous
  infinite loop.

For cycle duration D, initial delay A, gap G and finite count N, total logical
length is `A + N*D + (N-1)*G`. The current iteration and local offset use integer
quotient/remainder. Skipping a billion iterations does not walk a billion loops;
only one bounded keyframe sequence is sampled for the resulting frame.

## Speed, precision and overflow

`PlaybackRate::new(numerator, denominator)` uses positive `u32` components. Both
zero values are rejected, including by deserialization. Examples: 2/1 is 2×,
1/2 is 0.5×, 3/2 is 1.5×. Negative speed is expressed with playback direction;
zero speed is expressed with pause. The rate scales delay and repeat gaps too.

Clocks remain integer nanoseconds with `u128` intermediates. A remainder smaller
than one nanosecond carries across ticks, so partitioning a constant-rate advance
has the same logical result as one combined delta. Pause/resume preserves this
carry. An explicit seek or `set_playback_rate` clears it; at most a subnanosecond
fraction is discarded, with no change to the current whole-nanosecond position.
The rate setter is valid only for playing/paused records.

Finite total lengths that cannot fit `Duration` reject registration. A huge host
delta for finite playback clamps to the terminal endpoint before conversion.
Infinite clock advancement beyond `Duration::MAX` returns an error instead of
wrapping, freezing silently, changing iteration parity, or spuriously completing.
No document updates or candidate clocks from that failed tick are committed.
Seeking an infinite clip back to a representable earlier position permits it to
continue. Infinite samples at `Duration::MAX` are representable; an additional
whole nanosecond of logical advance is not.

The iteration index is `u128` so even pure sampling with very short cycle durations
retains parity beyond the exact integer range of f32/f64. Only the selected
keyframe's bounded interpolation progress is converted to the existing f64 easing
sampler. Float document properties remain subject to their normal f32/f64 precision.

## Ownership, atomicity and editor history

Delay, pause, active traversal and repeat gaps all reserve the same union of target
properties. Completion and low-level `cancel` release ownership immediately.
Completed records may be sought again only if the properties are not currently
owned by another record; a completed rewind pauses until `resume`. Cancellation
is terminal. `remove` drops a record and its reservation without restoring values.

A tick plans every animation's clock, fractional carry and value updates on cloned
records, then commits one engine/editor property batch. Validation failure or
clock overflow leaves document state, revision and all candidate clocks unchanged.
Normal lifecycle synchronization still retires stale or externally overridden
records before planning; an unrelated frame failure does not revive them.

Attachment epochs and instance handles remain authoritative across all timing
phases. Destroy/recreate with the same document ID, detach/reattach, and reset
cannot retarget an old record. An external write to an owned property cancels
that record instead of being overwritten by a future repeat.

Editor paths still require the original active `PlaybackToken`:

- `tick_editor` / `seek_editor` do not add Undo entries per frame or repetition
- `finish_editor` keeps the current pose, releases active records and commits the
  entire playback as one Undo step when it changed the document
- `cancel_editor` restores the original playback document and its prior Undo/Redo
  branch, then retires all noncancelled controller records
- A user/external edit interrupts the playback; stale finish/cancel/seek cannot
  overwrite it or erase its separate history entry
- Low-level engine `cancel` keeps current values; it is intentionally different
  from editor transaction cancellation

## Reuse decision and upstream references

The existing Flow interpolation kernel, engine batched transactions, editor
PlaybackToken protocol, Serde, and standard-library Duration are reused. No new
runtime dependency or manifest/lock change is introduced for this timing slice.

Evaluated before implementation:

- [gpui-kit motion at the locked commit](https://github.com/longbridge/gpui-kit/blob/0067bafcf3160cb871eff2521e98a51e526da175/crates/base/src/motion.rs)
  provides useful UI motion APIs but imports GPUI application/window/value types.
  Its package is not a dependency-neutral headless timing library. Its CSS-like
  same-name easing cannot silently replace Flow's serialized smoothstep meaning.
- [tween 2.2.0 upstream manifest](https://github.com/sanbox-irl/tween/blob/d1d6d64bb29e89dc7151c279e8feabf956c8a1e8/Cargo.toml)
  declares MIT OR Apache-2.0. Pinned source was read for
  [Looper](https://github.com/sanbox-irl/tween/blob/d1d6d64bb29e89dc7151c279e8feabf956c8a1e8/src/tweener/looper.rs)
  and [Oscillator](https://github.com/sanbox-irl/tween/blob/d1d6d64bb29e89dc7151c279e8feabf956c8a1e8/src/tweener/oscillator.rs).
  They accept f32 normalized percentages and use floating remainder/truncation for
  repeated phase. The exact large-iteration parity and nanosecond contract here
  would require replacing that core. Looper also retains the previous endpoint
  at an integer repeat boundary, unlike the explicitly chosen half-open policy.
  Bringing it in only to bypass those mechanisms would not provide useful reuse.
  This is an API/source compatibility decision, not a claim that tween is generally
  unsuitable. No published package was installed or copied into the project.
- Exact Leafer reference source:
  [Animate](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/animate/src/Animate.ts),
  [AnimateList](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/animate/src/AnimateList.ts),
  and [motion-path](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/motion-path/src/index.ts)
  were read from the pinned reference checkout. This implementation covers delay,
  repetition, reverse/swing-style direction, speed, pause and seek concepts with
  explicit Flow contracts; it is not a byte-for-byte port of Leafer scheduling.

## Deliberately unimplemented

Full nested AnimateList/timeline behavior, child offsets and parent clocks,
per-keyframe delay/loop/swing, automatic duration allocation, jump/join/ending
policies, percentage seek, callback events for every skipped iteration, physics,
blending, and motion-path position/rotation are not implemented here. Motion paths
should evaluate existing Kurbo/Lyon arc-length primitives rather than add another
curve-length solver. A future nested timeline must define ownership, cancellation,
seek and one-Undo rules before being advertised as complete.

## Authored coverage; execution pending

`crates/flow-animation/tests/timing.rs` covers pure phase/boundary mapping, all four
directions, huge iteration parity, finite/clock overflow, zero inputs, default
JSON/API compatibility, reverse easing and sparse sequences, logical seek vs host
speed, nanosecond carry and explicit carry reset, delay/gap property reservations,
external overrides, destroy/remount invalidation, repeated editor seek/cancel,
finish-as-one-Undo, interrupted playback and failed-frame fractional rollback.
Existing animation, sequence and editor-playback tests remain in place. All new
and existing test execution for this change is pending an authorized validation run.
