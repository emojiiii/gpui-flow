# Overlapping layered App (INS-02)

`LayeredApp` owns one GPUI surface and a bottom-to-top stack of independent
`SceneView` entities. Every visible child occupies **the same full-sized rectangle**;
this is an overlapping compositor, not a row of views. Each child retains its own
`SceneEngine`, `SceneEditor`, history, viewport, event router, plugins and resources.
Duplicate public object IDs in different layers are valid and remain engine-scoped.

## Fixed contract

- Layer order is paint order. Picking visits visible, interactive layers from top
  to bottom using the **existing SceneView picker**, including its state geometry,
  image-alpha rules, ancestor clips and editor handles. There is no second picker.
- An empty hit passes through unless that layer enables `hit_background`. Usually
  only the ground layer catches empty space for marquee and pan. An object with
  `HitPolicy::None` still paints normally and allows input through.
- Every layer has the same app-local screen origin. Independent viewport pan/zoom
  map those local pixels into each scene's world. The existing engine then maps
  world coordinates into the target object's local coordinates. `layer_to_app`,
  `app_to_layer` and `map_between_layers` expose the same mapping and reject invalid
  viewports, non-finite values and overflowing results.
- The app accepts one pointer stream at a time. The Down receiver owns its Move
  and matching Up even when crossing another layer or leaving the app. A different
  pointer or release button cannot end or steal that stream. Listener-prevented
  presses also cannot leak an unpaired Up into a lower layer.
- There is one native focus handle and one active keyboard layer. Switching layers
  sends FocusLost to the old router, clearing its logical focus, held keys and
  capture. KeyDown/KeyUp affect only the active layer. Window blur/deactivation or
  hiding cancels all routers. Pointer leave clears hover while retaining capture;
  release outside the app follows SceneView's cancellation policy.
- Hide/disable, reorder, viewport replacement, detach and destroy cancel affected
  gestures. Cancel rolls back the existing editor gesture. A stale release is
  swallowed until that physical stream ends; it never targets a newly exposed or
  reattached layer. The next fresh Down starts a new stream.
- Layer IDs include the app identity and a never-reused serial. Reattaching an
  Entity allocates a fresh layer ID. Object handle generations and mount epochs
  remain enforced by the original router/host.

The app paints its background once. A child view's default surface background and
grid are not scene content and are suppressed while mounted. Explicit background
objects, normal paint, clips and per-scene compositing domains remain intact.
Whole-canvas `OwnedCanvas` is rejected on mount; it has an opaque owned backdrop
whose semantics cannot be preserved by silently dropping it. A mounted trusted
host should use checked `set_compositing_mode`, which rejects `OwnedCanvas` before mutation.
A legacy direct assignment to the public field is explicitly rejected at the next
render: the mode returns to `Domains` and a visible diagnostic explains why.

## Ownership and API

Create an Entity with `cx.new(LayeredApp::new)` and call:

```rust,ignore
let ground = app.add_layer(ground_scene, LayerOptions {
    hit_background: true,
    ..Default::default()
}, cx)?;
let sky = app.add_layer(sky_scene, LayerOptions::default(), cx)?;
app.set_layer_viewport(sky, sky_viewport, cx)?;
// `app` itself is the GPUI Render entity, with all layers overlaid.
```

`add_engine` also accepts an already-configured engine. `attach_layer` accepts an
unowned SceneView, including one previously detached. It rejects an already-owned
or destroyed view, active inner editor, pending frame edits, and `OwnedCanvas`.
An Entity must never be rendered in another host while mounted here.

`layer(id)` exposes trusted host access for ordinary editor changes, listeners,
plugins, custom renderers and resource configuration. Use `LayeredApp::focus_element`
for native/programmatic focus and `dispatch_input` for app-local input. Input hit
payloads supplied by callers are ignored and repicked. Invalid focus handles fail
before another layer loses focus. SceneView tracing remains available on each
child; `LayeredAppEvent::Input` additionally reports the receiving layer.

`set_layer_options`, `reorder_layer` and `set_layer_viewport` do not edit the scene
or create history entries. Layer visibility is independent of object visibility.
Zero is the bottom stack index; invalid IDs/indices fail before mutation.

`detach_layer` returns the original Entity. It preserves scene identity, history,
listeners, resources and viewport, while ending input/focus and invalidating old
queued action senders. Posted flow-layout callbacks are generation-cancelled.
Reattachment does not resume a previous pointer stream.

`destroy_layer` is terminal: it cancels input and scheduled layout, disposes
resource/compositing work, drops the old engine (including detached trees,
listeners and scoped disposers), and leaves any externally retained Entity empty
and unmountable. Other layers are untouched. Dropping the LayeredApp similarly
destroys all still-owned layers. Trusted direct mutation of a retained Entity is
not a security boundary; do not revive a destroyed view manually.

## Bounded differences from upstream

The source contract was read at pinned upstream
[App.ts](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/app/src/App.ts)
and [emit.ts](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/interaction/interaction/src/emit.ts).
No upstream code was copied or executed.

This is a bounded Rust layer owner, not full Leafer App compatibility. It does not
implement sibling broadcast of move/zoom/rotate/key events, a combined event tree
spanning engines, aggregated Created/Ready/ViewReady phases, cross-layer history,
cross-layer blend/backdrop filters, nested apps, arbitrary per-surface affine
transforms, or shared-camera synchronization. Each layer may use its own scene
compositing domains; it cannot sample another layer's pixels.

Native inner Text/IME panels and Image crop tools are not supported while mounted.
Do not call SceneView's standalone native-focus/inner-editor APIs on managed layers;
detach the layer first. Standard scene selection, drag, resize, pan/zoom, keyboard
commands, listener propagation and focus reuse their existing bounded contracts.
Multi-touch gestures and cross-window pointer capture are not claimed.

## Validation and native fixture

Run with the shared single-job native environment:

```sh
cargo test --locked -p gpui-flow --lib layered_app
cargo test --locked -p gpui-flow --lib layer_host
cargo check --locked -p flow-studio --bin layered_app
cargo run --locked -p flow-studio --bin layered_app
```

The routing tests use the production layer dispatcher with real independent
SceneEditors, engines, plugin hosts and event routers. They cover overlapping hits,
background pass-through/catching, world/local mappings, equal IDs, cross-layer drag
and independent Undo, focus/Space isolation, hide/reorder cancellation, stale layer
release, destroyed objects, window loss, leave, pointer/button mismatch, missing
release, hidden/noninteractive layers, prevented input, wheel routing and independent
disposers. The eligibility test rejects owned-canvas/live/destroyed mounts.
These tests do not establish native GPUI paint or physical input success.

The fixture displays blue ground and gold upper cards in the same pixels. Gold
starts with its own pan/zoom. Controls exercise hide, reorder, viewport change,
detach/reattach with a new layer ID, terminal destruction/recreation, programmatic
focus, and independent Undo. It prints JSON input/stack/viewport/selection traces.

Native acceptance checklist (record actual results separately):

1. Both layers are visible simultaneously with correct overlap; gold wins overlap,
   and empty gold surface passes through to the exposed blue card
2. Gold's viewport maps the clicked/dragged object correctly after window resize
3. Drag gold across blue and release; only gold moves; Undo gold restores it
4. Focus blue/gold and use keyboard commands; only that layer changes
5. Hide/reorder/detach during an interrupted gesture; stale release never moves or
   clicks a newly exposed layer; reattach/recreate gives fresh ownership
6. Destroy gold and keep using blue; repeat create/destroy, close the window, and
   check for stale callbacks, orphaned resources, panic or capture

Initial single-job local checks passed: 21 layer-routing tests, 2 bridge eligibility/
retained-key tests, 2 APP05 geometry/image-alpha UI regressions, native fixture
`cargo check`, and targeted strict Clippy. Later shared bridge/wheel guards require
final integrator revalidation. Native physical acceptance remains pending; no OS
result is implied by the checklist.


## Verified native window

The [capture-format erratum](screenshot-evidence-formats.md) identifies these original desktop captures as lossy JPEG bytes
saved with `.png` filenames. Original files, raw reports and hashes remain
unchanged. Screenshot measurements describe decoded captures, not lossless
renderer pixels; trace-backed results retain their separate scope.

The exact source checkpoint [`1b79a3bb`](https://github.com/emojiiii/gpui-flow/commit/1b79a3bb9d895ac9be29fff5dd58ee151dc229da)
ran on dot-cloud Linux from 19:08:24 to 19:12:30 UTC on 2026-10-02 and closed
normally with exit 0. Its full 560-blob archive matches the source tree. The
[validation summary](validation/2026-10-02-layered-app-summary.json) retains raw
trace hashes, 67 selected original payloads and all 38 saved screenshot hashes.
All 1,140 records were independently checked for sequence and layer errors.

Both overlapping layers painted together. Real click/wheel, cross-layer drag,
post-window-resize coordinate mapping, focus/Delete/Undo isolation, outside-release
rollback, hide/reorder, detach/reattach with retained independent history, Space
pan and repeated destroy/recreate passed. Fresh recreated owners were 3, 4 and 5;
blue remained usable after each gold removal. Final pointer ownership was empty.

The held-mouse lifecycle-control and wrong-button cases were not physically driven
because the desktop API's atomic drag does not expose a separate held button
across control clicks. Those remain automated routing checks. The first resize
attempt touched the underlying Chromium edge and was restored; later actual
fixture titlebar resize and coordinate checks are separately documented. No
native leak instrumentation or cross-window capture guarantee is claimed.

Final shared verification passed full UI 230, mirror fixture 3, and 55 focused
wheel/event/focus tests; after the final cursor change, 21 layer tests and 3 bridge
tests were rerun. Strict UI/interaction all-targets and the three demo bins passed.
These checks overlap the earlier feature counts. APP-05's independent CPU state
transition visual defect remains open and does not change this layered routing
receipt.
