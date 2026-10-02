# Fixed Image source-crop editor

This bounded slice adds a normalized source rectangle and an opt-in source-frame
editor for one built-in Image. It reuses Flow's resource lifecycle, capture,
revision guards, property validation and Undo history. It does not fetch or
decode a second copy of the image or introduce a parallel editor framework.

## Document and rendering contract

- `Image::source_crop: Option<Bounds>` is a normalized positive rectangle inside
  `[0,1] × [0,1]`. Missing/`null` means the complete source; `None` is omitted in
  serialized Image geometry. Legacy documents retain their previous behavior
- `PropertyKey::ImageSourceCrop` / `image.source_crop` accepts this rectangle or
  JSON null. Invalid coordinates, nonfinite values, empty extents and crops
  outside the source are rejected atomically. Crop changes dirty geometry,
  paint and hits; they preserve the loaded resource ticket and source generation
- `Image::mapping(decoded_size)` resolves the selected physical source pixels,
  fits that selected rectangle with Contain/Cover/Fill, and places the complete
  texture with a destination clip. CPU compositing and source alpha sampling use
  this mapping and actual decoded frame dimensions, preserving fractional texels
- Every explicit source_crop, even an integer-aligned or full-source rectangle,
  uses the existing bounded CPU domain. GPUI's native atlas crop rounds source
  coordinates to integer texels, and actual decoded size is unavailable during
  domain planning. This conservative policy preserves normalized semantics;
  it never silently snaps a crop. None/full-source native behavior is unchanged
- `natural_size` remains optional document metadata. It cannot override loaded
  pixels in rendering or runtime alpha picking. Ordinary geometric engine hits,
  and pending/failed alpha Geometry fallback, keep their established metadata
  geometry. Only currently sampleable alpha-opted-in Images may use the engine's
  explicit rounded outer-box candidate hook before actual decoded-alpha filtering
- Image size, source, fit, corners, object transform, style, parent and identity
  are unchanged by crop gestures. Own rounded edges, ancestor clips and affine
  transforms remain in force through the bounded CPU domain
- The DAT03 Leafer adapter rejects nonempty `source_crop` on export rather than
  silently dropping this Flow-only property or guessing an upstream mapping

## Inner editor

Call `SceneView::open_image_crop_editor(handle, window, cx)` for a live editable
built-in Image. `close_image_crop_editor(cx)`, `image_crop_editor_active()` and
`image_crop_target()` expose its bounded lifecycle. The tool selects that single
Image and hides ordinary transform handles while active.

The preview temporarily stretches the complete source into the unchanged outer
Image box using Fill. Eight yellow handles delimit the normalized source frame;
drag an edge/corner to resize, or drag inside the frame to move it. Coordinates
use the frozen inverse world matrix and the current viewport conversion, so
rotated, skewed, reflected and nonuniformly scaled ancestors are supported.
The temporary preview never replaces canonical document fit/crop or exported
scene data. The uncropped None+Fill preview may use native presentation when
otherwise eligible; canonical cropped presentation always uses the CPU domain.

- Off-center pointer presses use a frozen delta and do not jump the frame
- Movement stays inside the full source; resizing does not flip or empty an axis
- Minimum interactive axis is 1/4096 of the source, or the initial axis if smaller
- Shift constrains body motion to its dominant normalized source axis and locks
  a corner's initial source aspect ratio; edge handles remain one-axis controls
- A completed pointer drag is one Undo entry, irrespective of move count. A
  no-op or exact return to the initial frame creates no history and retains None
- Done/Enter closes the preview. An unfinished drag is cancelled on close;
  completed drag edits remain in history. Escape cancels an active drag, and a
  second Escape closes the preview. Focus loss and missing/wrong mouse-button
  state follow the existing capture-cancellation rules
- Read-only denies editing and hides the mode; plugin disable/uninstall cancels
  capture. Source/selection/document/attachment changes expire stale mode/capture
  before further writes. External edits are not overwritten with an old snapshot
- Typed crop writes cannot be retargeted or changed into another command by
  middleware. Post-layout validation rejects changes to the target's outer pose
  or unrelated object fields, so a failed derived-layout write is atomic

## Validation and physical recipe

Focused model and UI results are recorded separately from native window receipts.
A passing `cargo check` or linked demo is not a physical drag/pixel acceptance.
Run the native fixture with:

```sh
cargo run --locked -p flow-studio --bin image_crop_trace
```

The fixture uses generated local pixels and a controlled ready/pending/failed
cache. Left full-source preview is native eligible; right preview is forced
through the CPU domain. Both canonical crops use CPU. Both carry deliberately
false `natural_size` metadata. No network is used.

1. Click Crop left. Drag the left yellow edge from its initial frame to about
   one quarter of its width. During the drag verify full-source preview and fixed
   outer box. Release, Done, Undo, Redo: one Undo restores the original complete
   image, and Redo restores the crop without changing its outer matrix
2. Reopen. Drag inside the crop to pan its source frame; try an off-center press,
   Shift, extreme edge motion and a corner crossing attempt. Confirm bounds and
   positive minimum size. Start another drag, press Escape before release, then
   release: its baseline returns and no extra history appears. Escape again closes
3. Right-half both selects only the right half of the source. Compare the two
   CPU-settled crops while cycling Fill → Contain → Cover. Fractional both uses
   an asymmetric crop with non-integral source coordinates (32.48, 20.04,
   80.48, 68.52) in the fixture's 160×120 source; it must not snap to integer texels.
   The lower right source region is transparent and exposes the purple underlay. Zoom and Affine + clip
   CPU exercise transformed/rounded ancestor clipping; wait for CPU upload settle
4. With crop mode closed, turn on Alpha hit. Under Contain and the right-half crop,
   click visible opaque image pixels outside the stale metadata letterbox: target
   must be native/cpu. Click transparent source pixels: target must be under-native
   or under-cpu. Actual letterbox and rounded/ancestor-clipped holes do not hit Image
5. Cycle native source ready → pending → failed → ready. The fixture's alpha
   policy passes through pending/failed states. Confirm old ready pixels cannot
   accept a hit after source replacement, even if they remain on screen briefly
6. Read-only hides the crop mode and refuses reopening. Re-enable, start a crop,
   then switch focus away or release outside the canvas and return without a
   pressed button. Verify capture is cleared and the owned unfinished edit cancels
7. Remount / reset creates a fresh view with no crop mode, no stale capture and
   default geometric picking. Reinstall Alpha hit explicitly. Close the window
   normally and retain stderr input traces, screenshots and the exit receipt

Additional regression commands:

```sh
cargo test --locked -p gpui-flow-scene -p gpui-flow-engine -p gpui-flow-editor -p gpui-flow-interaction --test image_crop
cargo test --locked -p gpui-flow-composite source_crop --lib
cargo test --locked -p gpui-flow --lib
cargo check --locked -p flow-studio --bin image_crop_trace
```

## Bounds of this slice

One built-in leaf Image at a time, normalized rectangular source selection and
nearest-source-alpha picking only. No arbitrary inner-tool factories, crop
rotation, skew/pivot controls, EXIF editing, destructive pixel export, mask/effect
alpha picking, source resampling or new image loader is provided. Tool preview
uses an intentionally stretched full-source view, not a separate photo editor.
There is no native atlas fast path for explicit source crops. Cross-platform
native acceptance, animation crop gestures and high-DPR physical input require
their own receipts; pure mapping tests do not claim those windows.

## Verified Linux window: 2026-10-02

The frozen `image_crop_trace` executable ran **18:26:32–18:32:37 UTC** and
closed normally with exit 0. Independent audit parsed all **1,434 contiguous
JSON trace records**, checked the frozen build-input manifest, and inspected
screenshots 01–28. No routed input event reported an error. The deliberately
requested read-only open produced `canvas capability denied: Edit`; its status
message remained visible until remount. Pending/failed source badges were also
intentional fixture states, not unexpected failures.

Physically verified in this window:

- Left-edge drag by approximately 80 pixels produced source x≈0.250004 and
  width≈0.749996. Done restored cropped presentation; one Undo restored None,
  and Redo restored the identical crop. The cropped and Redo canvas screenshot
  regions are byte-identical. Both Image-local matrices and 320×240 boxes were
  unchanged throughout all 1,434 records
- Reopening and an off-center Shift body drag moved only the normalized x axis
  to ≈0.056251. Corner crossing clamped both axes to exactly 1/4096, without
  flipping; one Undo restored the preceding frame. Escape closed the idle mode
- Right-half and asymmetric fractional presets displayed all three fit modes.
  Both explicit crops used CPU compositing; the uncropped left editor preview
  remained native eligible. Screenshots show corresponding settled colors,
  letterboxes and transparent underlays. They are visual fit checks, not a claim
  of byte-identical GPU outputs: complete left/right screenshot interior regions
  contain edge-adjacent channel differences, recorded in the audit summary.
  The precise unsnapped fractional source mapping is separately established by
  the automated independent CPU pixel-reference regression
- Real pointer traces accepted opaque Contain pixels as `native` and `cpu`,
  including positions rejected by the deliberately incorrect natural-size
  metadata. Transparent pixels reached `under-native` / `under-cpu`, and the
  left Contain letterbox reached `under-native`
- Affine/rounded clipping on the right and viewport zoom 1.2 were inspected in
  settled screenshots. Pending and failed left-source states showed no old
  Image pixels and both clicks passed through to `under-native`; ready recovered
- Read-only closed the mode and rejected reopening. After re-enabling, a source
  body drag left the canvas with capture held; returning after outside release
  produced PointerCancel, cleared capture/editing, and restored the exact
  baseline crop. Before/after-cancel canvas screenshot regions are byte-identical
- Remount reset the crop mode, both crops, zoom and error status. Reinstalling
  alpha opt-in then produced transparent clickthrough again

### Explicit limits of this receipt

The last purported rounded-hole click hit the selected underlay's TopLeft resize
handle (trace 1432), so it **does not verify rounded alpha rejection**. Native
Escape-during-drag and focus-loss-with-button-held were not exercised: the logged
Escape closed an idle mode, and every FocusLost event had no capture. Those
cancellation/rounded-clip cases retain automated regression coverage only.
The right affine parent is intentionally changed by the fixture; the invariant
above concerns each Image's local matrix, not that parent matrix.

This is one Linux native window, not Windows/macOS or physical high-DPR
acceptance. It does not claim bit-exact native-versus-CPU crop rendering; explicit
source crops have no native atlas fast path. The crop implementation was subsequently published as
[`e88d78af`](https://github.com/emojiiii/gpui-flow/commit/e88d78af5a235d4921414a9789d65be3c0ecb531), with zero Actions runs for that commit. The full binary also contains contemporaneous JSON/layout fixture WIP; it is not asserted to match that entire remote tree. Later source changes require their
own checks; these results are anchored to the archive below.

### Provenance and automated scope

- Frozen complete source archive `source-before-native.tar.gz` SHA-256:
  `7555e4e9f0e05f933ce045f623b81de67886455fead3642e6fc7fe99066c44d8`
- Frozen binary SHA-256:
  `99a33daf5f2d41bad255fa174af94464e3e142b1daabd52cce5e0e7c02af8e18`
- All 438 entries in `build-inputs.json` match the archived source bytes
- Pre-precision baseline: 17 model crop tests and full UI library 204/204 passed;
  the earlier composite source-crop filter had 3 passes, including 2 existing
  Canvas cases. These counts overlap later focused checks and are not additive
- After the final CPU precision-routing change: 9 composite Image tests, 9 UI
  crop-filter tests and 4 UI Image tests passed (one UI test overlaps). Strict
  composite/UI all-targets Clippy and demo Clippy, formatting and diff checks
  passed. The full 204-test UI suite was not rerun after this final routing delta

The machine-readable [validation summary](validation/2026-10-02-image-crop-summary.json)
records trace sequence evidence, screenshot names/hashes, exact comparisons,
automated scope and unverified cases. Original screenshots, native logs and
frozen source/binary remain in the local validation artifact bundle.
