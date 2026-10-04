# Custom sizing and asynchronous resources

`ElementDefinition` has optional pure `measure`, `resize_policy`, `resize` and
`resource_key` hooks. Existing definitions remain inert unless they opt in.
Measurement must agree exactly with generated local bounds and declare positive
finite minimum/maximum dimensions (at most one million). Dimensions and Uniform
resize policies return new schema-validated properties; the engine regenerates
geometry and rejects an incorrect resulting size, panic or layout override.

`SceneEngine::preview_custom_resize` preserves affine linear terms and a
normalized local anchor. `SceneView::resize_inner_tool` applies shape and anchor
translation atomically through the active inner-tool token and normal edit
policy. Multiple updates produce one Undo; Cancel restores the owned baseline.
Stale tokens, detached/locked targets and readonly views cannot acquire authority
through the callback. This is the sizing protocol for a host widget; it does not
add arbitrary Custom objects to built-in transform handles.

A nonempty `resource_key` (up to 4096 bytes) declares a required resource.
The host explicitly registers a `SceneCustomResourceProvider` per kind. Imported
keys grant no I/O permissions. The request contains the live engine ticket,
immutable properties and a cancellation token; its future returns owned bytes
or an error. Missing producers fail explicitly. A view admits at most 64 jobs,
each with at most 1 MiB returned bytes/capacity; host I/O, temporary allocations
and external retained copies remain the host's responsibility.

Custom resources participate in the existing pending/loaded/failed/cancelled
events and Ready/ViewReady/Complete receipts. Read accepted payloads with
`custom_resource_bytes`. A geometry/data change, detach, replacement or disposal
invalidates the generation and wakes its cancellation token even before another
render. The adapter drops the future and never publishes its stale result.
External threads/processes must honor the token or own a cancellation-on-drop
guard; arbitrary host work cannot be forcibly terminated. Provider replacement
explicitly invalidates all custom resources in that view. Redraw alone never
retries a failure.

Run `cargo test -p gpui-flow-engine --test custom_protocol` and
`cargo test -p gpui-flow-interaction --test custom_resize` for the core protocol.
`cargo run -p flow-studio --bin custom_protocol` exercises a real widget resize,
Undo/Cancel, asynchronous completion/failure, supersession and view release.

Validation: focused engine/interaction/async regressions cover policy and bounds rejection, anchored resizing, one Undo/Cancel, stale handles, parent layout ownership, source changes before render, explicit cancellation wakeup, dropped waiters, output limits and producer panic. The combined final run passed 1880 default tests/doctests (11 ignored), 304 UI tests, strict all-target checks and formatting. A Linux window exercised sizing/history, real payload completion, pending supersession, failure, readonly and release; all ten started producers were dropped and the final live count was zero. Both sample windows exited normally.

## Explicit Custom CPU pixels

`SceneView::set_custom_rasterizer(kind, Some(Arc<dyn CustomRasterizer>), cx)`
opts that view into a separate host-owned CPU capability. `DisplayRenderer`
continues to return `AnyElement`; registration of either capability does not
register the other, capture a native widget, or change native routing outside a
required CPU domain. The [custom_protocol example](../examples/studio/src/bin/custom_protocol.rs)
now installs both independently and uses OwnedCanvas to display resource-backed
RGBA with a transparent aperture. It is a runnable fixture, not a new native-window
validation receipt.

`CustomRasterRequest` borrows the immutable presented `DisplayObject`, its world
transform and viewport/DPR, the current accepted resource ticket/bytes when
required, cooperative cancellation, and remaining output-byte/pixel-work budgets.
Pure computation has no resource ticket. I/O remains exclusively in the explicitly
authorized `SceneCustomResourceProvider`; scene data cannot install either callback.
Return `Ok(Some(CustomRasterOutput))` for complete local premultiplied RGBA ink,
or explicitly `Ok(None)` for no ink. The host interprets local fill/stroke/data;
it must not bake in node opacity, blend, effects, masks, erasure, world transform
or ancestor clipping. Do not viewport-pre-crop the source: ancestor effects may
sample offscreen ink. Return a positive finite `local_bounds` contained in the
Custom's declared bounds (a tight subrectangle is allowed). Zero declared bounds
permit only no ink. This placement never changes canonical contours, bounds,
hit testing, layout or selection.

The toolkit-independent route is `CustomInputs` + `CustomImage` +
`rasterize_with_custom_inputs`. Capture admits Custom snapshots and charges their
contours; missing pixels now fail at materialization with
`CustomBackendRequired(id)`, rather than rejecting capture with `UnsupportedShape`.
No fallback contours become pixels. The complete domain either succeeds or fails:
there is no native overlay or partial sibling output. Explicit no ink is distinct
from missing input. Alpha/Grayscale/Clipping still start a zero-coverage mask run;
ClippingPath keeps canonical path coverage even when self ink is empty. Ordinary
no-ink paint and Pixel erasure leave preceding content unchanged.

Actual pixels use the existing affine `ImagePaint`, rounded ancestor clipping,
one-time opacity, supported internal blend modes, Alpha/Grayscale/self-painting
masks, Pixel erasers and node grayscale. Custom Path masks/erasers remain
geometry-only. Custom's own outer/inner shadows, foreground/background blur and
scale-fixed strokes remain rejected; host callbacks must not hide these effects
inside their output. Existing supported ancestor effects consume complete ink.
Scene raster and SVG export explicitly return `UnsupportedCustomExport(id)` for
visible Custom pixel producers, even if a SceneView has registered one. Canvas
subtree draw currently has no Custom input parameter and rejects such producers
with `CustomBackendRequired`; neither consumer silently captures native paint.
Geometry-only Path consumers retain their existing supported/export boundaries.

Every setter call, including the same producer Arc, revokes the previous
registration. Use `invalidate_custom_rasterizer(kind, cx)` after changing host
internal state without changing scene properties. Worker, candidate, upload and
presentation checks share that lifetime signal. Current resource tickets and
cancellation tokens, immutable bytes identity, canonical/attachment/document
revisions and viewport/DPR/window identity prevent old A/B/A work from reappearing.
The existing single worker, complete-domain upload acceptance, coherent presentation
and image retirement protocol remain in use.

RGBA length, dimensions, premultiplication, declared bounds and Vec capacity are
validated before use. `StaticImage::byte_len()` counts retained allocation including
spare capacity. Converted Image/Canvas, Text and Custom inputs share the 32 MiB
adapter input cap and the existing 128 MiB surface budget with candidate, accepted
and retiring frames. Output production writes (at least width × height), Text
production and compositor writes share the 32-million request ceiling. Custom
resource payloads retain their separate existing 64-job/1-MiB-per-payload contract;
these shared original payloads are not new pixel copies. Host/cache/GPU allocations
remain outside the pixel-memory claim. Host callbacks are trusted: budget hints and
reported work cannot forcibly bound their temporary allocations or arbitrary CPU
time. Panics become bounded diagnostic errors; cancellation is checked before and
after a callback and is available during it. A blocked callback must return before
the single worker can admit another job; there is no hard thread termination.

Focused regression entry points: composite `custom_tests`, UI
`display::composite::custom_tests`, `resources::custom::pixel_bridge_tests`, and
`custom_geometry_acceptance`. They verify pixels and lifecycle without claiming
native GPU execution, cross-platform behavior or general Custom export support.

### Two-minute optional native smoke

Run `cargo run --locked -p flow-studio --bin compositing`. This existing fixture
adds a small CPU toolbar; it registers a pure-compute producer with no I/O ticket.
Click `CPU Alpha holes`: the right domain has a clear round aperture, while the
left two domains remain intact. `CPU on / off` must withdraw the entire right
domain and report `CustomBackendRequired`, then restore it on re-registration.
`Shift + invalidate` moves the aperture using host state without a scene edit.
Click `CPU Pixel holes`: the same real alpha erases a rectangular region while its
transparent round aperture preserves a colored island. Toggle the producer and
invalidate again, then Undo and close. These are suggested steps; only a separately
recorded actual run is native evidence.

Local validation (2026-10-04): 505 composite tests, 324 UI library tests and 5
Custom acceptance tests passed; the final tightened bounds check also passed all
8 Custom pixel regressions. Targeted strict checks, formatting and both example
links passed. Linux windows showed Alpha holes, whole-domain missing-bridge
rejection/recovery, invalidation, Pixel erasure and Undo; resource Slow→Ready
cancelled its old task and Release left zero live tasks. Both windows exited 0.
These observations supplement the CPU pixel oracles; they are not cross-platform
or lossless GPU-pixel evidence.
