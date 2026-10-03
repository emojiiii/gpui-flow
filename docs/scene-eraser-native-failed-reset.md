Historical failed-run report. Its functional failure, source and crash records remain valid. A later capture-routing incident overwrote the old initial desktop screenshot: seven of eight old JPEG originals remain. The original two action/capture JSON files were restored to their exact sealed hashes. The report's initial full-capture-integrity statement is superseded by [the later integrity account](scene-eraser-native.md#evidence-integrity-incident). No replacement screenshot was generated. The original report body follows unchanged; original SHA-256: `856cf5032a823a2a65c893821b6d78948e3834bc82b62a5db12bdc7fe1976376`.

# Scene eraser native acceptance: failed at preset reset

The 15-row native gate did **not** pass. A real click on the **A Order + clip** preset button crashed the frozen process with `CapabilityDenied(Edit)` at `examples/studio/src/bin/scene_erasers.rs:412:45`. It exited **101** at **05:23:40 UTC**, rather than closing normally. The desktop was released immediately. No restart, limit change, production edit or build was performed.

## Exact reproduction and evidence

Started the frozen `run.sh` through the cloud Linux Application Finder at 05:21:44 UTC. In A, recorded the default live state and clicked the five world probes. Clicked Opacity twice (1→0.5→0.01), exporting each independently to CPU PNG/WebP. Switched Pixel→Path at opacity0.01 and recorded the successful live state. Then clicked A preset reset at desktop (163,141), recorded action completion time05:23:40.546Z. That invokes `Lab::preset`, whose line412 unconditionally unwraps `view.set_scene(document(case),cx)`.

`native.stderr` retains the original panic: `called Result::unwrap() on an Err value: CapabilityDenied(Edit)`. The same frozen fixture sets `set_editable(false)` at initialization. This is a source-grounded explanation of the failing reset route, not a production-runtime patch or proof that other controls would fail.

Last native state: `06-a-path-low.jpg`, A Path0.01 with blue contour hole, surviving green and transformed rounded clip. `07-a-path-one.jpg` is the original capture showing the browser underneath after the process disappeared. Its filename and the action labels were intended next actions, **not evidence that Path1 ran**. All original bytes and logs remain unchanged.

The next Path click was already queued in the same tool call after reset, before observing the crash. A later Live click and paint click also landed after exit. These three recorded click intents establish no accepted fixture action. There were14 clicks up to and including the failing reset (five canvas probes, eight successful host-button calls, one failing preset) and three post-crash click intents. No receipt after sequence5 exists.

## Completed physical subset

Rows1–3 passed their bounded observations; row4 completed only Path0.01 before failing on reset. Rows5–15 and the remaining Path1/no-paint/Pixel portion of row4 were **not executed**. B isolation, C masks, D Domains recovery, history, bursts and resize are therefore unaccepted in this native package.

Five saved live receipts consistently report pending=false, no composite issues, render issues `[]`, available presented geometry equal to canonical geometry, zoom1 and pan0. Native A default shows blue through the ellipse, green surviving as a later sibling, and blue through the transformed right eraser only inside the rounded clip. Pixel0.5 visibly yields a purple partial hole; Pixel0.01 leaves a near-red target; Path0.01 clears the full filled contour and excludes the wider Pixel stroke.

| Actual world click, within0.02 units | Router target | Presented and canonical geometric candidates |
| --- | --- | --- |
| (150,140) | cut | cut, earlier, external |
| (200,190) | later | later, cut, earlier, external |
| (410,150) | reach | reach, external |
| (715,235) | clip-cut | clip-cut, clip, clip-external |
| (530,345) | clip-external | clip-external |

All five pointer traces have no router errors. Every saved canonical Scene, Engine and presented Scene probe list agrees. The actual parent bounds are (60,80,280,200), and reach bounds(390,130,40,40), showing the reachable eraser outside the parent's positive bounds.

The desktop capture is1364×1024; the initial client is approximately[92,120,1272,932]. The canvas origin is(92,246), corroborated by actual adapter world-coordinate traces. Viewport zoom1 is recorded; the requested row15 display-scale/resize test was not reached. The final explanatory footer line is clipped at the lower edge in this initial window, while live status and pointer rows remain readable. This layout limitation is separate from the reset panic.

## Independent CPU export and audit

The two physical CPU-export button activations produced two real PNG/WebP pairs through `SceneView::export_scene`, each1000×430 with an explicitly owned opaque yellow background and no view grid. They are **independent canonical CPU exports, not native GPU framebuffer readbacks**.

Pillow independently decoded both formats and found exact RGBA equality, dimensions and FNV receipt agreement; all alpha values are255. The A hole and transformed clip-interior export samples are[128,0,127,255] at opacity0.5 and[252,0,3,255] at f32 opacity0.01. Later green remains[0,220,90,255], and outside-parent/clip-outside blue remains[0,0,255,255]. Exact color arithmetic is established by these CPU files, not the JPEG captures.

`native-partial-audit.json` has71/71 partial consistency/hit/export checks. Its native acceptance gate explicitly remains false. The audit uses explicit f32 representation0.009999999776482582 for opacity0.01; an initial Python f64-literal comparison was corrected as documented in `audit-method-note.txt`. None of this changes the crash result.

The preexisting headless tour's7/7 cases and11 encodes remain separate evidence. It did not create an Application or test preset switching. No headless pass is promoted to native acceptance.

## Provenance and original files

- Binary `scene_erasers`:179,934,736 bytes; SHA-256 `d7cdafaa5b648e5be72120e9edf6f5966f40c64350e5c0feb9e879c7bfa74892`
- `source-snapshot.tar.gz`:1,322,621 bytes; SHA-256 `23665f4df489f96a3befae017341b00a244084028e03dc99dbb8e836a539c5a1`
- All494 repository input hashes independently matched actual-link before/after and archived bytes. External compiler/dependencies/system libraries are outside that scope. The manifest's source publication is pending; any later source receipt is separate
- Session `session-data/gpui-scene-erasers-507707-1791004905180518694`:five JSON receipts and four encoded image files
- Eight original desktop captures have JPEG magic and `.jpg` names. They are qualitative visible-state evidence; no converted screenshot derivatives were made
- Raw `native.stdout` has five pointer records and five receipts. `native.stderr`, `native.exit` and `native.receipt` preserve the abnormal exit

The final space and file sizes are in `native-summary.json`. `screenshots.sha256`, `session-evidence.sha256` and `native-evidence.sha256` seal original observations and the partial report. Further native coverage requires a new frozen fixture fixing the reset route; this failed package must remain intact.
