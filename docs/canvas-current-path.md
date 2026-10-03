# Canvas current-path construction

Status: implemented with Linux automated and bounded native acceptance.
The full DPR 1/2 functional run passed against published source
[`39fcf56a`](https://github.com/emojiiii/gpui-flow/commit/39fcf56a2e91f59d5da7a04ee519cade66f8cb69).
Its original clipped-footer defect is preserved in the evidence; a separate
layout-only run verified the footer fix. See the [native report](canvas-current-path-native.md)
and [machine-readable summary](validation/2026-10-02-canvas-current-path-native-summary.json).
No browser or cross-platform pixel equivalence is claimed.

## Runtime path and batch-local state

`CanvasSurface` owns one current path. `draw_commands` can construct it with
`BeginPath`, `MoveTo { to }`, `LineTo { to }`,
`QuadraticCurveTo { control, to }`,
`BezierCurveTo { control1, control2, to }`, and `ClosePath`.
`FillCurrentPath { paint, rule }`, `StrokeCurrentPath { stroke }`, and
`ClipCurrentPath { rule }` reuse that geometry. The existing explicit `FillPath`,
`StrokePath`, and `ClipPath` commands remain available.

- The current path survives successful calls, including path-only edits through
  `SceneView::edit_canvas`. A later batch can append or draw without resupplying it.
- `BeginPath` replaces the current geometry with an empty path. It does not erase
  pixels. Filling, stroking, and clipping do not consume the current path.
- Positions and curve controls are transformed when appended. Changing the
  transform afterward does not move those stored positions. Draw-time transforms
  still govern paint coordinates and the stroke pen.
- Transform, opacity, clips, and the save stack remain batch-local. Each call
  begins with identity, opacity 1, and no clip. The save stack must balance.
  `Save`/`Restore` includes these drawing states, **not** the current path.
- `ClipCurrentPath` intersects with the frozen current geometry. It follows the
  existing bounded per-draw clip semantics, including nonzero/evenodd fill rules,
  opacity-independent clipping, and restoration of prior clips. Later path edits
  must not change an already established clip.
- A geometry-only batch returns `false`: it changes runtime path state without
  publishing pixels, increasing pixel generation, or replacing the pixel
  snapshot/upload. The return value remains a pixel-publication result, rather
  than a report that every kind of runtime state is unchanged.
- A failed batch commits neither its tentative geometry nor its pixels. Invalid
  late commands therefore cannot partially replace a previously retained path.

An empty `LineTo` begins at its endpoint. Empty Q/C commands begin at their
first control point. Empty, single-point and repeated closes do nothing; continuing after a close
explicitly restarts from that subpath's first point. All stored positions and
inverse-transformed draw snapshots must remain finite, with valid path bounds.

Size or DPR changes reset current geometry, including safe resizes and a safe
A→B→A sequence that occurs before resource preparation. Safe resize
still retains only the physical top-left pixel intersection without scaling;
cropped pixels do not reappear on growth. `clear` clears pixels while preserving
the path. Geometry and pixel content remain runtime resources outside scene JSON
and editor history. Configuration Undo does not reconstruct past commands.

This is the incremental-path subset of the pinned core
[Canvas path/state contract](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/interface/src/canvas/ICanvas.ts).
It does not turn Flow into a persistent browser drawing context. Arc/ellipse,
arcTo, rect/roundRect convenience commands, retained paint state, arbitrary JS,
and browser API equivalence are not claimed by this slice.

## Admission and ownership

The existing 256-command/16-saved-state limits remain. Persistent path length
obeys `max_points` across calls. Synthetic starts, copy-on-write candidates,
frozen clip snapshots, and repeated draws are charged against cumulative batch
point/work admission; `BeginPath` does not reset that work budget. Geometry uses
immutable shared ownership, so clips and surface clones retain frozen geometry.
A document/element clone starts with a fresh runtime surface instead.

`CompositeLimits::max_path_bytes` defaults to 16 MiB and independently bounds
retained geometry, candidate capacity, and emitted path copies. The UI adapter
also caps live geometry plus candidates at 16 MiB across its resources. Shared
baseline allocations count once, so a full pool can still release paths with
`BeginPath`. Candidate admission keeps the original alive throughout a multi-call
edit. Conservative cumulative allocation charges can reject a batch before the
peak actual memory reaches the ceiling. `byte_len` still reports pixels only;
`current_path_byte_len` reports allocated retained geometry capacity. These are
explicit owned-allocation bounds, not a process-wide allocator guarantee.
Externally retained surface clones/snapshots and arbitrary host allocations are
outside the adapter total. Clones made within an active admission callback
inherit its limit while it is active, and release that temporary restriction
when the callback exits.

All compilation, native font preparation, raster/point/work/memory admission,
and upload failures leave the previously published path and pixels unchanged.
Context clips own frozen path handles; later edits cannot mutate their snapshots.
The UI commits a successful geometry-only candidate without creating an upload.

Text outline stroking and image patterns have later independent
[stroke](canvas-stroke-text.md) and [pattern](canvas-patterns.md) contracts.
The remaining browser path
conveniences and full persistent-context state remain outside this increment.

## Standalone fixture

Run on a native GPUI desktop:

```sh
cargo run --locked -p flow-studio --bin canvas_current_path
```

The left Canvas starts at DPR 1 and the right at DPR 2. Both have 300 × 210 logical
units. Initial generation deliberately uses separate adapter edits for clearing,
building the Q/C outline, appending an evenodd hole, and drawing. The hole is
created between `Save` and `Restore`, demonstrating that path changes are not
undone by restoring drawing state. The outline's append translation and terminal
opacity do not leak into later calls. Per-surface footer reports expose the
publication result, pixel generation, and immutable-snapshot identity.

Controls and checks:

1. **Clear pixels → Fill + stroke**: the artwork returns from retained geometry.
   Clear affects pixels only, and a draw leaves the path reusable.
2. **Clear pixels → BeginPath → Fill + stroke**: the surface stays empty. Build
   outline and Append hole then return `published false`, with unchanged pixel
   generation and snapshot; Fill + stroke renders the rebuilt cross-call path.
3. **Clear pixels → Build outline → Fill + stroke** draws without the hole.
   **Append hole → Clear pixels → Fill + stroke** reveals the added hole. The
   clear is important: source-over filling does not erase earlier ink inside a
   hole. Append is cumulative; pressing it twice adds the same contour twice, so
   evenodd filling cancels its hole. Regenerate starts fresh.
4. **Paint transform** changes gradient placement and the pen scale without
   relocating the geometry. Compare the fixed silhouette/hole to Fill + stroke.
   Clear first when comparing stroke extent, because drawing is source-over.
5. **Clip + stripes** fills the current shape with blue/turquoise stripes and
   preserves its evenodd hole. The clip is requested under an additional
   translation, which must not move already-frozen geometry. A yellow corner
   square follows Restore and an orange corner square belongs to a separate
   batch, demonstrating both ways to leave the clip. Current geometry remains
   available for subsequent Fill + stroke. Clear first to inspect the hole.
6. **Reject late** begins a new triangular path, requests a white fill, then
   issues an invalid unmatched Restore. Both surfaces must report unchanged
   pixel generation/snapshot identity and identical path replay. The replay
   compares temporary before/after surface clones after clearing and redrawing;
   it does not publish those diagnostic draws to either live Canvas.
7. **Safe shrink/grow → Fill + stroke** retains cropped pixels but has an empty
   current path, so that draw must publish `false`. Growing does not reconstruct
   the cropped side. Undo config restores dimensions without restoring a path.
8. **Reset shrink/grow** resets pixels and geometry. **Swap DPR** changes each
   surface between DPR 1 and 2 using safe-resize; preserved physical pixels are
   deliberately not scaled and the path is reset. Regenerate restores the same
   logical artwork at the current DPR. The footer always shows current settings.

Each Canvas is edited independently. Atomicity applies to an individual surface
batch/adapter edit, not to both surfaces as a distributed transaction. The clip
control intentionally performs two batches, so its generation can increase twice.

The fixture writes one JSON stderr record per edit, resize, DPR swap, and Undo,
plus a Regenerate summary. Records include action/origin, status, adapter/action
acceptance, current Canvas configuration, and document revision. The cumulative
`button_action_count` increments once per physical control activation. Startup
records have count zero; the four internal Regenerate edits use explicit
`startup_substep` or `regenerate_substep` origins and do not increment that count.
For Reject late, `accepted` describes the adapter edit running the diagnostic;
`expected_batch_rejection` and the per-surface status describe the deliberate
invalid command batch and its rollback checks. Undo also records whether it
changed configuration.

## Local automated validation (2026-10-02)

- Full affected libraries: composite **309 passed** and UI **252 passed**, zero
  failed or ignored in either run. These contain 15 new composite and seven new
  UI regressions.
- Focused Canvas filters: composite 88 passed and UI 29 passed. These overlap the
  full library runs above and are not additional totals.
- Strict affected all-target Clippy, strict standalone fixture Clippy, workspace
  rustfmt, diff whitespace checking, and the actual native fixture link passed.
- All 475 repository build-input hashes, including Rust/Cargo files and compiled
  assets, matched immediately before and after the final link. Canonical manifest
  SHA-256: `a4a79d1e3c6fc755b31aa06af441c996ffede744a8b4db56f45fba5f2917bd88`.
  Executable SHA-256:
  `531921c529dd27894d5b194d699439c26d17e6adf0a2c9b35eba8e759a823a8c`.

Tests cover cross-call construction, frozen Q/C controls, draw-time gradients and
reflected/nonuniform stroke pens at DPR 1/2, implicit starts/single-point and
repeated closes, frozen vector/image/text clips, save/restore path independence,
late-invalid/font/point/work/geometry-memory rollback, cross-call accumulation,
scoped/escaping clones, and resize/DPR/cosmetic/clear/detach/retype/Undo ownership.
Near-full adapter geometry pools still allow BeginPath to release capacity.
Upload/OOM failure commit ordering was reviewed; allocator failure itself was not
injected. No cross-platform, browser-equivalence or GitHub Actions run is claimed.

## Native validation (2026-10-02)

The original Linux native run, **21:12:30–21:18:56 UTC**, exited normally with
code 0: **55 physical button actions, 84 JSON records, 63 original JPEG desktop captures**.
The recorded trace audit passed **27/27 assertions**. Twenty screenshot pairs
have zero differing decoded-JPEG pixels in both 300 × 210 displayed Canvas
regions. The [capture-format erratum](screenshot-evidence-formats.md) documents the original `.png` suffix mismatch.
Cross-call Q/C construction, Clear versus BeginPath, reusable geometry, evenodd
holes, paint/pen transform timing, frozen clips, four late-invalid rollbacks,
safe/reset resize, DPR swap, configuration Undo, and regeneration were exercised.
The comparisons are within-run decoded-capture checks, not lossless Canvas/GPU
buffer identity, DPR 1 versus DPR 2 pixel equivalence or proof of every renderer
frame. Runtime generation, snapshot identity and path-replay receipts are
independent of screenshot encoding.

All 475 frozen repository build inputs match the archive, pre/post-link
manifests, and published functional source `39fcf56a`. External dependencies,
compiler, and system libraries are excluded; no full reproducibility is claimed.
The original fixed-height footer clipped the Right result row; its complete
receipts remain in JSON. Expected immediate `Compositing...` transitions and
preceding-state captures remain in the original screenshot set, followed by
settled witnesses.

A separate **21:26:06–21:27:04 UTC** layout-only run exited 0 after two physical
button actions, with 11 JSON records and eight JPEG desktop captures. At restored **1090 × 670**
content size, both result rows were fully visible after Regenerate and Reject
late. The footer-only source fix is published as
[`8dd6f110`](https://github.com/emojiiii/gpui-flow/commit/8dd6f1104d2e134b8b15f83e64c8fdb336ee4044).
That follow-up executable also contains unpublished-at-build open-stroke work;
its 475-input frozen shared-tree provenance does **not** establish a build of one
published commit, repeat the full matrix, or accept that other work.

See the [native report](canvas-current-path-native.md),
[native summary](validation/2026-10-02-canvas-current-path-native-summary.json),
and [automated summary](validation/2026-10-02-canvas-current-path-summary.json)
for separate source/publication records and raw-evidence hashes. Native pool
rejection, font failure, upload/OOM injection, safe A→B→A before preparation,
and all-frame behavior were not established by these controls. Automated
coverage remains separately recorded above.

The exact frozen ELM-10 current-path constructor set is covered. Later increments
cover [outline strokes](canvas-stroke-text-native.md) and
[createPattern repeat/no-repeat with transform](canvas-patterns.md).
Arc/ellipse, arcTo, rect/roundRect conveniences and full browser
context state remain exclusions, not additional closure criteria. Prior
fillText/measureText evidence remains separate in [Canvas text](canvas-text-commands.md).
Existing slices: [command batches](canvas-commands.md),
[per-draw clips](canvas-clips.md), and [Canvas ownership](canvas-component.md).
