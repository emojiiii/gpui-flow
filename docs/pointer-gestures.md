# Pointer metadata and gestures (EVT-03)

The retained-scene router accepts mouse, touch, pen and unknown-device input through
`PointerInput`. `PointerDevice` and the existing `EventInput::Pointer` payload are
shared with scene listeners. `GeometryControl`, capture/history tools and queued
listener actions keep their existing paths.

## Host input contract

- Identity is `(device, pointer_id)`. At most 16 contacts can be held at once;
  overflow is rejected without evicting an accepted contact. Each touch retains
  its original live element handle and attachment epoch. Touch does not overwrite
  mouse hover. Layers also keep independent contact ownership.
- `button` is the changed button on down/up; `buttons: Some(mask)` is the current
  held mask (primary 1, secondary 2, auxiliary 4; other bits are preserved).
  `buttons: None` retains legacy inference. `PointerInput::left` and callers using
  `..Default::default()` remain compatible. `PointerInput::touch` starts a primary
  contact; set `buttons = Some(0)` on release. No pressure is inferred from contact.
- `pressure` is optional, finite and in `0..=1`. Invalid pressure is rejected
  without changing accepted contacts. A nonfinite position cancels and rolls back
  its matching contact, preserving the existing editor safety contract.
- `timestamp_millis` is optional monotonic host time, never wall time or an event
  counter. All input and `SceneEventRouter::advance_time` calls to one router must
  use the same clock domain. Backward timestamps are rejected; equal timestamps
  are valid. Routed `SceneEvent.timestamp_millis` preserves supplied time.
  Untimed legacy input produces immediate tap/click but cannot arm timed gestures.
- The first accepted contact in each scene router owns editor defaults/capture. Additional contacts
  receive tree events and independent gestures without stealing or committing that
  transaction. A held secondary contact is never promoted after the first ends.
  Simultaneous multi-object editing within one engine and pinch/rotate navigation
  are not implicit. Independent layers retain their independent editor ownership.
- `PointerCancelFor(identity)` cancels exactly one contact. `PointerCancel`, Escape,
  explicit focus transfer and focus loss cancel all applicable contacts and timers.
  Cancellation emits `PointerCancel` with `cancelled = true`, zero buttons and the
  last observed pointer metadata. Untimed cancellation has no invented timestamp.
  Orphan release, detach/remount, destroy and same-ID document replacement cannot
  activate an old contact. Timer advancement also retires externally reconciled
  documents and plugin-cancelled captures. Existing editor cancellation rolls back
  active edits.

## Stable event sequence

Distances are canvas-local screen pixels, independent of zoom. Primary contacts
qualify while displacement from down stays strictly below 4 px. Once the threshold
is crossed, returning to the origin does not restore eligibility. A button chord
also invalidates gesture recognition while preserving the initiating capture.

| Recognition | Notification contract |
| --- | --- |
| Tap | Matching, unprevented up on the original live target, below 4 px and held under 500 ms when timed: `PointerUp`, `Tap`, `Click` |
| Double | Two eligible, sequential taps on the same live target/device, below 4 px apart, at most 300 ms between releases: second tap adds `DoubleTap`, `DoubleClick` |
| Long press | At 500 ms, a timed, unmoved primary-button contact emits one `LongPress`; release no longer emits tap/click |

Singles are immediate; no delayed single-tap callback is held pending a double.
Doubles form disjoint pairs; an intervening tap on another target breaks a device's
sequence. Touch contact IDs may change between taps; concurrent
contacts do not form a double, including when timestamps are equal. The recent-tap
cache is bounded to 16 entries and expires with the host clock. A timer callback
uses its actual observation time, rather than claiming it ran at the deadline.
A later pointer event also checks due long presses, so a stationary release after
the deadline cannot become a tap if a host missed its timer callback.

Pointer listeners run before editor defaults. Prevented down cannot arm gestures;
prevented up rolls back the owning edit and suppresses tap/click. Gesture kinds
are separate notifications with no built-in document mutation; preventing one does
not suppress its compatibility alias. Queued actions drain through the same bounded
router queue and lifecycle validation as other tree events.

## Native GPUI boundary

On 2026-10-04 the user deferred native touch and pen input for this desktop
application. EVT-03 retains its original acceptance and partial status, with
this sub-scope recorded as `user_deferred`, not implemented or verified. Existing
mouse/keyboard and typed host input support remains in the current scope.


The Cargo lock pins GPUI to
[`26b0dccc97ebd14bbf9715741b6688132ba69491`](https://github.com/longbridge/gpui-fast/tree/26b0dccc97ebd14bbf9715741b6688132ba69491).
Its mouse down/move/up structs have no native timestamp or full held-button mask.
All three Flow adapters share one normalizer: monotonic `Instant` receipt time,
buttons tracked from observed transitions, and a reset on cancellation/focus loss.
A move reporting no held button clears interrupted mouse state.

The public `MousePressureEvent` is forwarded as genuine optional pressure updates.
GPUI documents this event as macOS-trackpad-only. Its value is normalized within
its current force-click stage, not calibrated
pen force. Ordinary mouse events have `pressure = None` until such an observation;
release/cancel clears it. This is not a claim that Linux hardware supplies force.

GPUI defines raw `TouchEvent { id, force, ... }`, but `Window::dispatch_touch_event`
consumes it in its private recognizer. Public `TouchDragEvent` has no contact ID or
force; synthesized mouse taps have no source-device discriminator. Consequently
Flow cannot recover raw multi-touch/pen identity or force through public native
mouse handlers. Such input is supported through the typed host/router contract;
real multi-touch/pen hardware delivery and validation remain platform-limited.
The existing native Text double-click affordance still uses GPUI `click_count`;
it is not used to synthesize router double-tap events.

SceneView reuses its coalesced, generation-checked frame scheduler while a long
press is pending. Inactive windows cancel input; reduced-motion preferences stop
inertia without disabling stationary long presses. `SceneInputOperation::GestureFrame`
records the exact timer observation for deterministic replay. No raw hardware
metadata, timestamps or pointer motion are fabricated to drive timers. Host adapters
can submit through `SceneView::dispatch_input` or `LayeredApp::dispatch_input` using
`gpui_flow::pointer_timestamp_millis()` for the shared native clock domain; both
entry points resolve targets through the view picker.

## References and checks

The alignment reference remains Leafer UI
[`1e11641d3ea6b0211c83447268a90b8d58feec25` PointerEvent](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/event/src/PointerEvent.ts)
and its [Interaction lifecycle](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/interaction/interaction/src/Interaction.ts),
as mapped in [the pinned source research](leafer-source-map.md). Flow's bounded
per-contact recognition and rollback policy are explicit host contracts, not a
claim of browser-event or complete Leafer gesture parity.

Focused regressions live in `flow-interaction/tests/pointer_gestures.rs`, the native
normalizer tests and the existing layered-app tests. They cover metadata, timing,
movement, concurrent contacts, identity collisions, cancellation/focus/reset,
prevented release, invalid values, chords and bounded state. Native hardware
acceptance must be reported separately from these deterministic host/model tests.

The existing Linux `focus_keyboard_trace` example accepted real single/double taps,
a 937 ms held contact with one long press and no release tap, and focus transfer
to the second view. All 941 native trace entries matched pure-router replay; two
timer observations were recorded, while the observed long press was dispatched
on a due pointer move. These checks do not validate touch/pen hardware delivery.
