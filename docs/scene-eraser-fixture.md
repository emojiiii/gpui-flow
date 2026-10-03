# Bounded scene eraser native fixture contract

## Delivery and verification state

The native fixture lives at `examples/studio/src/bin/scene_erasers.rs`. This document defines its behavior and acceptance matrix; build, helper and native results are recorded separately in `docs/validation/2026-10-03-scene-eraser-fixture-summary.json`. A successful syntax/type check is not native acceptance. The repaired fifteen-row physical run and its separate capture-integrity incident are recorded in [native evidence](scene-eraser-native.md); the first reset crash remains in the historical failed report (historical record in Git history).

The source uses the public APIs exercised by the existing compositing/export fixtures. Ordinary Studio auto-bin discovery finds it, and existing `gpui`, `gpui-kit`, `gpui-flow`, `serde_json` and `image` dependencies suffice. No new dependency or runtime-only test hook is required.

## Headless fixture tour

Run the existing built bin with `--cpu-tour`; the branch returns before calling `gpui_kit::application()`. It does not construct a SceneView, initialize an Application, create a native window, simulate a callback, or link a separate test harness. It uses the same `document(Case)` builders as the UI and the public `SceneExportPlan::new(...).encode_with_inputs(...)` path with empty image/text inputs, which is appropriate for these actual geometric-only scenes.

The finite tour has seven cases:

1. Four unchanged native presets: validate Scene, compare Scene/Engine hit-ID lists and declared top/underlying erased targets at all named probes, check A's positive-parent/eraser bounds, export each to PNG and lossless WebP and compare exact decoded bytes
2. C with Path, no paint and positive opacity 0.01: three holes are blue regardless of the masks' object/paint alpha
3. That same C Path state with explicit Multiply: opacity-one mask runs return to the documented no-op schedule while the opacity-0.5 run remains erased
4. B's hidden marker set to None: the left Multiply panel sees red and becomes black while the right isolated control stays blue

Every case uses the same fixed 1000×430 crop, scale=1, DPR=1 and default 32,000,000-write limit. The four baselines each encode both lossless formats; the three variants encode PNG only, for eleven successful encodes if all expectations pass. Interior exported RGBA probes have independent declared values, including exact opaque yellow in D's root hole and red=128/blue=127 for C's opacity-one mask with paint alpha 128. These are CPU-export assertions, not screenshot/framebuffer assertions. All output pixels must remain opaque with the explicitly owned yellow export background.

Each case also exercises real typed errors through the same public export entry: cancelled PNG → `Superseded`, one-byte surface budget → `WorkingSetLimit`, and unsupported SVG → `UnsupportedShape(...)`. SVG's first semantic blocker is preserved rather than normalized: A/D report `"SVG erasers"`, B reports `"SVG blend mode"` because its Multiply child is encountered before the hidden marker, and C reports `"SVG alpha masks"`. A missing/changed expected error is a failed tour case, never a successful export or native rejection claim.

Every actual tour execution creates one exclusive `gpui-scene-erasers-cpu-tour-<pid>-<nanoseconds>` directory below the existing directory named by `FLOW_SCENE_ERASER_DIR`, or the temporary directory when that variable is absent. PNG/WebP and per-case JSON/summary files use `create_new(true)`. The stage, returned error, any completed files, geometry results, pixel observations, unchanged canonical source and explicit native=false scope are retained. The tour continues to other cases after a case fails; there is no retry, dimension reduction, hidden budget increase or replacement file. Exit 0 means all seven fixture/CPU cases passed and their receipts were written; exit 1 preserves available evidence and reports failure. Neither exit code establishes native acceptance. Each execution must record actual output and may reveal a wrong fixture expectation or a real runtime/budget failure; do not predeclare the helper passing.

## App/entity reset regression

`--session-reset-tour` runs the same production preset and queued-burst callbacks in a real GPUI Application, without opening a window or rendering a frame. It uses the existing native dependency graph. It first confirms that LoadScene on the readonly entity is denied and preserves canonical JSON, then cycles A/B/C/D/A/A after host edits and a queued burst. Each cycle verifies a different readonly entity, exact initial document, fresh Undo/Redo history, default viewport, discarded old burst token, and functioning host edit plus exact Undo. Retired entity weak handles must no longer upgrade. This does not simulate physical input or establish compositing/pixel acceptance.

The first native 0518Z run failed with exit 101 when its old reset callback unwrapped the correct CapabilityDenied(Edit) response. Its original source, reports and partial matrix evidence remain separate. The revised callback and footer layout were subsequently exercised in the separate 0543Z native run; the seven CPU export cases do not cover this lifecycle regression. The later capture-integrity incident is documented independently of the functional result.

## Scope and evidence boundary

This is a real `SceneView` app with retained geometric leaves and ordinary engine transactions. Its logical window is 1080×790, below 1100×800; three fixed control rows and a fixed status footer surround one canvas. Four presets keep each ownership/ordering question understandable. Preset selection constructs a fresh host-owned readonly SceneView, rebinds its observer and input subscriptions, and releases the previous entity through normal lifecycle cleanup. It resets view zoom/pan and history. The previous readonly view continues to reject LoadScene with CapabilityDenied(Edit); the fixture never temporarily grants Edit or bypasses that command gate. A creation error is reported without replacing the current view. Source edits use the real editor history. Native clicks cannot drag fixture geometry because the view is read-only for interactive editing; host controls still make explicit undoable transactions.

The live status and JSON receipts expose public `compositing_pending`, typed `composite_diagnostics`, render issues, `presented_geometry`, canonical/presented JSON fingerprints, and actual canonical Scene/Engine plus presented Scene hit IDs. Native pointer-down traces also record the router's target and the complete presented/canonical geometric candidates at that point. `presented_geometry` is a supported observation of displayed-state geometry, not access to GPU bytes or a public upload receipt/fence. A clean diagnostic snapshot alone does not establish correct native pixels.

Native acceptance requires all three: observed physical `SceneView` pixels, the corresponding live receipt after settling, and real click traces where specified. Preserve screenshots in their original captured format. Existing desktop screenshots are JPEG and provide qualitative evidence; converting them to PNG would not make them lossless. Do not infer exact 8-bit alpha/color arithmetic from screenshots or label a CPU export as the displayed framebuffer.

The **CPU PNG + WebP** button independently calls public `SceneView::export_scene` for a fixed 1000×430 world region, opaque yellow background, DPR 1, with no grid. It decodes both formats through the existing image decoder, verifies equal dimensions and exact decoded RGBA equality, records the resulting sample pixels/fingerprint, and writes both original encoded files. These are explicitly labeled canonical CPU export results. They establish export/codec behavior independently of native rendering and do not prove that an uploaded image was accepted or drawn. Exact eraser arithmetic, admission-ledger rollback and publication-generation rules remain the production CPU/UI tests' responsibility.

Session directories are created exclusively below FLOW_SCENE_ERASER_DIR (or the platform temporary directory when absent) as `gpui-scene-erasers-<pid>-<nanoseconds>`. Every receipt and export file uses `create_new(true)`; existing evidence is never overwritten. A receipt records failed exports and source-invariance outcomes too. If a later write fails, any earlier successfully created file remains an explicitly incomplete attempt; there is no automatic deletion or replacement. Receipts are local only. No upload, publication, remote image fetch, font installation, external backdrop access, or permission change is performed.

## Four presets and controls

### A: order and inherited transformed clip

Left: external blue root, pass-through parent, earlier red leaf, white geometric eraser with fill + stroke, and green sibling. All siblings have equal z; **Sibling order** moves green before/after the eraser without changing its geometry. An additional fixed Pixel eraser `reach` sits outside the parent's positive aggregate bounds, over external blue. It must remain a geometric candidate even though it contributes no positive parent bounds and cannot erase the external sibling.

Right: a red rounded Frame rotates 8 degrees over a separate blue root. Its broad rectangular eraser is rotated −22 degrees and skewed 16 degrees, with the Frame's rounded clip inherited. A hole reveals blue, and the eraser cannot clear beyond the transformed rounded clip. No image or text producer is involved.

Primary **Pixel/Path/None**, **Opacity 1/.5/.01/0**, and **Fill/no paint** controls edit `cut` and `clip-cut` together; the independent `reach` marker stays Pixel. Fill/no paint removes strokes as well, so Pixel without any paint has zero source alpha while Path still uses its filled contour. Initial Pixel includes the ellipse's stroke in erased alpha; Path ignores it. Turning the primary eraser to None is ordinary white paint, not a clear operation. The positive opacity Path variants must produce the same contour clearing; zero opacity gates clearing and picking off.

Useful world probes at reset zoom=1/pan=0:

| Probe | World position | Settled default Pixel=1 expectations |
| --- | --- | --- |
| hole | 150,140 | blue; candidates include `cut`, then erased `earlier`, then `external` |
| later | 200,190 | green; `later` is the top candidate while it follows the eraser |
| outside-parent | 410,150 | blue; `reach` remains top candidate despite being outside `parent` bounds |
| clip-interior | 715,235 | blue inside transformed eraser + clip; `clip-cut` remains geometric |
| clip-outside | 530,345 | external blue is intact; `clip-cut` is excluded by ancestor clip |

For default parent geometry, the receipt should show `parent` at x=60,y=80,width=280,height=200 and `reach` at x=390,y=130,width=40,height=40. Bounds are geometric/positive-contribution bounds, never tight remaining-pixel bounds. Pointer coordinates in receipts are world coordinates computed from the actual adapter's canvas-local input; don't guess desktop offsets from the nominal window size.

### B: direct-parent isolation, including hidden/zero markers

Both panels place a pure-blue Multiply child over a pure-red external root. The right panel's parent explicitly uses Normal isolation. The left parent is pass-through and has an initially hidden direct Pixel marker. It must still isolate and appear blue like the right panel. **None** removes the eraser marker while leaving it hidden; the left Multiply child then sees red and appears black in OwnedCanvas. **Undo** must restore the exact previous canonical JSON and blue appearance.

**Hidden/zero/visible** cycles a hidden marker at opacity 1 → visible marker at opacity 0 → visible marker at opacity 1 → hidden marker at opacity 1. Hidden and zero both still isolate while clearing nothing; the visible positive marker clears the center to external red. Probe centers are 250,200 and 730,200. The expected appearance assumes a Pixel/Path marker; None is an ordinary leaf and has no marker isolation. The right panel remains a fixed control.

### C: fixed-source alpha-mask scheduling

Three direct-Group mask runs show the same red target and eraser over blue:

1. Left mask object opacity 1, paint alpha 255
2. Middle mask object opacity 0.5, paint alpha 255
3. Right mask object opacity 1, paint alpha 128

The third column distinguishes mask object opacity from mask paint alpha. No mask is drawn as visible paint. The green strip is a later sibling in each run.

| Eraser configuration | Left / right (mask object 1) | Middle (mask object 0.5) |
| --- | --- | --- |
| Pixel with painted alpha | Earlier content is already flushed; direct eraser is a no-op on it | Erases content before mask is applied |
| Path + PassThrough | Clears accumulated content | Clears accumulated content |
| Path + explicit Multiply | Blend triggers the opacity-one flush; clearing is a no-op on earlier content | Clears accumulated content |

**Path explicit blend** switches eraser artistic style between PassThrough and Multiply. Pixel still overrides its artistic style for its effective operation. Positive Path opacity .5/.01 must not attenuate clearing. Probe positions 155,200 / 480,200 / 805,200 fall inside all three masks and cuts, away from antialiased edges. At default full-alpha Pixel=1, these show red / blue / red-blue mix respectively. Path+PassThrough makes all three blue. Native checks validate Flow's documented schedule; upstream browser equivalence remains unexecuted and must not be claimed.

### D: owned root erasure and Domains rejection/recovery

Earlier red, root ellipse eraser and later green are actual scene roots. OwnedCanvas's synthetic document parent isolates the document before compositing it over the owned yellow background and grid. The hole reveals yellow/grid, later green survives, and host control/footer pixels are outside erase ownership.

**Owned/Domains** is a view configuration toggle and is deliberately not an editor Undo step. With a visible positive-opacity root eraser, Domains must report typed `UnsupportedExternalBackdrop`; it must not silently call a blank eraser domain successful. Zero-opacity/hidden roots are not exposed and are skipped in Domains. Returning to OwnedCanvas must settle without the typed rejection and display current geometry/pixels. The receipt includes the expected typed error and whether the current diagnostics match; that field is an observation, not an acceptance verdict.

The CPU export can succeed while native Domains rejects because the export API explicitly owns a separate whole-scene background. This difference must remain visible in the receipt and must not be treated as a native workaround or recovery.

## Bounded physical acceptance matrix

Run one native app process after a focused compile succeeds. Use the native display's actual pixels and original screenshots; no browser emulation. For every recorded settled supported state require pending=false, no composite/render errors, available current presented geometry and an observed physical result. For the deliberate Domains rejection record the typed issue rather than requiring a supported-state pass. Pointer checks must report real IDs, not just selection appearance.

| Step | Action | Evidence and stop condition |
| --- | --- | --- |
| 1 | Open A; wait for settled state | Original screenshot + live receipt; red/blue/green and transformed clip match expected ownership |
| 2 | Click A hole, later, outside-parent, clip-interior and clip-outside | Native trace + live receipt show expected router IDs, underlying erased candidates and clip gate |
| 3 | A Pixel opacity .5; then .01 | Native partial erase changes; independent CPU export records exact decoded bytes; do not use JPEG for arithmetic |
| 4 | A Path .01; then reset A → Path 1; Fill/no paint | Positive Path opacity and absent paint preserve contour clearing; Pixel after no-paint restores target (fixed reach unaffected) |
| 5 | A reset; Sibling order; Undo | Green erased when before; green survives after exact-JSON Undo; hit order follows sibling order |
| 6 | B reset; None; Undo | Hidden marker blue → no marker black → exact baseline blue; capture screenshot/receipt |
| 7 | B hidden → zero → visible | Hidden/zero both blue; visible cut reveals red; positive parent control unchanged |
| 8 | C reset; Pixel | Left retains red, middle erases to blue, translucent-paint right retains red-blue mix; screenshot + CPU export |
| 9 | C Path; opacity .5/.01 | All three contour holes clear to blue for positive opacity with PassThrough |
| 10 | C Path explicit blend | Opacity-one left/right revert to literal no-op; middle remains cleared; screenshot + receipt |
| 11 | D reset; Owned Pixel=1 | Yellow/grid through root hole; green survives; controls intact; click hole yields root-cut and erased root-earlier candidates |
| 12 | D Domains; record; export; Owned recovery | Typed UnsupportedExternalBackdrop is visible and receipted. CPU export labeled independently. Return Owned and verify physical/presented recovery |
| 13 | D or A: 8 frame changes; repeat; start then immediately switch preset | Finite generation churn settles at Pixel=1/visible/white. Switching preset cancels queued fixture callbacks by token; no later mutation of new preset. Capture final receipt/physical state |
| 14 | CPU failure checks; then export and Undo a preceding edit | Public export returns Superseded and WorkingSetLimit, no successful output, source unchanged; normal export recovers; exact-JSON Undo still works |
| 15 | Resize once within 1100×800; wheel/pan then reset preset | Actual native redraw and current hit geometry remain aligned; reset returns zoom=1/pan=0. Record actual display scale |

This is about 10–15 minutes of deliberate physical inspection after build, depending on compositing latency. Do not invent a pass if any row cannot be executed. If a native operation fails, keep the current source/receipt/screenshot and report the exact blocker before expanding the matrix. Additional repeats are justified by failure or unresolved stale-result behavior, not by an arbitrary soak requirement.

The eight-frame control exercises successive requested configurations through real `SceneView` paints. It cannot guarantee that a worker runs slowly enough to be cancelled on every step. Its final-state success must not be called proof of mid-row cancellation, obsolete completion rejection, allocator-ledger rollback, or successful GPU publication. Those require the existing focused CPU/controller tests and the actual physical end-state evidence together. The **CPU failure checks** button uses an already-cancelled token and a one-byte surface budget, and exposes no native worker token or allocator ledger.

## Resource/build gate

This draft adds one bin and no dependencies. Read-only source review observed existing compositor ceilings: max_dimension=4096, max_pixels=4,194,304, max_surface_bytes=128 MiB, max_surface_writes=32,000,000, max_nodes=512, max_domains=8 and max_depth=16. No ceiling is raised.

The four scenes have 9, 7, 18 and 3 retained nodes respectively, with shallow ancestry and no external producers. For a roughly 1080×490 logical canvas at scale 1, one RGBA surface is about 2.0 MiB; at scale 2, about 8.1 MiB / 2.12 million pixels. Forced parent/root, mask and contour scratch require additional admitted surfaces. Those arithmetic estimates are not RSS bounds or a claim that every display scale is supported; scale 3 can exceed the default canvas pixel cap. Use scale ≤2 for this bounded physical matrix and record actual scale; a larger-scale typed limit is a separate configuration outcome.

The fixed CPU export has 430,000 pixels and a 1.64 MiB RGBA buffer. PNG/WebP are encoded and decoded sequentially, but both encoded outputs and decoded buffers may coexist during equality checking. WebP admission also reserves the existing codec scratch budget (roughly 21.3 MiB at this extent, in addition to admitted raster surfaces). Fifteen complete PNG/WebP pairs can be budgeted conservatively at about 60 MiB for both encoded files combined; fixture flat-color compression should usually be smaller, but measure actual bytes instead of assuming. JSON includes the canonical scene plus at most 64 recent pointer inputs.

Build planning allowances are estimates; actual costs belong in the execution receipt. With existing compatible GPUI artifacts, use the established focused Studio bin workflow (one build job, no parallel native builds). Without such artifacts, a first GPUI dependency build is materially larger than this bin and must use the existing resource gate rather than assuming a fast incremental build. At source review, the shared filesystem showed about 15 GiB free; that is a momentary observation, not reserved capacity. Recheck current free space and active build ownership immediately before integration/build. A reasonable planning allowance for a warmed incremental bin is 0.5–2 GiB of temporary/object/link space; this is an estimate only. Do not start another full dependency build on the strength of this number.

## Explicit remaining limits

- No external GPUI/native backdrop erase or framebuffer readback; only supported captured parent surfaces and OwnedCanvas's explicit document/background ownership
- Path supports current geometric leaves only. Group/Box/Frame/Text/Image/Canvas/Custom Path producers reject with `UnsupportedPathEraser`; no union-of-children, glyph-alpha, or rectangle substitution
- Custom Pixel remains an unsupported captured producer; this fixture does not install a custom renderer
- Pixel Text/Image/Canvas and supported effect alpha remain covered by focused production CPU/host tests, not this initial geometric native fixture. It intentionally has no same-App text bridge and no remote image pipeline
- No claim of upstream Leafer browser pixel equivalence, full mask layout/hit policy, grayscale/path masks, Box phase parity, pixel picking, mixed SVG erase encoding, or Canvas draw of a bare eraser root
- No arbitrary cancellation/budget controls for a live SceneView, private controller inspection, public GPU fence, screenshot encoder, or newly invented API
- No macOS/Windows or cross-platform native claim; records identify only environments actually exercised
- This fixture does not establish that APP-03 is complete. This remains the bounded increment described in `docs/scene-erasers.md`

Validation records should contain compilation results, executed physical rows, receipt paths, original screenshot paths, skipped rows and precise source revision separately. A source fixture and a clean build are prerequisites, not native acceptance.
