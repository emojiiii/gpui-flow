# Flow-local bridge patch

## Files

- `LICENSE-APACHE` contains the original published license text.
- `Cargo.toml` uses the published package configuration and adds the
  `flow-headless` and `upstream-font-tests` features. The Criterion font
  benchmark source is retained with auto-discovery disabled.
- `src/cosmic_text_system.rs` and `src/fast/synthetic_italic.rs` gate tests
  requiring upstream font fixtures behind `upstream-font-tests`.
- `src/gpui_wgpu.rs` and `src/fast/mod.rs` export the bridge and retain scoped
  Clippy allowances for upstream loops and cache types.
- `src/fast/dynamic_textures.rs` retains a scoped Clippy allowance for the
  upstream texture-upload signature.
- `src/wgpu_renderer.rs` resets the namespace and receipt at draw entry.
- `src/fast/frame.rs` adds Flow state, upload/pass accounting, scoped error checks and
  receipt publication after queue submission. It restores Path winding and
  paint coordinates to window space for single-scene Flow tiles.
- `src/fast/flow_namespace.rs` defines namespace identity, panic-safe scope, cache reset,
  execution receipts and failure stages.
- `src/fast/flow_headless.rs` uses the production renderer with a persistent
  render target, checked RGBA readback and real-adapter tests.
- `src/wgpu_context.rs` and `src/fast/rounded_clip.rs` enable production
  headless device construction with the Flow feature.
- `src/shaders.wgsl` evaluates Flow tile Path coverage at the original
  window-space sample coordinates. It maps the hard clip to integer pixel
  bounds so coverage includes a sample whose center lies exactly on either edge.
- Formatting skips the external core fixtures referenced by upstream's
  optional `test-support` rounded clip tests.

The analytic Path renderer is shared by direct and tiled rendering. It uses
upstream's path grid. Flow tile geometry and hard masks stay in tile space.
Pixel output can differ from the previous gpui-fast release's MSAA renderer.

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

`FlowLayerNamespace::new_window_space()` opts into single-scene layer content
in window-device coordinates. It uses the upstream analytic Path renderer
with window-space winding and shading to preserve direct-rendered Path pixels.
Generic scrolling layers use `new()` and tile-space Path winding. The
window-space mode rejects multipart content.

The native opt-in frame path captures Validation, Internal and OutOfMemory
errors separately before submit and around submit. Only an error-free queue
submission publishes `FlowExecutionReceipt`. Every draw starts with no receipt;
any returned failure clears pending counts and the published receipt. A surface
failure therefore cannot reuse the last successful Flow receipt. The receipt
means accepted submission, not GPU completion or presentation success.

- `tile_raster_passes`: actual begin_render_pass operations targeting layer
  tiles
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

Tests behind `upstream-font-tests` require the original TTF fixtures from
upstream `assets/fonts`. These fixtures are outside the extracted crate.
The upstream rounded clip integration tests enabled by `test-support` require
`crates/gpui/src/fast/rounded_clip_fixtures.rs` in the upstream workspace.
The upstream image-returning headless renderer has its own optional
`bench-support` and `test-support` features.
