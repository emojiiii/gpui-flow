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
   must include the outside PointerUp, release `pan` capture and retain object
   geometry/selection. Release Space; it must be false despite prevention.
   Repeat a pan and press Escape to confirm capture/edit cleanup.
5. In view 1 press **Clear**, **Redirect next A → B**, then **Focus A**.
   The Focus listener queues a transfer to B. The final owner must be B and the
   original host Focus(A) result false. Type `r`: the KeyDown listener queues a
   redirect to the other card; subsequent keys go there. Type `d`: a request
   prepared for an already-destroyed generation must produce an error and must
   not change the live owner. The mirrored error should match; this expected
   diagnostic is not a comparison failure.
6. Focus A in view 1 and hold Space; press **Focus B** in view 2, then release
   Space. View 1's actual native FocusLost must clear its focus/Space/capture.
   View 2 receives only its own keys and begins with fresh repeat state. Press
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
- Physical native inspection is delegated to the integrator; it is not claimed
  by these automated results

The broad earlier Studio all-target check also passed before the independently
in-progress resource probe was added. Final focus validation uses the explicit
binary target above; it does not certify unrelated unfinished examples.
