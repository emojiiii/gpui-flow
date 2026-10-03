# APP-02 fixed acceptance audit — 2026-10-03

This is a source/contract audit by the integration owner, not a second independent reviewer or a native acceptance report. Current published source is 981e991817b4465112341b2d2b7ad680bea24920 and its frozen fixture is 369d250c29da388777d50045abdeebd434ad168c. Native main/reopen exited successfully; the sealed report accepts the fourteen bounded physical rows with 265/265 independently calculated checks by the same QA operator. See [the original report](scale-fixed-native.md).

## Original contract and unchanged scope

The first ledger commit in the retained Git history is local 8d98280030ee1f908a4819daa83e55ad63b73256, titled “Local checkpoint matching remote 05c92dcf”. A fresh connector GET resolved the original remote to 05c92dcfea58c5bd914808e0b629662ea4fb1e28 with tree 312b308f2bd97031f31cb4e3da2775b099f86304, exactly the retained local commit tree. Original JSON APP-02 starts at 391, original MD row 45; bounded status definition is MD line 13 and fixed-scope rule MD line 15. Original ledger JSON SHA-256 99592ef08fd025ad6487eb75314d30d8cb070cd367098d790133e6c5e60f083b. Its APP-02 acceptance string is byte-identical to the current ledger:

“cap/join/miter/dash、闭合及开放轮廓内外对齐、scale-fixed；bounds/hit/paint同源”

The original boundary says “现有居中及闭合inside/outside；开放和scale-fixed缺”. The original reference is docs/stroke-alignment.md; the upstream reference is IStrokeAttrData/IStrokeInputData at pinned UI1e11641d3ea6b0211c83447268a90b8d58feec25. No local-shear, every-backend/export format, every Cartesian combination, native pixel-equivalence, or arbitrary precision requirement is listed in this row. The current remaining_frozen_scope=[scale-fixed stroke] was introduced after open-contour acceptance, not by the new fixed-stroke implementation.

The original ledger MD introduction states that bounded_verified means the supported set in the row has actual evidence, not full upstream-module parity. It fixes the finite acceptance set and explicitly separates the older 39 cross-reference contracts. Those older contracts include D03 matching transformed clip/render/picking policy with visible unsupported-backend diagnostics, D04 continuing gradients/budgets/cache/zoom/DPI tests, and B03/B07 width invalidation and old/new damage bounds. They do not add an arbitrary-skew scale-provenance representation or make every consumer format mandatory for APP-02.

## Mapping every listed behavior

- cap/join/miter/dash: existing shared Kurbo/Lyon mesh and 2026-10-02 automatic/native open-stroke evidence; fixed width is resolved before that same mesh and leaves local dash/phase unchanged
- closed/open inside/outside: existing mask-only implicit closure, combined winding and shared Boolean stroke mesh; fixed policy uses the same path, with existing width/precision/work bounds
- scale-fixed: per-stroke disabled/fixed/numeric threshold; pinned scalar-X compensation, strictly greater on either ancestry axis, per-layer policy; ancestry local-column magnitudes are accumulated before camera scale and kept separate from DPR
- bounds/hit/paint same source: canonical camera1 versus explicit current-view derived scene, resolved-object guards, matching renderer/CPU/hit meshes, current-view selection/handles/drag/snapping/Fit, ancestor invalidation and accepted-scene coherence. The final suite has 1,679 default passes/10 existing ignored and 275 UI passes, strict checks and independent static review. These are retained runs, not rerun by this audit
- physical gate: fourteen-row current fixture observes fixed/threshold/disabled widths, anisotropy, ancestor90/reflected35 resulting world shear, dash/caps, open alignment, independent layers/rounded clips, host Undo/Redo, Fit/constraints, atomic typed reject/recovery, new sessions and close/reopen. Final native report is required before status changes

## Limitations that must remain explicit

Supported fixed ancestry consists of locally orthogonal invertible translation/rotation/reflection/independent-axis scale chains, including sheared world matrices produced by those chains. Material local shear lacks authored scale provenance and rejects. Rotated numeric Y-threshold ambiguity rejects; it is not guessed or silently clamped. A raw CPU fixed camera is exact positive uniform zoom plus translation. Authored object rotation remains supported. Geometry, cumulative work and depth limits remain enforced.

Strict Leafer interchange, SVG fixed strokes, fixed Canvas-subtree capture and ItemBox::Stroke fixed/derived inputs are explicit unsupported consumers. Advanced Text/Image/Group styles and Custom CPU retain their previous limits. Imperative Canvas commands remain centered local strokes; these are not silently reclassified as the retained vector-stroke API. No all-format, all-shape, arbitrary-affine, cross-platform or backend byte-equivalence claim follows.

Keep the prior open-stroke auxiliary 20/21 decoded-JPEG bbox observation and isolated low-contrast outlier, whose cause remains unknown. This was auxiliary screenshot comparison, not a proven shared-mesh failure or an original exact-pixel parity gate. The actual old captures are lossy JPEG despite historical PNG suffixes.

The current scale-fixed native run observes sticky interaction_error after successful Home. Source review proves execute_command success/apply_input_report never clears the field in either pre-increment 7d12f8b or current source. Record settled geometry and retained historical error separately; do not invent an intentional policy promise or say the banner cleared. The physical second burst completed its 8 frames before reset; only later fresh-session/input stability was observed. This fixture’s queued callback token guard is source-reviewed only in this run; product generation/navigation cancellation has separate automated coverage. Neither is a physical timing claim for that second burst.

## Status decision

The final native audit accepts the finite matrix without a new functional mismatch. APP-02 becomes bounded_verified under its unchanged original acceptance and the above precise supported domain. This closes the last named required behavior, rather than deleting or renaming a required capability. Existing numerical/backend limits are visible capability gates. Do not change acceptance text or retroactively relabel old evidence. This update makes 33 bounded_verified / 9 partial / 3 not_started, still 45 total.

Dated current-state corrections accompany this decision for ledger verification_limits “scale-fixed remains absent”, docs/stroke-alignment.md current-scale-fixed sentence, and the source map's current stroke availability. Historical dated reports remain intact and can receive a link to newer evidence without rewriting their original observations.
