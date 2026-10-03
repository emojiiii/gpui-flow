# Fixed-region scene raster export

This is the bounded raster subset of **ELM-11**, implemented on 2026-10-02,
including the SceneView resource/font bridge and focused Linux native acceptance.
The separate bounded [mixed SVG branch](scene-svg-export.md) preserves vector
geometry and embeds Image/Canvas/Text ink per producer. Its supported-effects
boundary and acceptance are documented separately.

## Reused implementation

`gpui-flow-composite::SceneExportPlan` captures the complete supplied `Scene`
through the existing `owned_canvas` admission/tree-order path, then calls the
same `rasterize_with_inputs` compositor. It does not introduce a second renderer,
take a screenshot, change the document, or touch editor history. The plan owns
an immutable scene snapshot. Subsequent document edits do not change that plan.

All existing CPU-domain contracts continue to apply: stable root/sibling z-order,
local and group opacity, supported blend modes, sibling alpha masks, vector
fills/strokes, rounded clips, supported shadows, retained Canvas presentation,
decoded Image pixels, and host-produced Text ink. Unsupported effects remain
errors rather than partial native fallbacks.

The ordinary `owned_canvas` constructor still requires an opaque backdrop. This
export-only plan changes its privately captured backdrop to the explicit export
color, which can be transparent. This color is painted before the scene, so
top-level blends can read it and preceding roots. There is no external/native
backdrop capture. The existing opaque-only constructor has a regression test.

## Public contract

`SceneExportRequest` requires all of:

- `world_bounds`: finite positive fixed crop, with its edges within +/-1,000,000
- `scale`: finite positive output scale
- `pixel_ratio`: finite positive additional physical-pixel density
- `background: Color`: explicit RGBA; `Color::TRANSPARENT` requests transparency
- `format: SceneExportFormat`: `Png`, `Jpeg { quality }`, or `WebPLossless`

Output dimensions are `ceil(world extent * scale * pixel_ratio)`. Both factors
are folded into the plan's physical-pixel `world_to_screen` matrix; the exposed
`RasterViewport::device_scale` is 1. The requested world origin maps to zero.
The exact rectangle clips the complete output, so fractional last pixels have
partial alpha coverage in PNG/WebP. Bounds do not auto-trim to visible content.
An empty scene still produces the entire requested background rectangle.

There is no editor viewport, theme, grid, selection or handle input. Callers can
pass their existing presented scene if they want current presented properties;
the core does not evaluate animation, layout, or host state independently.

```rust
use gpui_flow_composite::{
    CompositeLimits, ImageInputs, SceneExportFormat, SceneExportPlan,
    SceneExportPreparation, SceneExportRequest, TextInputs,
};
use gpui_flow_scene::{Bounds, Color, Point, Scene, Size};
use std::sync::atomic::AtomicBool;

let scene = Scene::new();
let plan = SceneExportPlan::new(
    &scene,
    SceneExportRequest {
        world_bounds: Bounds::new(Point::new(10., 20.), Size::new(320., 240.)),
        scale: 1.,
        pixel_ratio: 2.,
        background: Color::TRANSPARENT,
        format: SceneExportFormat::Png,
    },
    CompositeLimits::default(),
)?;
let result = plan.encode_with_inputs(
    &ImageInputs::default(),
    &TextInputs::default(),
    SceneExportPreparation::default(),
    &AtomicBool::new(false),
)?;
assert_eq!((result.width, result.height), (640, 480));
assert_eq!(result.request.format.mime_type(), "image/png");
// result.bytes is the complete file; the host chooses whether/where to save it.
# Ok::<(), gpui_flow_composite::CompositeError>(())
```

`SceneExport` returns only a complete encoded file with dimensions and its
request. No path, file dialog, download, data URL, clipboard, or network write
is part of this API.

## Resource and text integration

`plan.source()` exposes the existing `DomainSource::images()`, `canvases()`,
`text_sources()` and `ids()` accessors. The host resolves those exact captured
producers into immutable input buffers. No URL is fetched, image decoded, font
chosen, or glyph fabricated by this module. Animated images are a host-selected
current frame. Runtime Canvas pixels must also be supplied through ImageInputs.

`plan.encode_with_text(images, other_retained_bytes, cancelled, prepare)` passes
the existing `CanvasTextRequest` to the host callback and consumes the existing
`CanvasTextOutput { inputs, pixel_work }`. A same-App text backend can therefore
reuse its Canvas text preparation unchanged. `plan.text_request(...)` exposes
that request separately when the host needs staged preparation. Text requests
use the exact export matrix and retain the existing effect-aware full-source
text policy; text is not silently cropped before offscreen ink can affect a
visible shadow.

For already prepared inputs, `encode_with_inputs` accepts
`SceneExportPreparation { pixel_work, retained_bytes }`:

- `pixel_work` is actual glyph raster pixel writes already performed
- `retained_bytes` is other host-retained memory kept alive during the operation
- Exclude ImageInputs, TextInputs, and this export's own buffers from that value;
  they are charged automatically, including while codecs run
- Shared buffers within each input collection are counted once, using the
  collection's existing accounting; cross-collection sharing is conservative

Missing visible Image/Canvas inputs, failed inputs, missing text backends,
unsupported styles, visible Custom, geometry errors, cancellation and exhausted
budgets reject the entire result. Visible unsupported/missing producers outside
the crop also reject: the full captured scene must be renderable. Invisible or
zero-opacity subtrees follow the existing skip contract. Empty Text ink can be
explicitly supplied as `Ok(None)` by the host producer.

The host owns current-resource ticket and font-epoch checks. It should capture
current immutable image/Canvas frames, shape with the same native backend, then
verify its epochs again before publishing an asynchronous result. A cancellation
flag is checked before and after codecs and during JPEG/WebP external writes.
It does not interrupt a codec's private compression computation; PNG uses the
existing synchronous `StaticImage::encode_png` implementation.

### SceneView adapter

`SceneView::export_scene(request, limits)` and
`SceneView::export_scene_cancellable(request, limits, &AtomicBool)` are
synchronous, immutable host commands returning `Result<SceneExport,
CompositeError>`. They capture `editor.scene()`, so this adapter exports the
canonical document. Editor pan/zoom, grid/theme, selection/handles, transient
state styles, native widgets and the example's separate preview do not enter
the output. The adapter does not evaluate an independent presentation timeline.

The adapter obtains Image and Canvas observations from the existing resource
adapter and their current engine tickets. It converts only the selected decoded
frame, deduplicates identical `(GPUI ImageId, frame_index)` buffers, and admits
conversion bytes and pixel work before copying. Image conversion, glyph
preparation, composition and codec work share the request budgets. Animated
images use their current selected frame without advancing or changing playback.
Canvas observations must still match the current canonical configuration/ticket;
an old pre-resize snapshot is not accepted as a substitute.

The installed same-App `GpuiTextRasterizer` enters through the existing
`TextPreparation`/Canvas text-input contract at the export's physical-pixel
mapping. Missing, pending, failed, cancelled, or stale inputs reject the complete
export. The adapter passes cancellation through preparation and rechecks the
font backend epoch and cancellation before returning bytes. It never fetches a
URL, starts a resource load, waits for readiness, fabricates text rectangles, or
falls back to a stale image. The host can render/resolve resources through its
normal workflow and explicitly try export again.

These APIs do not write files or create previews. The `scene_export` example
performs those host actions after complete successful export and decode checks.

## Formats and locked dependencies

- PNG: existing streaming `StaticImage::encode_png`, `png = 0.17.16`, RGBA8
- JPEG: locked `image = 0.25.10`, quality **1..=100**, explicit **opaque** request
  background required; no silently selected white matte and no alpha-bearing
  JPEG promise. Scene transparency composites against that background; remaining
  fractional crop-edge alpha is flattened onto the same color. The codec reads
  an RGB view block by block without another full RGB allocation
- WebP: locked `image = 0.25.10` / `image-webp = 0.2.4`, **VP8L lossless only**,
  straight RGBA8. No lossy WebP or WebP quality control is exposed

The only dependency change enables the existing image `jpeg` and `webp`
features. Default-format bundles remain disabled; `Cargo.lock` is unchanged.
No metadata profile embedding, color-management conversion, animation export,
or multi-page/multi-frame file is added. Hidden RGB beneath zero alpha is lost
in the existing premultiplied representation; lossless means lossless relative
to the composited/converted pixels, not the original source file.

## Budgets and atomicity

Every caller limit is reduced to `CompositeLimits` ceilings: dimensions 4096,
4,194,304 pixels, 128 MiB working-set allowance, 32,000,000 pixel-work units,
and existing node/domain/depth/geometry/mask limits. These are ceilings, not a
guarantee that every image at the ceiling will export. Complex scenes and codecs
can exhaust work/memory earlier.

Before raster work, the plan reserves codec work from the same cumulative ledger
as glyph preparation and composition. PNG reserves 3 work units per output pixel;
JPEG reserves 4 per edge-padded 8x8-block pixel; WebP reserves 8 per output pixel.
These are conservative pixel-work admission units, not CPU-cycle or time bounds.

The encoder phase keeps input and host retention charged. PNG/JPEG use an 8 MiB
fixed codec/scratch allowance. WebP additionally admits its straight RGBA input,
the locked codec's RGBA copy, and an internal frame allowance: 8 bytes per pixel
plus 64 KiB, tripled for Vec growth. The four locked Huffman channel codes have
maximum length 15 bits; the bound includes run/header slack. Final JPEG/WebP
output is capped by remaining memory and `2 * RGBA bytes + 64 KiB`; the PNG path
retains its existing equivalent cap. Too-large encoded output returns an error,
never a truncated successful file. JPEG's output writer grows geometrically
within its cap rather than reallocating on every tiny codec write.

These checks bound explicit surfaces/output and conservatively admit known
codec scratch. The codecs and tiny-skia still own internal allocations; this is
not a process-wide memory cap, allocator-failure recovery guarantee, or proof of
bounded compression latency. In particular image-webp builds its internal frame
before writing it to the bounded destination. Recheck allowances if locked
codec versions or parameters change.

The export API borrows canonical state immutably. Failed operations publish no
bytes and do not change scene JSON, Canvas pixels, generation, or undo history.
Successful plans can be encoded repeatedly and survive later document edits.

## Leafer source comparison and open scope

Pinned source inspected read-only:
[ExportModule.syncExport/export](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/export/src/export.ts),
blob `00250118fe511418e5ae3e04d8b413991fc95489`.
Its raster flow considers bounds, coordinate space, scale, pixel ratio, clipping,
optional fills and encoding; its async path waits for resources/view completion.

This slice deliberately fixes world-coordinate crop and explicit background.
It uses ceiling physical dimensions rather than copying upstream logical-size
flooring. Its background is the owned compositor backdrop before painting,
whereas the pinned upstream export flow can fill behind already rendered pixels.
It rejects unavailable resources immediately instead of waiting internally.
These choices are explicit contracts, not claims of full Leafer export parity.

Still open: broader SVG effects/editable text, trim/padding/slice/rotated clip options, alternate relative
spaces, automatic resource waiting, auto-download/data URLs, per-format metadata,
font embedding, lossy WebP, arbitrary native Custom rendering, and broader
cross-platform/performance acceptance.

## Validation

All 264 flow-composite tests passed, including 21 new export tests. Strict
all-targets Clippy (`-D warnings`), changed-Rust-file formatting and
`git diff --check` also passed. The core adds deterministic regression coverage for fixed world origin/crop,
scale/DPR/ceil dimensions, fractional crop coverage, empty and alpha backgrounds,
ordinary owned-canvas opacity contract, z-order/backdrop blends, existing parent
clip/group opacity/alpha masks, current image and Canvas inputs, host text ink,
missing/offscreen resources, Custom, immutable snapshots, JPEG policy and decode,
lossless WebP-to-PNG decoded pixel equality, repeatability, malformed/oversized
geometry, input/codec/output budgets, combined work, and cancellation.

Validation commands use the locked graph, offline dependencies, one Cargo job:

```sh
cargo test --locked --offline -p gpui-flow-composite -- --test-threads=1
cargo clippy --locked --offline -p gpui-flow-composite --all-targets -- -D warnings
```

The final flow-ui library run passed **191 tests**, including **7 new export
integration tests**. The focused integration tests cover selected-frame dedup and
admission before copy, animation-frame selection without playback/document
mutation, missing/pending/failed/stale resource rejection, early cancellation,
Canvas resize-ticket freshness and immutable owned pixels, required font bridge,
and real glyph case/paragraph/affine behavior plus font-epoch rejection. Strict
UI-library and example Clippy and native example linking passed. The audit
inspected the final logs rather than rerunning these builds. This is focused
core/UI/example acceptance, not a full-workspace rerun.

## Native acceptance and independent file audit

The actual Linux window ran **2026-10-02 17:01:50–17:07:56 UTC** and closed
normally with exit **0**. The native operator physically exercised all fourteen
export attempts and inspected the displayed pixels. The source pane contained a
clipped rotated vector, Unicode Upper text with paragraph spacing, a predecoded
embedded alpha image, and runtime-owned Canvas pixels. The decoded file preview
was a separate host `img`, never a node inserted into the exported scene.

The frozen source is
[commit 99392f7c35f956f8765ed5730423df19e3d474cd](https://github.com/emojiiii/gpui-flow/commit/99392f7c35f956f8765ed5730423df19e3d474cd),
tree `23acbddf0bbebdbbc4ef5e92b7bffabe5891f736`, verified by the integrator through
the GitHub API. The audit independently matched **all 522 archive files** to the
local synthetic commit `c4414daccc35dcea505407b7efca672bdf721bab`, whose tree is
exactly that frozen tree; later working-tree edits were not used as evidence.

- Source archive SHA-256:
  `171e9409c93c6e32e38e90dfcb87f68384a5f79e637e546d068ce3380ab6663c`
- Executed binary SHA-256:
  `8f17c920acd6e11e85178fdb58dc411671d5cac5579c6a2897b0616e252e02a2`
- Local evidence directory: `/workspace/shared/scene-export-validation`
- Actual raw JSON is `native.stderr`: **15 records**, comprising fourteen
  attempts and one Undo witness. `native.jsonl` is empty stdout, not the record
  source. `native.receipt` and `native.exit` confirm the run window and exit
- Compact audit summary (historical record in Git history)
  preserves all exact parsed records, independent decode results, source/file/raw
  hashes, screenshots inventory, and inspected build/test-log hashes

The fourteen attempts yielded **10 complete encoded files and 4 expected
rejections**:

1. PNG, JPEG quality 90, and lossless WebP at full 440×320 / DPR 1 / opaque
   background (attempts 1–3)
2. Full PNG at DPR 2, yielding 880×640 (attempt 4)
3. Transparent WebP at world crop `(20,30,360,240)` / DPR 2, yielding 720×480
   (attempt 5)
4. JPEG with transparent background, missing font bridge, pending Image, and
   failed Image each rejected the complete export and retained the previous
   decoded preview (attempts 6–9); **no encoded files** exist for those attempts
5. Restoring Image readiness recovered the same transparent cropped pixels;
   changing canonical vector color changed the output; Undo restored the exact
   pre-edit canonical JSON; changing view pan/zoom left fixed-world export
   unchanged (attempts 10–13 plus the Undo record)
6. Cropped JPEG quality 90 at DPR 2 with the explicit opaque background succeeded
   at 720×480 (attempt 14)

Independent decoding used **Pillow 12.3.0**, its reported JPEG library **6.2**,
and **libwebp 1.6.0**. All ten encoded lengths and native FNV-1a64 file hashes
matched. Dimensions matched crop × scale × DPR, and independent alpha histograms
matched every native receipt. FNV-1a64 is a fixture checksum; the audit separately
records SHA-256 for encoded files and decoded RGBA.

- Decoded full PNG 1 and WebP 3 are byte-for-byte equal RGBA, SHA-256
  `2c67ea35df8a5aa2a7b7e2dd905b423b97554672e5517f82e782151f05285a48`
- Decoded cropped WebP 5 and PNG 10/12/13 are byte-for-byte equal RGBA, SHA-256
  `c06b9ff447ab2f34c3dffbb55f53518b397d33a37120a2a823907c7b84446519`.
  They contain 163,071 transparent, 35,234 partial-alpha and 147,295 opaque pixels.
  The three PNG files are also identical encoded bytes
- Edited PNG 11 differs at **68,902 pixels**, while keeping the same alpha
  histogram; Undo recovered the baseline. Its decoded RGBA SHA-256 is
  `de7a5aff81aa4e457c300a74a70b8dbdfcf9f5685787d56e7251236c07d2f8b9`
- JPEG 2/14 independently decoded to the correct extent with fully opaque alpha.
  Their empty 8×8 corner decoded uniformly to `[231,238,248,255]`, within one
  channel level of the explicit `[232,238,247,255]` background. Pillow/libjpeg and
  native image/zune JPEG decoded hashes differ; **JPEG pixel equality is not
  claimed**. The summary preserves both observed hashes

All fourteen attempts asserted unchanged canonical JSON, engine revision,
selection, viewport and Canvas snapshot identity. Export borrowed SceneView
immutably; the subsequent real Undo explicitly verified the previous edit's
baseline. Missing-producer recovery, repeated exports and view-only movement
were all exercised without contaminating the source with preview nodes.

The example writes unique `create_new` files and JSON receipts only under
`FLOW_SCENE_EXPORT_DIR`, or its documented temp-directory fallback. Those host
example side effects are separate from the library API. This acceptance does not
claim SVG support, lossy WebP, cross-platform font equality, all native scenarios,
large-scene performance, a process-wide memory cap, or workspace-wide validation.
No code, build, UI run, publication, Actions or deployment was performed by the
independent file-audit pass.

### Subsequent SVG closure

The raster checkpoint above remains unchanged and does not itself establish SVG.
The later [mixed SVG native report](scene-svg-native.md) and
compact evidence (historical record in Git history) add the
missing fourth format. Their combined evidence closes ELM-11's fixed-region
PNG/JPEG/lossless-WebP/mixed-SVG contract, retaining all per-format limitations.
