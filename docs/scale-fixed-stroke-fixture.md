# Bounded scale-fixed stroke native fixture

The fixture is `examples/studio/src/bin/scale_fixed_strokes.rs`. Build, helper and native outcomes are recorded separately; this contract does not establish a native pass. Existing gpui/gpui-kit/gpui-flow/serde_json/image dependencies and application assets suffice. It creates no scene Text/Image producers and adds no fonts or dependencies.

## Purpose and finite scope

Use one real SceneView with Domains compositing. Each preset contains six vector specimens: three PassThrough parents in the top row and three Normal isolation parents in the bottom row. These are intended native-vector and CPU-domain routes respectively; the receipt labels are route intent, not framebuffer proof. The same document geometry and style are used for corresponding columns. Native appearance still requires physical observation.

Requested window: 1060 × 790 logical pixels, within the 1100 × 800 ceiling. Chrome comprises three 36-pixel control rows, a 1040 × 420 SceneView, and a 190-pixel footer. Total fixed content height is 718 pixels. Actual measured desktop/window/canvas size and whether the whole footer is visible must be recorded; requested dimensions do not prove the host honored them. Do not enlarge the window past the ceiling to hide a layout problem. Fix the fixture if its footer is clipped.

The small document spans 450 × 205 world units. At zoom 2 and Home pan (40,12), ordinary specimen panels remain within the 1040 × 420 canvas. All presets are fresh host-owned readonly SceneView sessions. Observe/InputTrace subscriptions are replaced along with the entity. No preset calls set_scene, unwraps a denied readonly load, or temporarily enables Edit. The pure helper deliberately calls LoadScene on a readonly host to verify CapabilityDenied(Edit) with unchanged source.

## Public API and numerical boundaries

Public APIs used by this fixture:

- Stroke.scale_fixed and StrokeScaleFixed::{Disabled,Fixed,Threshold}; effective styles use the existing hidden Resolved marker
- Scene::{stroke_scale,resolved_object,resolved_for_view,resolved_stroke_camera,world_bounds,visible_bounds_for_view,local_to_world,hit_test_all}
- SceneView::{presented_geometry,execute_command,set_input_tracing,set_editable,install_plugin,uninstall_plugin}, SceneViewEvent::InputTrace and SceneInputOperation::Canvas
- SceneCommand::{Zoom,SetViewport,Fit}, SceneViewportConstraintsPlugin and SceneViewportConstraints, SceneEditor::transaction/undo/redo
- SceneExportPlan::new and encode_with_inputs with explicit logical scale and pixel_ratio

The fixture calls public navigation commands. It does not assign editor.viewport directly or implement its own Fit search. Fit and constraint middleware must evaluate each candidate at that candidate's resolved scale. The Fit receipt uses presented bounds in the physical run; the helper independently asks visible_bounds_for_view at the returned zoom.

Current SceneError conversion into the public interaction host is InteractionError::InvalidInput with a scene-validation message. The expected precision rejection is recorded with its full Debug variant; do not invent a dedicated public scale-fixed interaction error. The physical rejection button records the returned variant and atomicity observations; the helper asserts InvalidInput. If production intentionally changes this variant, update the expectation after inspecting its public signature.

The bounded implementation explicitly rejects SVG export of active fixed strokes as UnsupportedScaleFixed("SVG export"). This fixture produces PNG only. It makes no SVG parity promise. Rotated numeric Y-threshold ambiguity and raw fixed-stroke CPU cameras outside positive uniform pan/zoom are also explicitly rejected; see [the precision contract](scale-fixed-strokes.md). These specimens stay within the declared range.

Presented geometry is derived and may contain Resolved strokes that deliberately reject document serialization. Never use presented_geometry().to_json(), serialize its Style or Stroke, or hash its document JSON. The fixture records its object IDs, effective widths, debug policy/cap/join/alignment strings, local dash values, world bounds and hit IDs. Only canonical authored scenes use JSON, for invariance and exact Undo/Redo checks.

Actual pointer diagnostics repeat SceneView picking with tolerance `4 / zoom` world units and `include_locked=true`; they record this as `presented_hit_ids`. They also preserve `strict_presented_hit_ids` at zero tolerance. The declared specimen probes always use zero tolerance and are geometry queries, not physical events. Near-edge points can legitimately be picked outside visible ink within the four-screen-pixel tolerance.

## Presets and numerical expectations

All authored primary widths begin at 8. All shapes are finite small paths; fill paint is absent. Stroke geometry/hit/clip semantics continue to belong to production code.

| Preset | Three columns | Distinguishing observations at zoom 1 |
|---|---|---|
| A Width | true, numeric 1, disabled | At .5 their ideal logical width is 8/4/4; at 1, 8/8/8; at 2, 8/8/16 |
| B Axes | true under (2,4), numeric 1 under (.5,1), numeric 1 under (.5,1.25) | Local effective widths 4/8/16. Horizontal thickness 16/8/20 and vertical thickness 8/4/8. Equal Y=1 stays in the threshold branch; Y=1.25 triggers division by X |
| C Ancestry | parent (2,1), parent (2,1) + child 90°, reflected parent (-2,1) + child 35° | Scale provenance X=2 and local width=4 in each. 90° exposes final-column-length mistakes; 35° under a nonuniform parent creates an admitted sheared world matrix |
| D Dashes | butt, round, square | Local [18,10] and offset -5 unchanged; zoom changes dash centerline spacing while primary logical width remains 8. Cap tips are not dash centerline endpoints |
| E Open | Center, Inside, Outside | Open L path M10 10 H95 V50, transparent/no fill. Diagonal mask closure has no synthetic closing ink. Aligned meshes keep the existing mask clipping contract |
| F Layers | ordinary, reflection, reflection + 8° | Red fixed 8, blue numeric-1 width 4, white disabled width 2; two nested rounded clips intersect the outline |

Host edit is one editor transaction across all six specimens. It changes primary width 8↔12, numeric primary threshold 1↔2, and parent X multiplier 1↔1.25. It leaves the view readonly. Footer hints describe the unedited baseline; receipts contain the actual edited values. Undo and Redo compare exact canonical JSON to fixture-owned before/after snapshots, with at most 16 retained host edit pairs. Home, zoom, Fit and constraint changes are navigation/runtime operations and must not add document history.

## Fourteen bounded physical rows

Each row has a concrete stopping point. For a normal settled row, wait for pending=false and presented=true, then click Record once. If those conditions do not settle, record the actual pending/error state and mark the row blocked; do not manufacture a pass or silently reuse an older screenshot. No indefinite stress run is part of this fixture.

| # | Action | Evidence/observation and stopping point |
|---|---|---|
| 01 | Open A, Home, Record | Entire window and complete footer visible; both routes show three L specimens; actual window DPR captured |
| 02 | A: Zoom .5, then Record | True visibly wider than numeric/disabled; record effective widths and bounds; stop after settled screenshot |
| 03 | A: Zoom 1, then Record | Strict equality baseline, all three ideal widths 8; inspect native and CPU rows |
| 04 | A: Zoom 2, then Record; click visible red ink in each row | True/numeric remain width 8, local width doubles to 16; preserve actual InputTrace targets, then Record again |
| 05 | Fresh B, Zoom 1, Record | Confirm scalar-X anisotropy and the Y-only threshold jump; horizontal and vertical orientation thickness differ as specified |
| 06 | Fresh C, Zoom 1, Record | Confirm provenance through 90° child and reflected 35° child, including world shear; no local-shear support claim |
| 07 | Fresh D, Zoom .5 and 2, Record at each settled state | Compare unchanged local dash/phase diagnostics and changing spacing. Butt/round/square cap tips may move with width |
| 08 | Fresh E, Zoom 1, Record; click top-inner/top-outer and diagonal midpoint in both rows | Center/Inside/Outside differ; no closing diagonal ink. InputTrace is the actual target evidence; the four-screen-pixel pick tolerance overlaps these near-edge points, so those targets do not establish exact painted coverage. The diagonal midpoint remains away from the real open segments |
| 09 | Fresh F, Zoom .5 and 2, Record | Independent layer policies, reflected geometry and both rounded clip boundaries; click a clipped-out corner and visible ink; Record targets |
| 10 | Fresh A, Host edit, Undo, Redo, Undo; Record at each settled state | One exact canonical edit/restoration per action, ancestor width invalidation visible in both routes. End at original source |
| 11 | A: Home, Fit; Record. Limits on, Fit; Record. Limits off, Home | Actual returned camera and resolved bound fit within 24 logical-pixel padding. Constrained Fit uses .5..1.5. Source/revision unchanged |
| 12 | Fresh E: Record, Reject 1024, Record, Home, Record | Typed InvalidInput scene rejection, source/revision/viewport preserved, no partially accepted width; Home recovery settles. At requested zoom 1024, primary aligned width 8/1024 is below 1/64 |
| 13 | Fresh A: 8 zoom frames; observe pending and settle, Record. Start another burst and immediately choose F then A; click ink and Record | Eight-command bound, final zoom 1 when uninterrupted; retired token blocks queued callbacks after either reset. Fresh history/viewport and live input subscription. Timing of worker cancellation is not asserted |
| 14 | Close normally, confirm process exit, reopen once to A, Record, close | Clean exit and fresh session ID/input binding/history; no stale viewport, source edit, callback or receipt overwrite |

No row requests OS DPR changes. DPR comes from Window::scale_factor in the actual native run. At a runtime DPR other than 1, logical fixed width is not a one-physical-pixel claim. Synthetic DPR checks belong solely to independent CPU exports. Do not claim cross-platform coverage or native/CPU pixel identity.

## Complete physical-record footer template

For each row preserve this human-reviewed record alongside the original screenshot(s) and live JSON:

```text
Run ID / UTC:
Physical row number / preset / action sequence:
Source commit or exact source snapshot hash / binary hash:
Platform / actual native backend / window size / canvas size / actual DPR:
Readonly / entity session ID / viewport zoom and pan:
Source revision and canonical fingerprint before / after:
Presented available / resolved camera / pending before and after settling:
Presented effective widths / ancestry scales / actual presented bounds:
Native actual PointerDown targets / world positions / presented hit IDs:
Typed render or CPU or navigation errors (exact variant/message, or none):
Screenshot original filename(s) / corresponding live receipt filename(s):
Observed visual result for top native row:
Observed visual result for bottom CPU row:
Whole footer visible: yes/no
Verdict: pass / fail / blocked / not run
Mismatch, uncertainty, or outlier retained:
Close/reopen/exit result when applicable:
Evidence boundary: actual visual observation + diagnostics; no GPU framebuffer readback
```

The onscreen footer contains case expectation, readonly/zoom/DPR/pending/presented status, actual presented bound for native-0-ink, last actual routed pointer, action/write status, and explicit evidence limitations. Long action errors are abbreviated only onscreen; their full Debug form stays in the recorded action observations. A record-writing failure remains a failure, not a receipt success. The final physical verdict is filled by the operator, never synthesized from a successful helper exit.

## Evidence paths and helper separation

Before each process, explicitly set FLOW_SCALE_FIXED_EVIDENCE_DIR to an existing absolute per-run evidence root. There is no temp-directory fallback. Each mode creates a new timestamp+PID subdirectory with create_dir and all files use OpenOptions::create_new. Existing evidence is never overwritten.

- Native mode creates scale-fixed-physical-* with manifest.json and up to 256 numbered live receipts. A receipt keeps the latest 64 action observations and latest 64 real pointer traces. Original physical screenshots are captured separately by the operator; this program does not capture a screenshot or GPU buffer
- --cpu-tour creates scale-fixed-cpu-*. It exports only the canonical A scene at (logical scale,DPR) = (1,1), (.5,1), (2,1), (1,2), (2,2). Each output PNG is decoded and actual bytes/dimensions/FNV-1a fingerprints and horizontal cross-section RGBA samples are recorded. FNV is an integrity convenience, not a cryptographic provenance hash. Ideal thickness predictions are recorded separately from raw pixels; antialiasing sample counts are not automatically declared matching
- --headless-tour creates scale-fixed-headless-*. It resolves 6 presets × 3 zooms, checks readonly LoadScene rejection, one host edit plus exact Undo/Redo, navigation source invariance, typed precision rejection with atomicity, candidate-context Fit and constrained Fit. It constructs no GPUI Application or native window and emits no physical acceptance verdict

Helper errors preserve partial files and a failure.json when possible, exit nonzero, and keep the failed stage distinct from subsequent native acceptance. Export files are independent canonical CPU outputs. They are never labeled as accepted SceneView pixels or native GPU framebuffer captures.

## Resource estimate and boundedness

The six specimens require 18 document nodes ordinarily, or 24 in the nested-clip preset. There are at most 18 authored stroke layers (the layer preset) and three CPU isolation domains. Paths contain only a handful of line segments. No images, text rasters, shadow filters or remote resources are admitted.

One 1040 × 420 RGBA8 canvas would be about 1.67 MiB at DPR 1 or 6.67 MiB at DPR 2. This is a sizing comparison, not an allocator measurement: isolated domains, intermediate buffers, upload retention, tessellation and GPUI overhead can add to it. Production CompositeLimits remain unchanged. Pending CPU work follows production bounds; this fixture does not expose or claim an allocator ledger.

Largest CPU image is 1800 × 820 at scale 2 and DPR 2: 5,904,000 RGBA bytes (about 5.63 MiB). Five exports contain 2,329,425 output pixels total after rounding, about 8.89 MiB of decoded RGBA across all files. The tour decodes one export at a time. Encoded bytes and working scratch are reported/limited by existing production APIs, not estimated as zero. Stored scan profiles are bounded by the fixed sample radii. Eight frame commands, 16 edit pairs, 64 recent actions, 64 pointer traces and 256 native receipts prevent an unbounded fixture history; process/session counts are determined by the 14-row tour.

## Integration/verification owner checklist

Review and compile after the final production API changes. In particular, confirm the source parser's error type, HitPolicy::Painted, Bounds/Viewport re-exports, command error variant, geometry APIs, Window::scale_factor, rounded frame defaults and InputTrace event shape. Compilation, helper and CPU outcomes are retained in the linked validation records; native acceptance remains a separate gate.

Run formatter and appropriate checks under the repository's existing workflow, then the finite helper and CPU tour, then the 14-row physical tour. Record exact source and executable hashes separately. If a helper finds a constructor admission error or public API drift, fix the draft rather than weakening production admission. If Fit/constraints, retained geometry or readonly reset behavior fails, report that bounded failure instead of bypassing it with direct viewport writes or granting Edit.

This increment does not reopen existing APP-02 outliers, broaden arbitrary local-shear/interchange support, certify all caps/joins/shapes, introduce font or producer claims, or establish broad performance equivalence. Existing acceptance text and previously retained failures stay intact.
