# Bounded grayscale native fixture

> 历史验证记录：旧 Studio 原生演示已于 2026-10-05 移除。本文的源码路径、命令、交互矩阵与结果仅对应当时版本，不是当前可运行入口或新工作台验收。当前入口见[入门](getting-started.md)，测试保留范围见[迁移说明](studio-retirement.md)。

This contract defines the finite fixture. Compilation, helper results and physical acceptance are recorded separately; this document alone establishes no pass. The original unrun author draft remains preserved outside the repository.

Fixture file: `examples/studio/src/bin/grayscale.rs`. The draft uses the current Studio dependencies (`gpui`, `gpui-kit`, `gpui-flow`, `serde_json`, `image`) and default binary discovery. No Cargo change is proposed. No new dependency, font, remote resource, private renderer access, or general export wrapper is needed.

## Runtime contract and source grounding

The scalar field is `Style.grayscale: f32`, default `0`, finite in `[0,1]`. The runtime worker confirmed `CompositeError::UnsupportedGrayscaleBackdrop(DisplayId)`. An active pass runs once after the complete node, including its own colored outer/inner shadows and its children, before local opacity/blend/inherited clip. Alpha is unchanged. Child passes precede ancestor passes.

The draft follows the public APIs and host patterns already used in `scene_erasers.rs`, `scale_fixed_strokes.rs`, `outer_shadows.rs`, `inner_shadows.rs` and `container_inner_shadows.rs`. In particular:

- A preset creates a new `SceneEngine` and readonly `SceneView`, then replaces both observation and input subscriptions. It never calls readonly `set_scene().unwrap()` or routes an edit command through the readonly host
- Host changes use `editor.transaction`; Undo/Redo use real `SceneEditor` history and compare complete canonical authored JSON
- Native pointer evidence comes from `SceneViewEvent::InputTrace` and the actual `PointerDown` operation. The separate public geometry query uses the adapter's four logical screen-pixel tolerance converted by the traced zoom; strict queries use default hit options
- Public presentation records include canonical source, source fingerprint, engine revision, entity identity, fixture token, actual viewport, actual window DPR, pending/presented status, presented amounts, full composite diagnostics, render diagnostics and sticky interaction error
- The internal publication generation is not public. The fixture records the above public correlation evidence and makes no claim to have read or checked private generation IDs. Controller/unit tests must establish exact generation rejection
- Successful commands do not imply that the existing `interaction_error` field was cleared. Receipts preserve it. A new preset gives a genuinely new interaction session
- All preset resets, navigation buttons, direct amount buttons, history actions and invalid-input checks retire the burst token. The callback checks its token before changing any queue flag, source, history or log. One burst schedules at most eight native frames and ends at amount 1. Observing a preset after an already finished burst does not establish cancellation timing

The target native window is 1060 by 770 logical pixels. Three 36-pixel control rows, a fixed 1040 by 420 canvas and a fixed 190-pixel footer total 718 pixels. The operator must confirm the entire footer and controls fit in the actual window; a source-level size calculation is not a layout pass. The widget is the existing shared `ControlButton`.

## Five bounded presets

| Preset | Visible fixture | Amount buttons change | Variant button |
| --- | --- | --- | --- |
| A Leaves | Three columns 0 / .5 / 1; each contains opaque R/G/B. Bottom shows unfiltered/filtered own colored shadows and unfiltered/filtered half-alpha red | Right primary column, `low1`, `shadow1` | No change |
| B Group | Left Normal group at gray 1 and opacity .5; right matching gray-0 reference. Opaque red/green overlap, earlier outside blue and later outside magenta | Left `focus` group | Left opacity .5 / 1 |
| C Nested | Box and Frame inside fractional rounded ancestor clips; child gray .5, parent gray .5, colored child/own shadows and parent stroke | Box and Frame parent amounts | BeforeChildren / StrokeAfterChildren |
| D Alpha | Three Normal gray-1 parents: blue half-alpha mask, blue half-alpha Pixel eraser, and Path eraser. Red source and later green child | Only mask / Pixel / Path source amounts | No change |
| E Reject | OwnedCanvas with red earlier root, full-opacity PassThrough gray-1 group, blue Multiply descendant and later magenta root | `focus` group | PassThrough / Normal |

A uses exactly eight active compositing domains at its maximum: six nonzero primary leaves, one half-alpha leaf, and one identity Normal gray-0 group containing both shadow controls. The gray-0 shadow control still needs compositing; the shared identity parent avoids a ninth domain and preserves both controls' world positions and independent amounts. It does not raise the default domain ceiling. CPU and native runtime calls use unchanged `CompositeLimits::default()`. If admission rejects a fixture, that is a fixture/runtime finding; reduce legitimate fixture work or fix the defect rather than increasing limits.

## Independent numeric expectations

For premultiplied bytes, in f64:

`Y = (2126R + 7152G + 722B) / 10000`; each RGB result is `round(c + amount * (Y-c))`, defensively clamped to `[0,A]`; alpha is unchanged.

`--cpu-tour` first creates six tiny real vector scenes at amounts 0/.5/1, captures them through public `capture_subtree`, rasterizes through public `rasterize`, and compares interior **premultiplied** bytes to this independent formula. It also checks budget retained bytes return to zero after `discard_frame`. Inputs include opaque primaries, half-alpha red, half-alpha orange and premultiplied gray.

Exact examples include red 1 → `[54,54,54,255]`, green 1 → `[182,182,182,255]`, blue 1 → `[18,18,18,255]`, red .5 → `[155,27,27,255]`, half-alpha red 1 → `[27,27,27,128]`, and `[128,64,0,128]` at 1 → `[73,73,73,128]`.

Separate 960 by 380 PNG exports use a transparent background, scale 1 and synthetic DPR 1. The runtime export is immutable, and `image::load_from_memory` actually decodes each encoded PNG before audit. Receipts record MIME, encoded byte count and fingerprint, decoded dimensions, decoded RGBA byte count/fingerprint, source invariance and actual sampled pixels. PNG decoded bytes are straight-alpha; they are not the premultiplied working buffer.

Additional assertions are concrete, not merely before/after equality:

- A opaque primary samples follow the formula at 0/.5/1 with exact bytes. Half-alpha red decodes near `[54,54,54,128]` (one-byte demultiplication tolerance). Opaque own magenta outer shadow becomes `[73,73,73,255]`; opaque own red inner shadow becomes `[54,54,54,255]`
- B left red-only sample is approximately `[27,27,154,255]`; opaque green over red in the same group is approximately `[91,91,218,255]`, with one-byte source-over rounding tolerance. These derive from filtering the final opaque group result and applying opacity .5 once over blue. Per-child opacity would produce a different overlap. Earlier blue and later magenta are exact original colors
- C red child .5 gives `[155,27,27]`; parent .5 then gives `[105,41,41]`. Both child interior probes require exact bytes. The right-edge stroke probes are red-derived `[105,41,41]` with BeforeChildren and magenta-derived `[164,36,164]` with StrokeAfterChildren. A point outside the fractional rounded ancestor clip must be transparent
- D mask coverage produces gray near `[54,54,54,128]`. Pixel destination-out leaves gray near `[54,54,54,127]`; Path erasure gives transparent black. Later surviving green content becomes `[182,182,182,255]`. Only then is the stronger complete decoded-byte equality assertion applied to source amount 1 versus 0
- E unsafe export must yield exactly `UnsupportedGrayscaleBackdrop("focus")`, preserve authored source and publish no output PNG. Explicit Normal recovery yields gray-blue `[18,18,18,255]` while earlier red stays red

These samples deliberately avoid ambiguous antialiasing edges. Full edge-alpha invariance, all work/admission thresholds and low-alpha upload precision remain focused production-test responsibilities. Native screenshots cannot establish these byte values.

## Finite physical matrix: 15 required rows, one optional reopen

Each row is a physical procedure with an explicit operator verdict. Preserve every failed or inconclusive observation. A row may contain multiple checkpoints but does not authorize unbounded repetition. Record settled states after `pending=false` where success is expected, and capture the actual rejection state in E. If pending does not settle within a reasonable observation window, preserve it and investigate rather than declaring pass.

| Row | Actions and required observations |
| --- | --- |
| 01 | Fresh A at zoom 1. Inspect all 0/.5/1 columns, low-alpha patches and both shadow sets. Confirm footer fits. Click opaque `r2` away from an edge; trace and strict query should name `r2`. Record actual DPR and viewport |
| 02 | A Amount 0. Wait for the new presentation; filtered right/bottom targets regain their authored colors. Middle .5 column stays .5. Record source amounts and absence of stale old gray presentation |
| 03 | A Amount .5, then Undo and Redo. Each settled state must match its source; history records must show exact canonical JSON restoration. No source geometry or picking change is expected |
| 04 | With the safe amount edit in history, use Invalid model. Eight attempts cover negative/above-one/NaN/infinity while hidden or zero-opacity. Every attempt must reject with unchanged source/revision and unchanged immediate public presentation. Undo/Redo must still restore the safe amount edit exactly |
| 05 | A Zoom .5, then 2, then Home 1. Source and revision must remain unchanged. At 2 the canvas intentionally crops the world; that is not a missing-content failure. Click a visible primitive after returning Home and compare actual traced zoom/tolerance and strict queries |
| 06 | Fresh A, use 8 amount frames once; allow it to finish. Record final amount 1 and a settled current presentation. No stale colored/gray state may replace that accepted final source later during this row |
| 07 | Start one second burst, then select fresh B promptly. Record the actual retired remaining count, new entity/token, stable current presentation and a new native click. If the burst had already finished, record cancellation timing as unobserved. Do not repeat indefinitely trying to catch it |
| 08 | Fresh B default .5 opacity. Inspect left/red-only and left/overlap versus the right reference. Earlier blue and later magenta retain color. Click `focus-green` in the overlap, then Record |
| 09 | B Variant to opacity 1; inspect once-only opacity recovery, then Undo to .5 and Record. History/source correlation and geometric targets remain intact |
| 10 | Fresh C. Inspect both completed parent filters, colored child effects transformed by the parent, differing Box overflow/Frame clipping, and fractional rounded ancestor clip. Click each child interior and Record |
| 11 | C Variant moves own inner/stroke to StrokeAfterChildren. Inspect the right edge where the magenta-derived filtered stroke now overlays opaque child paint. Undo restores the earlier phase. Record both checkpoints |
| 12 | Fresh D. Inspect half-alpha mask, half-alpha Pixel cut and fully cleared Path cut, plus later gray survivor. Click interior Pixel and Path geometry; actual traces may target `pixel` and `path` despite removed paint. Record strict/tolerant query results separately |
| 13 | D Amount 0. Coverage, holes and surviving colors must not change; the unchanged parents still filter the surviving contents. Undo to amount 1, then Record. Exact full-image identity is a CPU-export assertion, not a JPEG visual claim |
| 14 | Fresh E. Preserve the typed `UnsupportedGrayscaleBackdrop` diagnostics. The retained source is valid, but the complete OwnedCanvas presentation must reject; no unfiltered substitute counts as success. Record the actual pending/presented/error state |
| 15 | E Variant selects Normal. Inspect successful gray-blue child plus unchanged external red/later magenta. Click the child and Record. Undo should restore rejection; Redo should restore valid presentation. Preserve errors from every checkpoint, including any sticky interaction error |
| 16 optional | Close normally once, relaunch once with a distinct parent-supplied reopen log. A fresh invocation must create a unique evidence session and fresh A view/history/input. Record full-footer fit and one actual native click. Mark skipped if not performed |

The two bursts total at most 16 requested frame callbacks. This matrix makes no exact in-flight cancellation timing claim. Session-token safety is inspectable in source and requires controller tests; physical evidence only proves what its recorded state actually observed.

## Pure helper, evidence routing and limits

`--headless-tour` returns before constructing a GPUI Application. It validates all five builders and uses the actual public `scene_units` / `SceneUnit` planner to audit domain counts and errors. A–D permit no rejected unit and no domain-limit overrun. E requires the exact grayscale-backdrop rejection; a separate `owned_canvas` call requires the same typed failure inside OwnedCanvas and successful capture after explicit Normal recovery. This checks capture/planning, not raster work-budget admission. It then demonstrates the real readonly host denies LoadScene, accepts one trusted host amount edit, rejects invalid hidden/zero-opacity values atomically, keeps authored source invariant under three viewport commands, and checks exactly one Undo/Redo. It cannot exercise native input, platform window callbacks, display upload or visual presentation.

The parent should first inspect/refine this source, compile it and run the helper/CPU tour through the existing project workflow. Draft authoring is not execution evidence. The parent must supply the existing absolute `FLOW_GRAYSCALE_EVIDENCE_DIR`, plus distinct exclusive main/reopen console-log paths if the optional reopen is done. This draft does not choose a repository-relative output folder or overwrite any evidence.

Every invocation creates a unique process-ID/nanosecond session directory with exclusive `create_dir`; every file uses `create_new`. Main, reopen, CPU and helper runs therefore cannot silently overwrite each other. Native actions and actual pointer events are logged immediately to numbered files and stdout; full diagnostic history is not kept only in a lossy ring. An evidence write failure is visible and existing partial files are preserved. Each invocation limits file bytes to 8 MiB and numbered records to 192.

The planned CPU tour stores seven small PNGs: A, B, C-before, C-after, D-one, D-zero and E-Normal. The runtime limits remain unchanged. Store the original encoded operator screenshot files, preferably within a parent-managed combined ~30 MiB evidence budget. If the screenshot provider returns JPEG, preserve that original encoded JPEG; do not rename it PNG, claim it is a framebuffer or RGBA capture, or fabricate decoded/native pixel hashes. CPU export PNGs and decoded hashes remain explicitly separate from native screenshots.

The event writer prints a JSON event to stdout before attempting its exclusive file write. The parent must match stdout events one-to-one against saved numbered event files and preserve any write errors. A printed event alone does not prove successful evidence retention and cannot count as a passed saved record. The parent should audit all files actually produced and report counts, total bytes and original screenshot encoding from real evidence. The 8 MiB program cap covers files written by this binary, not shell logs or operator screenshots. No claim of a 30 MiB total can be made until those external files are included.

## Scope and known limitations

- There is no optional Image, Canvas or native Text producer case in this bounded fixture. Their grayscale behavior is **tests-only** until the corresponding producer tests actually run; no physical producer coverage is claimed. Existing font/resource bridges are not installed by this binary
- No native framebuffer, GPU readback, native/CPU pixel-equivalence claim, browser comparison or cross-platform pass is provided
- Screenshot visuals cannot prove exact channel bytes, alpha invariance, budget parity, or exact cancellation timing. Those require independent CPU and production-test evidence
- SVG rejection, strict Leafer hidden-state rejection, direct lower-level native rejection, StatePaint retention, damage/bounds/hit invariance, alpha-edge identity, consumer export formats beyond PNG, budget thresholds and controller generations belong to the runtime's focused test set
- Assertions are designed from the written grayscale and existing compositor contract. Until compiled/executed, source-level API review and arithmetic are the only verification performed here. Any oracle mismatch must be investigated, recorded and fixed explicitly; do not silently widen tolerances or label a draft run passed
