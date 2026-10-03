# INT-06 physical-window recipe and acceptance

Fixture: `examples/studio/src/bin/drag_viewport_constraints.rs`.
The original build-only recipe is preserved below. The later physical results
and qualified source provenance are recorded in [Native acceptance](#native-acceptance-2026-10-02).

## Build and launch

```bash
source /workspace/shared/gpui-flow-native-env.sh
cargo build --offline --locked -p flow-studio --bin drag_viewport_constraints
/workspace/shared/rust-target-native/debug/drag_viewport_constraints --trace \
  2>/tmp/drag-viewport-native.jsonl
```

Use an active, visible native window with reduced-motion disabled to test inertia.
The production adapter deliberately cancels tails on an inactive/hidden window
or with reduced-motion enabled. Keep the initial 1340×790 window size; its canvas
is a fixed 1000×600 logical-pixel div, with a status sidebar. A smaller window may
clip the fixture; it does not change the bounds plugin's configured canvas size.

No runtime download, network, asset files or custom font is required. `--trace`
prints fixture JSON to stderr; without it, the sidebar still shows live state.
All transforms and viewport changes go through production commands/plugins.
Resize/rotation/line tools are disabled to isolate the translation contract.
Grid snapping and edge snapping can be toggled independently; the painted grid
itself remains visible. Mode/reset preserves toggle state and read-only state.
Mode/reset intentionally replaces the sample document and clears its history.

## Evidence format

Each JSON line has `fixture: "drag-viewport"`. Ignore any unrelated library log
lines when parsing stderr. The trace includes:

- `trace.operation`: normalized real pointer/key/wheel/focus input, or the actual
  `NavigationFrame` callback with its explicit timestamp
- `trace.navigation_time`, `pan`, `zoom`, selection, capture, editing, prevention,
  changed flags and errors: snapshots taken by the production input adapter
- `observed_at_subscription`: current scene revision/document epoch, selected
  bounds, A/B local transforms and world bounds, viewport, capability policy,
  snapping flags, pending tail and renderer diagnostics at subscriber delivery
- `observed_viewport_matches_trace`: whether that subscriber's viewport still
  matches the operation snapshot. Do not pretend subscriber geometry is an
  exact per-operation snapshot when asynchronous delivery has moved on
- `inputs`, `frames` and `moving_frames`: observed cumulative counters; recent
  operation names and pan positions are also visible in the sidebar
- Toolbar actions have explicit action names and current observed state. They
  are fixture actions, not disguised physical canvas input

## Physical checks

1. **World containment, one Undo and Redo**
   - Click World mode, Select A, then Zoom twice to reach 0.5x. The full world
     rectangle is visible at this zoom
   - Save the selected bounds/transform from the sidebar/trace
   - In one uninterrupted press, drag blue A across the world right and bottom
     edges, reverse direction, cross the edges again, then release
   - Check A's complete bounds stay inside [0,1200]×[0,800]. At the lower-right
     limit its origin is (1080,720), with size 120×80
   - One Undo restores the saved transform. One Redo restores the final transform
   - Repeat with Select A+B: their relative spacing must not change and one Undo
     restores both. Do not count viewport zoom commands as document Undo entries

2. **Independent edge and grid snapping**
   - Reset World mode at 1x; turn Grid off and Edges on
   - Move A so its right edge is within six logical screen pixels of B's left
     edge. It should align to world x=320, making A's x=200; a guide must agree
   - Turn Edges off and repeat: the position must remain unsnapped. Enable Grid
     with Edges off and verify the separate 20-world-unit grid behavior
   - Repeat at 2x: the six-pixel acceptance band is three world units. Bounds may
     override a snap, in which case a now-false guide must disappear

3. **Rotated parent-local bounds and stale-capture cleanup**
   - Switch Parent mode. The gold outline is a 600×360 parent-local rectangle
     under rotation 18° and nonuniform scale (1.05,0.9)
   - With snapping off, drag A against each gold edge. Its local translation must
     stay within x=0..480 and y=0..280; its linear transform remains unchanged
   - Drag, press Escape before release, and verify exact restoration/no Undo
   - Switch mode/reset after an interrupted gesture or active inertial tail,
     then start a fresh drag. No old capture, selection, frame or transform may
     affect the replacement document. Check `document_epoch` and capture readout

4. **Actual release tail, decay and natural stop**
   - World mode → Center pan → Zoom once (2x). This gives ample room to move
   - Ensure Inertia on, then quickly drag an empty canvas region about 40–70
     logical pixels and release. Avoid a world boundary for this natural-stop test
   - Require actual `PointerUp` followed by multiple `NavigationFrame` records
     whose pan continues changing after capture is released
   - Compare movement divided by each frame's elapsed time: speed should decay.
     After roughly a second for an ordinary flick, `pending=false` and the native
     callback counter stops increasing without new input. No scene revision or
     document transform should change during this tail
   - Repeat toward a viewport edge: clamp without bounce, then stop scheduling
   - Turn Inertia off: after release there must be no synthesized tail

5. **Interruption and read-only policy**
   - Repeat a fling, interrupt separately with a fresh down, wheel and Escape.
     The old tail must stop; the new wheel/down keeps its normal behavior
   - Repeat with Inertia toggle, mode/reset, and native focus loss. Verify no
     delayed jump when reactivating the window
   - Turn Read-only on and click Probe edit +20: require an explicit Edit denial,
     unchanged scene revision/transforms, and no stranded capture
   - In the same state, pan/flick/zoom must still work because Navigation remains
     allowed. Dragging a rectangle must not edit it
   - Turn Read-only off; a fresh drag must again commit exactly one Undo entry

Capture screenshots of both modes, bounds and guides, plus the post-release
status/trace. Report any mismatch as a failure, not as an upstream or device
limitation. Record native platform, window size and DPR separately. This fixture
does not establish pinch/multi-touch, wheel-device classification, Windows/macOS
or cross-DPR hardware acceptance.

## Build-only checkpoint: 2026-10-02

- Two fixture tests passed: both modes exercise the production constraints, and
  JSON preserves the distinct native-frame operation/timestamp
- Binary `cargo check`, strict `cargo clippy ... -- -D warnings`, and native
  `cargo build` passed with `--offline --locked`
- `ldd` reported no missing libraries
- No window was opened by the fixture author at this build-only checkpoint.
  The later independent physical run is recorded below
- Source SHA-256: `249422360e7e76b4d703f404eaf2a3717a91bd8a3aa8556b0f729e73d5df32c7`
- Linked binary SHA-256: `4dd192f525ca810a2691dca76b51dbdf95b1a45a2807a84ee1b683c384a9c6a1`

## Native acceptance: 2026-10-02

Source publication: [`93ee7f7c`](https://github.com/emojiiii/gpui-flow/commit/93ee7f7cabfb7979aeca93ec137ccf81644baeb5), verified remote tree
`a8f08d035dde97d13eecc932630e8fdda65dfb42`. The frozen Linux/X11 fixture
ran **20:12:22–20:19:26 UTC**, physically operated through the desktop and normally
closed through its titlebar, **exit 0**. The compact evidence record (historical record in Git history)
contains source, binary, audit and trace hashes; originals remain in the retained
local directory identified there. No screenshots or 16.9 MB raw trace are included
in this repository.

**9,224 valid trace records, 58 original JPEG desktop captures and 33/33 evidence assertions**
establish the exercised fixed INT-06 contract. An independent read-only audit
confirms the real release/tail/interruption witnesses. The ledger promotes
INT-06 to **bounded verified**, preserving the physical and provenance limits.
No build or native run was performed by this documentation audit.

See the capture-format erratum (historical record in Git history) for the historical
`.png` suffix mismatch. Capture-derived colors/bounds are decoded-JPEG
observations; trace and input-time geometry checks remain separate evidence.

### Fixed-contract witnesses

- **World bounds and history:** a continuous crossing/reversal drag ends at
  A=(1080,720), size120×80, L1704. One Undo L1715 and Redo L1717 restore the exact
  poses. Pair selection L3504 preserves spacing (240,50) through 1,784 observed
  snapshots; one Undo/Redo restores both. Zoom adds no document Undo item
- **Independent snapping:** at 1x, edge-only L3655 gives x200; both off L3795 gives
  x196. Grid-only x209 stays outside tolerance (L3948); x204 snaps to x200
  (L4092). At 2x, four screen pixels away snaps (L5071), eight does not (L5325).
  The initial out-of-tolerance grid trial is preserved, not relabeled as a pass
- **Parent-local containment:** under 18° rotation and scale (1.05,0.9), physical
  drags reach local left x0 (L5641), right x480 (L6275), top y≈0 (L6530), bottom
  y280 (L6889). Linear transforms remain unchanged; Undo/Redo restore positions
- **Actual inertial release:** PointerUp L6987 has capture=None, pending=true.
  L6988–7006 are **19 actual moving NavigationFrame callbacks** without input.
  Tail duration is **0.660161s**; elapsed-time speed strictly decreases from
  **1303.20 to 8.45 px/s**, then pending=false. Revision2541/object transforms
  stay unchanged. Next input ~20.3s later has the same pan and frame count
- **Limits and disabling:** boundary pan clamps without bounce; L4818–4819 stop
  nonmoving callbacks. Inertia off release L7349 schedules no tail. Raw frame
  displacement need not decrease when frame intervals vary; speed does
- **Active-tail interruption:** Escape L7069, new down L7139, wheel L7203,
  Inertia toggle L7442, document replacement L7526 and real AltTab FocusLost
  L7693 each stop a real post-release tail. Fresh replacement-document drag
  L7606 succeeds, and reactivation produces no delayed jump
- **Read-only:** Probe edit L7716 returns the expected Edit denial; rectangle
  drag L7797 changes neither revision nor transform. Pan and a real tail still
  work. Ctrl-wheel L8264 actually changes zoom1→0.865887761 while read-only;
  the earlier clamped trial L7895 is not counted as a zoom-change witness
- **Recovery/cancel:** editing re-enabled gives a fresh one-Undo drag at L8006;
  Undo L8014 and Redo L8016 restore exact poses. Separate actual PointerLeave
  L8179/PointerCancel L8180 restores the interrupted gesture; Undo L8181 is a
  no-op, and fresh drag L8238 succeeds

### Observation, input and provenance limits

There are **29 PointerCancel trace/subscription viewport mismatches** around
real toolbar viewport/document resets. Operation-time snapshots can precede the
newer subscriber state; these are disclosed, not asserted simultaneous. Native
frame speed uses operation-time timestamps/pan. Selected immediate screenshots
also show queued older render state; later settled captures are retained.

The atomic physical-drag API cannot interleave a key, toolbar action, focus change
or screenshot while the same button remains held. This run therefore does not
claim held-mouse Escape/focus-loss/reset, held-drag guide pixels, or removal of a
guide when bounds win. Model-level cancellation/order tests remain separate.
Focus-loss coverage is an **active post-release tail**. Touch/pinch, wheel-device
classification, hidden/reduced-motion policy, Windows/macOS and hardware cross-DPR
remain unverified. The session shows one screenshot pixel per logical canvas
pixel, but no independent hardware-DPR query was recorded.

The source observation archive SHA-256 is
`4cc32c3f5d889c813789b5effc030fdf4bfbf47b064b98e8284f38de1fdec771`.
All **237 archived entries** match their manifest: **13 exact recorded INT-06
source hashes, 218 pre-link snapshot matches, six mtime-only files**. The last six
are Cargo.lock, four crate READMEs and examples/studio/Cargo.toml; they have no
cryptographic proof of their content at original link time. External dependency
source/native libraries are excluded. Exact executable identity is established;
complete linked-source attribution and a reproducible rebuild are not. Later
source publication does not erase this limit.

Final model checks were **28 constraints + 7 picked-wheel tests**, strict
interaction Clippy, **2 fixture tests**, fixture check/strict Clippy/native build,
and two navigation-adapter tests. The earlier full interaction **334 pass**
predates the final seven cases and router/clock/overflow hardening; it is not a
fresh final-tree full-suite pass. UI/navigation checks overlap APP-05 evidence.
