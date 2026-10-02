# Focus and keyboard native acceptance

This is a bounded validation harness for the existing focus/keyboard contract,
not a new event family, Tab navigator, IME bridge or accessibility implementation.
The fixed references remain Leafer UI
`1e11641d3ea6b0211c83447268a90b8d58feec25` and Leafer In
`3eb445bf34debb638766615282224b77108bf610`; see
[focus-routing](focus-routing.md), [keyboard-routing](keyboard-routing.md) and
[the source map](leafer-source-map.md). In particular, the routed non-cancelable
Blur → Focus protocol is an explicit Rust integration contract, not a claim of
identical browser or upstream notification semantics.

## What is actually compared

Run `cargo run -p flow-studio --bin focus_keyboard_trace > focus-keyboard.jsonl`
in a native display session. The window contains two independent `SceneView`s,
each paired with its own independent `SceneEditor`, `ScenePluginHost`,
`SceneEventRouter`, live handles, queue, listener registrations and policy cells.

`SceneView::set_input_tracing(true)` enables an opt-in diagnostic event.
`SceneViewEvent::InputTrace` snapshots the **actual normalized CanvasEvent before
routing**, the resulting routed events/default report and final router state.
It also records programmatic Focus/Clear, including same-target no-op results.
A native-declined focus attempt has a distinct `FocusDeclined` operation; no
logical transfer is replayed. Invalid API handles fail before routing as usual.
Automatic logical clear after a declined queued native request is also traced.

The probe feeds that exact input into the separate pure router and compares:

- Routed event order, target/current target/phase, related target, sequence,
  normalized input, positions, data, timestamp, bubbles/cancelability and
  prevention/propagation flags
- Default handling/effects/errors and document/selection/viewport changed flags
- Logical focus by document ID (never cross-engine handle equality), accepted
  Space state, pointer capture owner/pressed state, edit session, selection,
  viewport pan and zoom

Each JSON line contains both `native` and independently computed `pure` values
and a boolean `match`. Both routers share production routing code, as expected:
this checks the native adapter against the pure contract, not two independent
implementations of the algorithm. It does not verify a platform event that never
reached the adapter, so the physical interaction script below remains required.
The probe's tests deliberately perturb focus/related-target output to prove the
comparator detects wrong results. Separate pure tests assert the expected
capture/target/bubble callbacks, related-target values, repeat and cleanup rules.

Tracing is off by default, retained per view, read-only and emitted after router
dispatch, without a reentrant mutation callback. The view keeps no trace history.
The probe retains only nine display rows and writes at most 8,192 full JSON
records; `trace_limit` requires restarting rather than accepting incomplete
coverage. Programmatic control messages are separate JSON records. Snapshot
state is after routing and before host UI effects, not a GPU-present receipt.
This probe does not exercise Fit or other host effects outside the router.

## Physical interaction script

Use the frozen linked binary and matching source manifest from the integrator.
Keep stdout/stderr and at least one screenshot. Do not replace physical keyboard
and pointer checks with synthetic calls or a screenshot of static labels.
Start with a fresh window. `A` is the left card and `B` the right card in each
panel; amber indicates the current logical focus. All controls preserve pointer
focus unless they explicitly request a focus transfer.

1. In view 1, press **Focus A**, then **Focus A** again, then **Focus B**, then
   **Clear**. Expect initial Focus(A), an empty same-target report, then
   Blur(A, related=B) before Focus(B, related=A), and Blur(B, related=None).
   Focus/Blur must be non-cancelable and have no fabricated pointer input.
2. Focus A in view 1. Type `x`, then hold `x` long enough for physical repeat and
   release. Check KeyDown/KeyUp on A, first down `repeat=false`, later downs
   `repeat=true`, and a subsequent new press returns to `repeat=false`.
   Hold Shift while typing a character to inspect native modifier preservation.
3. Focus A, press **Block next Space**, then hold Space. The first down must be
   prevented and Space must remain false on subsequent repeat. Release it.
   Space Up is deliberately prevented by a listener yet must clear Space.
   Press Space again: it must now arm navigation. Release and confirm false.
4. Hold an accepted Space press, left-drag a card, move outside its SceneView
   into the surrounding host chrome and release the mouse. The native trace
   must include `PointerCancel`, release `pan` capture, clear Space/editing and
   restore the pre-pan viewport. Existing `on_mouse_up_out` intentionally cancels
   rather than forwarding PointerUp; no Click should be emitted. An inside pan
   release instead emits PointerUp, releases capture and retains its final pan.
   Release Space; it must be false despite prevention. A separate active-pan
   Escape check may be performed, but must not be inferred from idle Escape.
5. In view 1 press **Clear**, **Redirect next A → B**, then **Focus A**.
   The Focus listener queues a transfer to B. The final owner must be B and the
   original host Focus(A) result false. Type `r`: the KeyDown listener queues a
   redirect to the other card; subsequent keys go there. Type `d`: a request
   prepared for an already-destroyed generation must produce an error and must
   not change the live owner. The mirrored error should match; this expected
   diagnostic is not a comparison failure.
6. Focus A in view 1 and hold Space; press **Focus B** in view 2, then release
   Space. View 1 must finish with focus/Space/capture cleared. The host control
   click may deliver PointerCancel before native FocusLost; record this order
   rather than attributing all held-state cleanup to FocusLost. View 2 receives
   only its own keys and begins with fresh repeat state. Press
   **Clear** in view 1 while view 2 is focused and continue typing: view 2 must
   retain native focus. Repeat the programmatic A/B/Clear sequence in view 2.
7. Focus a card, then click the real **host text input** and type `host123`.
   The losing SceneView must emit FocusLost/Blur and no longer receive these
   keys. Click **Clear** in the unrelated view, then type `more`: text input
   must still own native focus and display `host123more`. Refocus a card and
   verify its first key starts fresh.
8. While a card owns focus, switch to another native window and back. Verify
   actual FocusLost cleanup and no phantom KeyUp delivery to the old logical
   owner. Focus a card again and verify normal input recovers.

Every compared operation must have `match=true`, no `trace_limit` marker may be
present, and the relevant input/output records must exist for each claimed case.
An empty trace, all-green screenshot, or two equal summaries is insufficient.
Record any step not physically performed as unverified. Programmatic/native
focus decline requires a separate focus-disabled host configuration and is not
claimed by the ordinary two-view script.

## Automated checks

Focused regressions live in
`crates/flow-interaction/tests/focus_keyboard_acceptance.rs`; the existing
`focus.rs` and `keyboard_router.rs` retain their lifecycle/queue/budget coverage.
The probe has its own comparator/fixture tests. The precise local test, strict
Clippy, link, binary hash and native receipt are reported alongside this change;
a successful compilation is not native-window acceptance.

This increment does not broaden EVT-01 into double-click event routing, multiple
pointer devices, touch pressure, native timestamp delivery, IME or a complete
Leafer/browser event model. Existing `SceneViewEvent::Pointer` subscribers keep
receiving the same events; the opt-in trace variant is diagnostic only.

### Local checks, 2026-10-02

- Existing focus (22) + keyboard (12) + new cross-operation acceptance (4):
  38 tests passed
- Independent mirror/comparator fixture: 2 tests passed
- Native adapter/UI library: 179 tests passed
- `cargo clippy -p gpui-flow -p gpui-flow-interaction --all-targets -- -D warnings`
  and `cargo clippy -p flow-studio --bin focus_keyboard_trace -- -D warnings`:
  passed (offline, one shared Cargo job)
- `cargo build -p flow-studio --bin focus_keyboard_trace`: linked successfully
- Physical native inspection was subsequently completed as recorded below;
  automated checks alone are not its evidence

## Native receipt and acceptance, 2026-10-02

The integrator operated the actual native window from 16:07:37 to 16:13:10 UTC.
It exited normally with code 0 and empty stderr. The frozen binary SHA256 is
`f8931568055beb6601d5863f06a1c5b80b1f0da3796da89a746e0687e82ae897`.
Source publication is commit
`0d81d182407191427019c08f49fdc351837de57b`, tree
`f1b105ffb11a3d3fe61537a0b822e88c8133110f`.

The raw log contains 500 JSON lines, of which 474 are compared operations
(view 1: 432; view 2: 42). An independent audit compared the actual native and
pure payloads again, confirmed zero mismatches and no trace-limit marker, and
asserted the expected behavior below independently of equality. Raw-log SHA256:
`e86bd573389e73be96a4626abec0e105b999502526d718652da6846854a154de`.
The [compact durable evidence](validation/2026-10-02-focus-keyboard-summary.json)
contains provenance, exact operation indices, 41 complete native/pure payload
pairs and exact host-input/control records. `V1:50` means view 1 operation 50,
not a raw-log line number; the evidence includes both.

- Programmatic focus: V1:2/4/6/8 and V2:21/23/25/27 prove A/A/B/Clear,
  no events for the same owner, ordered Blur → Focus, correct related targets,
  and non-cancelable notifications without fabricated pointer input
- Keyboard: V1:21–48 contains an initial down, 26 repeated downs and release;
  V1:16 preserves Shift. Refocus at V1:347 followed by V1:348 restarts repeat
- Space: V1:50 prevents the first down; every held repeat through V1:60 remains
  unarmed. V1:61 is prevented Up with Space false. V1:62–71 is an accepted
  hold; prevented Up at V1:72 clears it
- Queued requests: V1:305 redirects Focus(A) to B and the host result is false;
  V1:306 redirects B to A from KeyDown. V1:308 rejects the destroyed-generation
  destination, records the expected stale-handle error and retains A
- Instance isolation: V1:310 arms Space, V1:311 PointerCancel clears it, V2:13
  acquires B, and V1:312 FocusLost clears the old logical focus. V2:15 has fresh
  repeat state; V1:314 Clear does not steal V2's keyboard input at V2:18
- Host text input: V1:330 records FocusLost/Blur. Physical individual key presses
  produce `host123`, then `host123more` across unrelated V2:34 Clear. Scene
  operation counts remain unchanged during each typing span and both logical
  owners remain empty. The initial unsuccessful paste-style attempt is excluded
- Native window loss: operator Alt-Tab/window switching corresponds to V1:343
  FocusLost and later inert loss notifications; V1:347/348 proves recovery
- Outside release: V1:86 starts pan capture; V1:288 still has nonzero pan and
  capture. V1:289 is PointerCancel, with capture/editing/Space cleared and pan
  restored to zero. This verifies the existing cancellation contract, not an
  outside PointerUp guarantee
- Inside release: V1:358 emits PointerUp then Click. In the later pan,
  V1:430 emits PointerUp, releases capture and retains pan approximately
  (59.0009, 25.0004). V1:431 prevents Space Up but clears Space and retains pan

**Recommendation:** the fixed EVT-01 tree-routing/default-action acceptance and
EVT-02 keyboard/programmatic-focus acceptance are met for this existing bounded
contract, combining the actual native matrix with the focused pure tests.
Existing pure tests establish capture/target/bubble, once/off, listener queues,
prevention and active terminal cleanup; this native run supplies the formerly
missing exact adapter/pure trace comparison and programmatic focus matrix.

**Explicit limits:** native Escape at V1:291/292 occurred after capture had already
been released; active-pan Escape is proven only by pure tests. Physical window
FocusLost occurred without an armed capture/Space latch, and the cross-view
control click cleared Space with PointerCancel before FocusLost. Native held-state
FocusLost cleanup therefore remains a pure-test assertion rather than a claim
from this session. Native-declined focus, IME, accessibility, Tab navigation,
touch/multiple devices, pressure, native timestamps and general double-click
routing remain outside this acceptance. No source behavior changed during QA.
