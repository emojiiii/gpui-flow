# Custom geometry native acceptance (CUS-01)

This closes a bounded acceptance gap for existing engine definitions and the
per-SceneView native renderer registry. It does not add a new Custom backend,
query language, factory schema contract, or Leafer-compatible API.

## Fixed source and ownership

The reference remains Leafer UI
`1e11641d3ea6b0211c83447268a90b8d58feec25`, core
`1d703303ff0759ce4973d54c87eeb8192dddc570`, and Leafer In
`3eb445bf34debb638766615282224b77108bf610` from the
[source map](leafer-source-map.md). Class/data registration is documented at
[UI.ts](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display/src/UI.ts).
These independently pinned references are not a tested combined upstream build.
No upstream code is copied. The Rust contract separates pure deterministic
geometry/hit definitions from a host's explicit GPUI renderer, as described in
[engine](engine.md) and [compositing](compositing.md).

`CustomDefinition` resolves radius into bounds plus diamond contours. Geometry,
paint input and picking consume the same committed SceneEngine. The explicit
native host renderer receives that `RenderObject`, colors a clone cyan (view 1)
or amber (view 2), and reuses the existing `DisplayElement` vector path. Removing
it exposes the same geometry's gray serialized fallback. The renderer never
rewrites the document or registers a factory implicitly.

## Automated behavioral assertions

`crates/flow-engine/tests/custom_geometry_acceptance.rs` verifies:

- Radius changes rebuild contours, transformed world bounds, ancestor bounds,
  damage and hit results in one commit, with geometry/hit/paint dirty flags
- An empty diamond corner inside its bounding rectangle is not a hit; an
  expanded-only point changes from miss to hit when radius grows
- An unchanged effective update leaves revision unchanged, an unrelated sibling
  stays unchanged, and geometry-only edits do not rebuild world transforms
- A bad contour and a panicking geometry provider each reject a multi-instance
  edit without partial input, revision, last-change, bounds or hit changes
- Same-named objects in different engines have different handles and independent
  state; a foreign handle cannot mutate the other engine
- A specialized hollow-diamond hit callback gets resolved current properties and
  local coordinates under nonuniform affine transforms, including after updates

`crates/flow-ui/tests/custom_geometry_acceptance.rs` verifies:

- Editor Undo/Redo restores the exact document, live handle, geometry, hit result
  and GPUI `RenderObject` input; viewport pan/zoom composes with object transforms
- Native renderer registration, replacement and removal are per registry
- A required group compositing domain containing Custom rejects once, with no
  native Custom or ordinary sibling fallback from inside that domain; an
  unrelated native root remains; Undo restores the original complete plan
- OwnedCanvas rejects the complete output, including roots before/after Custom,
  without mutating the document or returning a partial domain

The probe's fixture test verifies the exact small/large hit coordinates and
serialized fallback paint. Registry tests do not fabricate a native window or
claim a renderer callback ran. Actual native callback and pixels are established
only by the physical script below.

## Two-view native probe and receipts

Run the frozen linked `custom_geometry_trace` binary in the native display
session and retain stdout/stderr plus screenshots. Both panels independently own
an engine, live handles, click listeners, queued commands, history and renderer
registry. They deliberately reuse the same document IDs and kind.

The probe logs:

- `native_input`: actual adapter-routed pointer operations and events, including
  target and world/local positions, plus errors
- `click`: the live listener's requested radius and queue-acceptance result
- `renderer`: actual host callback input on each changed presentation; `call`
  counts every callback, including unchanged presentations
- `check`: both canonical states, exact bounds, independent hit probes,
  other-view isolation, renderer counts and current compositing diagnostics

A `Sample` button records state after painting settles. A command-time sample
may precede the next render; use the later Sample receipt and screenshot for
rendering rejection/recovery. State checks and callback counts are not GPU
presentation receipts. Repaints may increment counters; an unchanged input is
not logged again. The probe retains only the latest state and renderer input.

The probe disables object dragging to preserve the fixed physical hit probes.
This is a geometry/renderer acceptance window, not transform-handle acceptance.
At initial zoom 1 and pan 0, coordinates below are relative to each canvas's
inner top-left, not absolute desktop coordinates.

## Physical interaction script

1. Start fresh. View 1's diamond is cyan, view 2's amber; each has radius 40 and
   bounds origin `(64,88)`, size `(80,80)`. Both show a purple sibling rectangle
   and a green bar. Retain initial `renderer` records for both views and a
   screenshot proving the distinct explicit native bodies.
2. Click view 1's empty corner `(72,96)` and expanded-only point `(174,158)`.
   Neither should produce a Custom Click, change radius/revision, or invoke a
   queued radius update. These points distinguish a rectangle hit from actual
   diamond geometry. Selection changes do not imply a document change.
3. Click view 1 at `(104,128)`. Require actual PointerDown/PointerUp/Click on
   `custom`, a successful radius 70 queue, a document receipt with bounds
   `(64,88,140,140)`, expanded hit `custom`, corner miss, all checks true, and
   changed native renderer input plus visibly larger cyan geometry. View 2's
   serialized input, revision, bounds and hit results must remain unchanged.
4. Click view 1's expanded-only point `(174,158)`, now inside the larger diamond.
   It must produce a Custom Click, shrink to 40 and restore the old hit result.
   Press Undo: radius 70 and the large native body return. Press Redo: radius 40
   and the small body return. Press Undo then click the stable center `(104,128)`:
   the click must read restored current state and shrink to 40, rather than use a
   stale listener toggle. Confirm no false state/isolation checks.
5. Repeat a grow/shrink and Undo/Redo in view 2. View 1 must remain unchanged.
   In view 1 press Remove renderer: only its body becomes gray. Bounds, hit,
   canonical paint and history stay unchanged; view 2 remains amber. Sample,
   click the gray fallback and verify geometry/hit still update. Reinstall the
   renderer: the same current geometry becomes cyan again.
6. With both renderers installed, press Reject domain in view 1, wait for paint,
   then Sample twice, allowing a paint between samples. Its diamond and
   purple sibling must both disappear and `UnsupportedShape("custom")` must be
   reported for `domain`; green bar remains. View 2 remains intact. The host
   renderer count must stay equal across those post-rejection samples. Normal
   pre-rejection repaints may increment it, so do not compare against a
   command-time baseline from before rejection. The canonical
   opacity edit remains valid; rejection is atomic rendering, not rollback of
   document input. Bounds and hit probes still describe canonical geometry.
7. Press Undo in view 1, wait and Sample: diamond plus purple sibling return,
   error disappears, and the host renderer resumes. Redo must reject again;
   Undo must recover again. Keep evidence for both rejection and recovery.
8. Press Owned canvas in view 1 and Sample after paint. All of its scene output,
   including the green root outside the group, must disappear as one rejected
   canvas; no Custom renderer or ordinary scene root bypasses rejection. Editor
   selection outlines and diagnostic chrome remain outside scene output and are
   expected to stay visible. View 2 stays intact. Switch back
   to Domains mode and Sample: complete native output recovers without document
   edits or lost radius/history. Close the window and verify process exit.

Every claimed Custom hit requires actual physical event records, and all state
checks must have `isolated=true`, `geometry_ok=true`, `hit_ok=true`. This frozen
probe suppresses operations with no routed target events: physical no-hit
clicks therefore require an operator receipt plus unchanged-state evidence,
rather than a nonexistent native-input record. Screenshot-only labels, callback
counts without pixels, or model-only commands do not satisfy native acceptance.
Record each unperformed step as unverified.

## Boundaries

- Native host renderers are trusted host code. They must honor transforms, clips,
  opacity and paint capabilities themselves; registering one does not add a CPU
  pixel producer. Arbitrary widgets, resources and host renderer effects are not
  verified by this vector-body probe
- Visible Custom remains unsupported inside required CPU or OwnedCanvas domains;
  this increment verifies whole-domain failure, rather than adding compositing
- Specialized hit callbacks must stay within declared bounds. This probe uses
  generated fallback hit geometry; the hollow custom callback is covered only
  by the independent engine test
- Factory/schema defaults and serialization remain DAT-01, not a second counted
  CUS-01 implementation. No native touch/pen, IME, Windows/macOS, GPU partial
  damage redraw, performance or general Leafer parity is claimed
- No desktop access, remote CI, commit, push, deployment or publication is part
  of this worker's validation. The integrator owns native execution/publication

## Local validation

On 2026-10-02, using the shared cached native environment, `--locked --offline`
and one Cargo job:

- New engine acceptance: 4 passed; existing registry: 12 passed
- New public UI acceptance: 4 passed; existing UI library: 184 passed
- Native probe fixture: 1 passed
- Strict targeted Clippy passed for both new integration-test targets and the
  `custom_geometry_trace` binary (`-D warnings`)
- `cargo build -p flow-studio --bin custom_geometry_trace` linked successfully
- Direct Rust formatting checks passed for all three added Rust files

The frozen binary SHA-256 is
`1de4c8bcc9cef58774475ded5589e075098f082dd123dbe12a3c43c53493b4bd`.
The integration receipt contains the copied binary, complete source archive,
build-input manifest verified before/after final checks and linking, owned-file
manifest, and test/Clippy/link logs. No source changed during that build window.

These are focused local results, not full-workspace or cross-platform
certification. The separate physical receipt below establishes native acceptance.


## Native acceptance and independent audit, 2026-10-02

The integrator executed all eight physical steps in the cloud Linux native
window from **16:32:39 to 16:36:38 UTC**. The frozen binary above exited normally
with code **0** and empty stderr. The separately audited receipt supports
**CUS-01: bounded contract verified**; it does not add Custom compositing support.

The source checkpoint is remote
`09bc537a551da09d4cb7c407e22ee10d49d3c667`, locally represented by
`1edfa9e29efdd4d2cf6a97ff7808c2a5687522d8`, with tree
`96158cdd734dac404fedd3f0d51d745272605eea`. Every frozen build-input hash was
independently compared with that local checkpoint: zero mismatches. The
integrator verified the remote checkpoint and zero GitHub Actions runs; native
execution and these local checks are not a remote CI result.

The [compact audit summary](validation/2026-10-02-custom-geometry-summary.json)
contains source/binary/receipt/raw-log hashes, screenshot hashes, independently
checked assertions, and 45 selected exact payloads with original JSONL line
numbers. The complete trace has **81 records**: 49 state checks, 13 changed
renderer-input records, 12 native input records, six queued Custom clicks and
one initial snapshot. All state flags are true and every native input error
list is empty. The independent audit recomputed contours, bounds, resolved
radius, canonical gray paint, hit expectations and other-view invariance from
the actual payloads instead of accepting the reported booleans alone.

Observed results:

- View 1 grew from 40 to 70 at the stable center, then shrank on the newly hit
  expanded-only point. Undo/Redo restored the exact radius/bounds/hit behavior;
  clicking after Undo used the restored radius. View 2 repeated its own
  grow/shrink and Undo/Redo without changing view 1
- Removing only view 1's renderer exposed the gray fallback, retaining its
  revision and callback count. A real fallback click still grew geometry.
  Reinstalling resumed the cyan host renderer with that current geometry;
  view 2 remained amber and retained its own canonical state
- Settled checks **36/37** both report `UnsupportedShape("custom")` for
  `domain`, identical canonical state and view 1 renderer count **48**.
  The screenshot shows neither the cyan Custom nor its purple sibling, while
  the green outside root remains. Undo clears diagnostics and resumes callbacks
  at check **40**; repeated Redo/Undo rejects and recovers again by check **45**
- OwnedCanvas at check **47** rejects the complete scene, including the green
  outside root. Its editor selection outline and diagnostic chrome remain;
  these are outside scene compositing, not a Custom/native producer fallback.
  Returning to Domains at check **49** restores the full cyan/purple/green
  output, clears diagnostics and resumes the renderer, with canonical state
  unchanged from checks 45 and 47. View 2 remains intact throughout

The screenshots `initial`, `grow`, `undo`, `fallback`, `domain-rejected`,
`domain-recovered`, `owned-rejected` and `final` were inspected against these
records. They and the full raw trace remain in the integration receipt directory
`custom-geometry-validation-20261002T161415Z`; the compact summary is checked in.
The raw JSONL SHA-256 is
`9e2927341ae31d86a656f6f610c8817bd4f0f28a8c9f197260d02a2fa362efa3`.

### Physical miss evidence boundary

Before check 1 the operator physically clicked desktop `(181,518)` and
`(283,580)`, corresponding to canvas-local `(72,96)` and `(174,158)`, then Sample
at `(420,315)`. The integrator confirmed both panels stayed radius 40/revision 0
and no Custom Click occurred. Check 1 records that unchanged state and both
miss probes; the engine and fixture tests independently assert the same contour
semantics. These two physical misses are **operator-observed**, not independently
reconstructed from pointer records: this fixed probe omits no-target operations.
Successful Custom clicks do have exact native pointer/click records. No CUS
probe source or instrumentation changed between its frozen build and physical
run; unrelated later working-tree edits are outside this receipt.
