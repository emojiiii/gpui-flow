# Flow-local bridge patch

## Files

- `LICENSE-APACHE`: materialize the original upstream license symlink target
- `Cargo.toml`: expand inherited settings; add opt-in `flow-headless` feature;
  leave the original Criterion-only font benchmark source unmodified but disable
  its auto-discovery and dependency in this controlled application workspace
- `src/cosmic_text_system.rs`: keep the original font-fixture tests behind the
  separate `upstream-font-tests` feature; production font code is unchanged
- `src/gpui_wgpu.rs`, `src/fast/mod.rs`: module/export hooks and narrow style-lint
  allowances on unchanged upstream modules (constant chunk loops/cache tuple type);
  Flow bridge code remains under the workspace strict check
- `src/wgpu_renderer.rs`: one draw-entry namespace/receipt reset hook
- `src/fast/frame.rs`: Flow state, upload/pass hooks, scoped error checking and
  receipt publication after queue submission; explicit window-space Path
  projection and original vertex selection for Flow tiles
- `src/fast/layers/raster.rs`: original Path identity mapping and translated
  full-window viewport for the explicit window-space mode
- `src/fast/layers/tests.rs`: supply viewport size in the existing test host;
  original test cases and generic tile-local behavior remain unchanged
- `src/fast/flow_namespace.rs`: opaque namespace, panic-safe scope, cache reset,
  execution receipt, failure stages and unit tests
- `src/fast/flow_headless.rs`: feature-gated production FrameHost, persistent
  render target, checked RGBA readback and real-adapter test
- `src/shaders.wgsl`: evaluate Path clipping at the fragment pixel position,
  avoiding an interpolated zero-sign flip at half-pixel bounds when the target
  changes from a full window to a tile; retains the same inclusive clip rule
- `SOURCE.md`, `PATCH.md`: provenance and contracts

All other original files, including the tile cache policy, remain
byte-for-byte the official source. The Path clipping correction also applies to
ordinary direct rendering; no bit-identical claim is made against the old shader
at its unstable half-pixel boundary.

## Public contracts

`FlowLayerNamespace::new()` allocates a checked, nonzero process identity. It
never wraps/reuses an identity (exhaustion panics). Clone/Copy retains identity.
`with_flow_layer_namespace(token, closure)` is synchronous and thread-local;
nesting and panic restore the previous scope. It must enclose the actual normal
`WgpuRenderer::draw` call, not merely scene construction. No async propagation.

`with_flow_layer_namespace_receipt(token, closure) -> (R, Option<FlowExecutionReceipt>)`
captures a PlatformWindow draw without exposing its concrete renderer. A bounded
thread-local Rc/RefCell collector returns a receipt only for exactly one successful
draw attempt; zero, multiple, failed or mismatched-namespace draws return None.
Nested draws also count toward outer collectors, and panic restores collectors.
Every successful receipt has `namespace == Some(token)`.

Switching namespace, including to/from the unscoped host, clears tile textures,
per-tile globals/path targets, and cached texture/instance bindings. Consecutive
frames in the same namespace retain tiles. Normal unscoped-to-unscoped drawing
keeps upstream cache/transport behavior and does not enable counters/error waits.
Flow must fall back to an unscoped whole host frame when host/Flow layer keys
cannot be separated safely; namespace isolation is per-frame, not per-layer.

`FlowLayerNamespace::new_window_space()` additionally opts into single-scene
layer content in window-device coordinates. Original Path vertices are retained
bit-for-bit and use the full-window projection with a translated tile viewport,
preventing a different MSAA coverage decision at a tile boundary. Generic scroll
content uses `new()` and its existing tile-local projection. A window-space layer
with unsupported multipart content or inconsistent Path identity is rejected.

The native opt-in frame path captures Validation, Internal and OutOfMemory
errors separately before submit and around submit. Only an error-free queue
submission publishes `FlowExecutionReceipt`. Every draw starts with no receipt;
any returned failure clears pending counts and the published receipt. A surface
failure therefore cannot reuse the last successful Flow receipt. The receipt
means accepted submission, not GPU completion or presentation success.

- `tile_raster_passes`: actual begin_render_pass operations targeting layer
  tiles, including continued passes; excludes path-intermediate passes
- `instance_upload_calls/bytes`: actual successful write_buffer_with staging
  operations and allocated bytes, including alignment/minimum binding padding;
  excludes atlas, uniform and readback traffic
- `encoded/submitted`: true only on a published successful native receipt
- `readback_completed`: false for normal draws; true only after checked complete
  headless mapping, row depadding and BGRA-to-RGBA normalization

Counts are execution records, not dirty-tile estimates or timing. A cached
composition can have zero tile passes while still uploading its sprite instances.
No counters are claimed for the WebGL fallback/wasm transport.

`FlowHeadlessRenderer::new() -> anyhow::Result<Self>` is available with feature
`flow-headless` on native targets. It errors without an adapter/device.
`render(namespace, &Scene, Size<DevicePixels>, Rgba)` retains its target until
resize and returns `FlowRenderedFrame { namespace, rgba, receipt }`. `rgba` is
uncompressed, tightly packed top-to-bottom RGBA8. The target is cleared per frame.
Resize and failures clear tile/binding validity. Readback failures return Err,
never partial pixels or a successful receipt. Poll/mapping waits are bounded.

`adapter_info()` reports the actual wgpu adapter, including CPU/software status
via DeviceType; this backend makes no hardware-GPU or timing claim. `atlas()`
exposes the production WgpuAtlas for real glyph/image uploads. The upstream
64 MiB tile budget remains soft; Flow's separate admission/resource limits remain
its responsibility.

## Verification

CPU tests cover namespace identity/scope restoration including panic, same-token
cache retention, token-change invalidation, and counter reset after failure.
The `flow_gpu_readback_receipt_and_namespace_isolation` test requires a real wgpu
adapter (including a software adapter), and fails rather than skips if unavailable.
It checks RGBA pixels, persistent targets, warm-frame zero tile passes, real upload
bytes, key alias isolation, failed-frame receipt clearing/retry, actual queue
rejection after input-buffer destruction, and invalid-copy readback errors.
Original upstream tests keep their upstream skip behavior; they
must not be substituted for this mandatory-adapter Flow test.

The original font-fixture test module is not run in this workspace: its three
TTFs live in upstream `assets/fonts` outside the extracted crate. Enabling
`upstream-font-tests` requires supplying those original fixtures; this is not a
default/all-features test claim. No production font functionality is disabled.
