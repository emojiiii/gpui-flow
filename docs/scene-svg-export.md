# Fixed-region mixed SVG scene export

This is the bounded SVG branch of ELM-11. `SceneExportFormat::Svg` uses the
existing immutable `SceneExportPlan`, fixed world crop, image/Canvas resource
observations, and same-backend text preparation contract. It returns a complete
UTF-8 SVG file (`image/svg+xml`) without files, dialogs, network access, scene
mutation, or editor history changes.

## Vector and raster content

This is a **mixed SVG**, not an all-vector or editable-text promise:

- Rect, ellipse/arc/ring, polygon, star, Path, Pen, Line stroke, Box and Frame
  geometry remains vector. Canonical `Path::to_svg` serializes path commands;
  existing primitive `to_path`/contour generators supply geometry
- Stroke geometry reuses `stroke_mesh_with_rule`. Its consistently wound
  triangle union is emitted as one nonzero-filled vector path per stroke layer,
  preserving the existing alignment, dash, cap/join and curve tolerance contract.
  It is not a raster or a set of independently alpha-blended triangles
- Solid and local-space linear/radial gradient paints, fill rules, multiple
  fills/strokes, local transforms, stable sibling/root order, nested group
  opacity, rounded child clips and container before/after-child stroke phases
  are preserved. Gradient interpolation is explicitly sRGB
- Images contain only current host-supplied decoded pixels as PNG data URLs.
  The existing actual-decoded-size crop/contain/cover/fill mapping is reused,
  including the selected source rectangle and rounded image-box clip
- Canvas pixels are a separate PNG image in the vector Canvas outline, between
  its vector fills and strokes. Its smoothing preference is represented by SVG
  `image-rendering`; exact resampling remains consumer-dependent
- Text is one host-shaped **raster ink image per Text node**, preserving its
  local ink bounds and exact local content clip. It is marked
  `data-flow-raster="text-ink"`, just as image and Canvas nodes are identified
  separately. Transparent glyph gaps remain transparent. No text rectangle,
  guessed glyph outline, fallback font, source font name, or editable `<text>`
  element is substituted

A geometry-only scene contains no `<image>` elements. The complete scene is
never rendered into one PNG and wrapped in SVG. Image, Canvas and Text nodes
remain intrinsically raster content; zooming them beyond the captured resolution
can reveal pixels. Vector geometry can be scaled independently without that
loss. The existing output scale/DPR controls the same-App text producer's ink
resolution, while Image/Canvas use their current selected frames without
inventing additional detail or advancing animation.

Visible alpha-mask markers, non-Normal/non-PassThrough blends, visible outer or
inner shadows and Custom producers reject the **whole** SVG. A hidden mask still
changes following sibling coverage, so it also rejects. Unsupported producers
outside the crop reject too. Ordinary hidden/zero-opacity subtrees remain
skipped. Text styles unsupported by the existing text-ink contract (strokes,
multiple fills, non-solid fills) also reject. Transparent shadow layers have no
visible effect and do not reject.

These boundaries do not add SVG filter approximations or silently change the
raster compositor's supported effects. SVG does not imply complete Leafer export
parity; higher-fidelity complex effects, editable text/outline production, and
cross-consumer/cross-platform pixel equality remain outside this slice.

## Crop, viewBox, scale and background

The same explicit `SceneExportRequest` applies:

1. Finite positive fixed `world_bounds`, finite positive `scale` and
   `pixel_ratio`, explicit RGBA `background`, and `format: SceneExportFormat::Svg`
2. Physical `width` and `height` are `ceil(world extent * scale * pixel_ratio)`,
   subject to the same bounded dimensions/pixel count as raster export
3. The root `viewBox` is **physical** `0 0 width height`. An outer affine matrix
   maps world coordinates into those physical coordinates, uniformly applying
   scale × DPR and mapping the crop origin to zero
4. An explicit `userSpaceOnUse` clip retains the **exact world rectangle**.
   Fractional last pixels are not stretched to fill the ceil-rounded extent;
   they can have partial alpha. No auto-trim, editor pan/zoom, native widget,
   theme, grid, selection or overlay is consulted
5. The explicit background paints behind scene content within that crop.
   Transparent backgrounds remain transparent; alpha backgrounds are preserved

Every element's local affine matrix is retained in nested `<g>` elements.
Matrices must remain finite and invertible. Canonical path conversion keeps the
existing 16,384 commands / 1 MiB path data / ±1,000,000 local-coordinate limits.
Stroke triangle-union output is instead bounded by the composite triangle and
final-output budgets; it is not re-imported through the source-path command cap.
Generated geometry, gradient positions and embedded image/ink rectangles also
use the ±1,000,000 coordinate bound; a finite image mapping outside it rejects
rather than emitting unsafe numeric data. No browser layout round-trip is used.

## Resources, text fidelity and privacy

`source()`, `viewport()`, `text_request()`, `encode_with_text()` and
`encode_with_inputs()` remain the resource bridge. SVG text preparation gets the
same physical viewport as the raster branch; no full-scene raster or full-frame
codec scratch is reserved for a geometry-only export. Missing, pending, failed
or stale visible resources reject the entire operation. A host may explicitly
supply empty Text ink as `Ok(None)`.

The SceneView adapter can use its installed same-App text rasterizer unchanged:
its resolved font, shaping, wrapping, case/paragraph spacing, decoration,
alignment and clip are captured in actual glyph pixels. The adapter's existing
font-epoch and current-resource ticket checks remain mandatory before returning
bytes. Font files are not embedded or fetched. No claim is made that the SVG
contains glyph outlines or editable text, or that an unrelated viewer has the
same installed fonts.

All PNG images are embedded with a fixed `data:image/png;base64,` prefix. Source
URLs, query tokens, resource filenames and image credentials are never copied
into SVG. The serializer does not initiate fetches or output external resource
references, scripts, CSS URLs, event handlers, links, arbitrary scene metadata,
source text content or font names. Scene IDs appear only in XML-escaped
`data-flow-id` attributes; generated internal IDs use a deterministic counter.
XML-special characters and attribute whitespace are escaped; forbidden XML 1.0
control characters reject. Consumers need no external resource or font access.

## Bounded atomic serialization

`CompositeLimits` ceilings still cap dimensions, output extent, nodes, tree
depth, source/generated geometry and stroke triangles. Callers may only reduce
limits. The SVG serializer additionally charges:

- Current ImageInputs/TextInputs buffers, deduplicated by each existing input
  collection, plus `SceneExportPreparation::retained_bytes`
- Live generated-path command capacity and path-string capacity, retained
  stroke mesh storage with growth slack,
  live encoded PNG capacity, and final SVG vector capacity
- Cumulative host `pixel_work`, three work units per embedded PNG pixel, and
  one work unit per serialized XML byte, against `max_surface_writes`
- The existing 8 MiB PNG codec/scratch allowance while each resource is encoded;
  geometry-only files do not pay this allowance

Base64 is streamed through a fixed-size stack buffer; there is no second
full-size base64 string. Resources reused by several nodes may be embedded more
than once, and each encode/serialization is charged. Output capacity grows only
within the remaining allowance. Budget failure, invalid XML/geometry, missing
resources and cancellation return an error without any partial successful file.
Snapshots can be encoded repeatedly and are unaffected by later scene edits.

These limits admit explicit retained buffers/output and reuse the existing
geometry/codec work ceilings. Shared geometry producers and codecs still own
internal allocations. This is not a process-wide memory limit, allocator-failure
recovery guarantee or compression-latency bound. Cancellation is checked on
serialization writes and before/after each PNG codec; it does not interrupt the
codec's internal compression work. Recheck admission if locked codec or path
implementations change.

## Pinned source comparison

The read-only GitHub source inspected was
[ExportModule.syncExport/export](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/export/src/export.ts),
blob `00250118fe511418e5ae3e04d8b413991fc95489`. Its SVG branch delegates to
`leaf.toSVG()` and optional download. The raster-specific bounds/relative-space,
scale, pixel-ratio, clip, trim and padding logic runs in a separate branch.

This implementation defines its own explicit fixed-world-crop SVG contract,
shared with the existing [scene raster export](scene-raster-export.md); it does
not infer that upstream's `leaf.toSVG()` facade implements the same crop or
resource semantics. It rejects unavailable resources immediately instead of
waiting in an asynchronous global resource loop. It does not copy download,
trim/padding, relative coordinate spaces or filename-driven behavior.

## Validation

The focused tests parse complete XML with locked `roxmltree`, read path
data back through the scene path parser, and rasterize emitted files with locked
`resvg 0.46.0` using its `raster-images` feature (without system fonts or text).
Embedded PNG bytes are independently base64-decoded and read with the existing
PNG decoder. Tests cover geometry-only vectors, dimensions/world crop/DPR,
fractional edges, background alpha, hierarchy/group opacity, rounded clips,
container paint phases, solid/gradient/layer/fill-rule behavior, canonical
primitive curves, advanced stroke meshes, XML escaping, URL non-disclosure,
Image crop and Canvas pixels, explicit text ink/content clipping, unavailable
and failed resources, unsupported effects, immutable snapshots, budgets and
cancellation.

On 2026-10-02, all **28 new SVG tests** passed, the complete composite suite
passed **294 tests**, and strict all-target composite Clippy (`-D warnings`)
passed. Changed-Rust-file formatting and `git diff --check` passed. These runs
used the locked graph, offline dependencies, one Cargo job and the existing
shared build cache. The manifest adds direct dependency edges to already-locked
`base64 0.22.1`, `roxmltree 0.21.1` and `resvg 0.46.0`; no package version,
source or checksum was upgraded.

```sh
cargo test --locked --offline -p gpui-flow-composite scene_svg_tests -- --test-threads=1
cargo clippy --locked --offline -p gpui-flow-composite --all-targets -- -D warnings
cargo test --locked --offline -p gpui-flow-composite -- --test-threads=1
```

Compact validation record (historical record in Git history)
records source and log hashes. Native SceneView/font acceptance, host file
delivery and cross-platform comparison are separate from these deterministic
core tests. This pass did not open a native window, commit, push, trigger Actions
or deploy.

## Subsequent native acceptance

The capture-format erratum (historical record in Git history) applies to the 25 desktop captures below,
which are JPEG bytes under historical `.png` names. Actual exported/embedded PNG
files and their independent decode/FNV/alpha checks are unaffected.

The frozen Linux SceneView/font/file-delivery run passed its exercised matrix:
13 physical exports, eight SVG plus two PNG files, three expected atomic
rejections, 25 screenshots, normal exit 0. Actual resvg readback and an independent
file audit establish mixed vector plus per-producer Image/Canvas/Text ink; no
editable text, glyph outlines or SVG/PNG bit-exact claim is made. See the
[native report](scene-svg-native.md) and
compact native record (historical record in Git history).
Together with the earlier PNG/JPEG/WebP evidence, ELM-11 is bounded verified
within the existing four-format/fixed-region contract. The earlier core-only
validation boundary above remains historical.
