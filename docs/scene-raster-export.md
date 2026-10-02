# Fixed-region scene raster export

This is the headless raster subset of **ELM-11**, implemented on 2026-10-02.
It does not complete ELM-11: scene SVG export remains open. A SceneView resource
bridge and real-window acceptance are separate integration work.

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

Still open: scene SVG, trim/padding/slice/rotated clip options, alternate relative
spaces, automatic resource waiting, auto-download/data URLs, per-format metadata,
font embedding, lossy WebP, arbitrary native Custom rendering, and integration
acceptance beyond this headless core.

## Validation

All 264 flow-composite tests passed, including 21 new export tests. Strict
all-targets Clippy (`-D warnings`), changed-Rust-file formatting and `git diff
--check` also passed. The core adds deterministic regression coverage for fixed world origin/crop,
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

Only focused headless/core checks belong to this slice. No native window,
workspace-wide acceptance, commit, push, GitHub Actions or deployment was run
by this implementation worker.
