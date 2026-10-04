# Bounded Linux GPU damage

This opt-in path retains real GPU layer tiles. It consumes engine damage and
keeps ordinary Flow painting as the fallback. It is not a CPU scene-construction
optimization, a general GPU renderer replacement, or a cross-platform claim.
Acceptance depends on the real RGBA/workload tests below, not the presence of
this API or an analytic dirty count.

## Opt in

Enable `gpui-flow/gpu-damage`, create the application with
`GpuDamageHost::application()`, and bind each SceneView to that host/window:

```rust
let host = gpui_flow::gpu_damage::GpuDamageHost::default();
// Use host.application().with_assets(gpui_kit::assets::Assets).run(...),
// then initialize/open the usual Kit window.
view.set_gpu_damage(Some(&host), window, cx)?;
// Later, remove the binding and its engine observer/surface:
view.set_gpu_damage(None, window, cx)?;
let status = host.status(window);
```

The host forwards normal platform/window operations, including native input,
IME, accessibility, clipboard and focus. Its window-composition support is
reserved for these inline GPUI slices. Native/external surfaces and unrelated
surface creation/reordering return an error. Authored paint-operation order is
preserved: a later ordinary sibling still covers an earlier Flow canvas.
LayeredApp views must detach before binding, and a GPU-bound view cannot mount
in LayeredApp until unbound.

The default application path remains ordinary full painting. Existing scene,
editor, hit-test and document-history APIs do not change.

Fallback retains ordinary Flow rendering and its existing capability/errors; it
does not supply a missing CPU text rasterizer or static-outline binding. This
host currently owns application construction and does not compose with the
separate text-rasterizer or private startup-outline application factories.
Enabling those Cargo features together verifies compilation only. Native text
can still use normal GPUI painting; GPU admission excludes text/CPU domains.

## Bounded admission and fallback

A supported slice is an opaque, rectangular, device-aligned Flow canvas with
solid native vectors, including real GPUI Paths and transparent overlapping
vector paints. The existing DisplayElement tessellator and Window::paint_path
produce the input; there is no parallel geometry implementation. Selection and
tool overlays remain outside the retained slice.

The actual submitted scene must have the expected opaque borderless backdrop,
matching bounds/clip, no atlas sprites/effect primitives, at most 4,096 paint
operations and 262,144 Path vertices. The window admits at most 32 visible tiles,
each 512×512 device pixels, across at most 32 live views. Bounds must fit the
backend's tile-coordinate range. A shared 48 MiB conservative upload estimate
charges each source's vertices and operations to every visible tile, including
padding; a large Path's tile fanout cannot spend the source budget repeatedly.
This admission estimate is separate from actual receipt byte counts.
These limits bound Flow's admitted work; they
are not a hard total VRAM bound. The upstream 64 MiB tile-cache budget remains
soft and excludes other driver/pipeline/atlas allocations.

Text, images, Canvas, Custom, CPU domains, state-style bindings, fixed-stroke
presentations, semantic focus projections, foreign GPUI scroll layers and
unsupported backdrop/geometry use the ordinary native path. A changed retained
parent can replay paint without invoking a child: actual backdrop bounds/clip
are checked again, so stale paint metadata cannot reuse an old viewport.

After state styles are cleared, GPU admission resumes only when the coherent
displayed StateFrame exactly equals the current canonical frame. No pending or
old styled snapshot is relabelled with a newer engine revision.

## Damage, identity and submission

- One weak engine observer coalesces world-space damage until submission; it
  does not allocate a per-edit history
- Contiguous revisions accumulate old/new coverage. Revision gaps, missing or
  nonfinite damage, full_paint_invalidation and engine/document/camera/backdrop
  changes force a full tile rebuild
- Insert/remove/reparent operations compare surviving siblings' relative order.
  A numeric index shift alone does not invalidate every later sibling; genuine
  reorders and both affected parents' bounds/effects still receive damage
- Damage is transformed to device space with a conservative two-pixel vector
  antialias margin. Tiles stay at fixed window-device coordinates; pan/zoom,
  clip/origin/DPR changes rebuild instead of translating cached Paths
- Each view has a checked non-reused layer key. Each window has an opaque backend
  namespace. Switching producer or returning to unscoped host drawing clears
  retained tile textures and bindings, preventing historical host-key aliasing
- Flow explicitly selects window-space Path rendering: original vertex bits and
  full-window projection are preserved while the viewport moves onto each tile.
  Ordinary upstream scroll layers retain their independent content-space mode
- A prepared frame is not committed. Only exactly one successful scoped backend
  submission acknowledges its engine revision. Zero/multiple submissions,
  mismatched namespace or failure keep the damage and invalidate reuse; the
  exact original native scene is retried once without Flow tile transformation
- Unsupported frames discard the affected Flow cache. Foreign GPUI layers force
  an unscoped whole-window native fallback; they are never merged into Flow's
  key namespace

CPU paint generation and scene copying still occur. No FPS, latency, hardware-GPU
speedup or universal memory saving follows from fewer tile passes.

## Real execution receipts and pixel oracle

`GpuDamageStatus.receipt` records actual backend operations after queue submission:

- `tile_raster_passes`: real tile-target render-pass begins, including resumed
  passes; excludes Path-intermediate passes
- `instance_upload_calls/bytes`: actual successful instance staging writes,
  including alignment/padding; excludes atlas, uniform and readback traffic
- `encoded/submitted`: accepted recording/submission, not display presentation
- `readback_completed`: false for normal native draws; true only for completed,
  checked headless mapping

The controlled headless host uses the same production pipelines, shaders, atlas,
frame recorder and persistent tile cache. It compares tightly packed RGBA bytes,
not desktop JPEGs, geometry predictions or hashes. Missing adapters fail the
mandatory oracle rather than silently passing. Its adapter report distinguishes
software/CPU adapters from physical GPUs.

The private native test captures actual Flow surface paint from two simultaneously
bound SceneViews. Persistent, direct and forced-full GPU outputs are compared
byte-for-byte across warm reuse, move/recolor/delete, transparent z-order, two
commits before a frame, pan/zoom/resize, unsupported presentation and recovery.
Native submission receipts are checked independently. CPU tests cover revision
gaps, malformed damage, unacknowledged candidates, camera/DPR admission and
fallback guards. Physical multi-DPR, Windows/macOS and hardware timing are
separate unverified platform coverage.

The backend negative test also submits an otherwise valid command buffer after
destroying its input buffer: actual queue rejection must return an error and
publish no receipt. This is distinct from the CPU-only failure-state tests.
The native window's failed-submission retry branch has been reviewed but is not
fault-injected by the 12-stage window test; its recovery stage clears unsupported
presentation state, rather than simulating device loss.

```bash
cargo test --locked -p gpui-flow --features gpu-damage-oracle --lib gpu_damage::tests
cargo test --locked -p gpui_wgpu --features flow-headless --lib flow_namespace
cargo test --locked -p gpui_wgpu --features flow-headless --lib fast::flow_headless::tests::flow_gpu_readback_receipt_and_namespace_isolation -- --exact
# On a Linux desktop, run the ignored native regression alone:
cargo test --locked -p gpui-flow --features gpu-damage-oracle --lib gpu_damage::native_tests::native_flow_paint_persistent_gpu_rgba_and_work -- --exact --ignored --test-threads=1 --nocapture
```

On 2026-10-04, the Linux X11 fixture passed all 12 stages on Mesa llvmpipe
(software/CPU adapter), with byte-identical persistent/direct/forced-full RGBA.
Warm reuse executed 0 tile passes versus 8 forced-full; move/recolor/delete
executed 1 versus 8. Three real-backend GPU regressions, 339 UI tests, four
namespace tests and 34 affected engine tests passed, alongside strict Clippy,
format checking and the Studio entry-point type check. Two UI native tests and
two engine CPU benchmarks are ignored by their ordinary suites; the GPU window
and new-Kit IME-handler tests were run separately. OS IME candidates remain
unverified. The fixed ledger records only this bounded accepted scope.

## One dependency graph and source provenance

The workspace pins official gpui-fast `26b0dccc97ebd14bbf9715741b6688132ba69491`
and GPUI Kit `4c7f1350331562436df868c55ac33bebc4c6406c`. Official gpui-pre facade
packages re-export that same GPUI core. The newer gpui-fast upstream sync changes
LineWrapper incompatibly; it is not mixed into this graph.

Only gpui_wgpu is locally controlled, with original Apache-2.0 licensing and a
small, documented bridge. See [source](../vendor/gpui_wgpu/SOURCE.md) and
[patch inventory](../vendor/gpui_wgpu/PATCH.md). No upstream repository or Cargo
cache source is modified.

Cargo patches are root-workspace settings, not transitive library settings.
An embedding workspace must carry this repository's `[patch.crates-io]` facade
entries and `[patch."https://github.com/longbridge/gpui-fast"]` backend entry,
with the backend path adjusted to this checkout. All direct GPUI/platform
references must use the same explicit revision. Keep its own Cargo.lock and
verify one core/platform/backend graph; do not silently combine crates.io GPUI,
another Git source selector, or an unpatched native renderer.
