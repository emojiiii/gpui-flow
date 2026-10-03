# APP-04 shadow modes: independent source review

Review started 2026-10-03 11:31 UTC. Read-only review of `/workspace/shared/gpui-flow` against the accepted shadow contract and design notes. No Cargo, build, test execution, native window, Git mutation, repository source edit or repository documentation edit was performed. The foreground-blur 1049Z package was not touched.

The checkout HEAD read at the start was `802f7cdd36a821e1a6c162eb5a47a7b07f286b20`; shadow changes were uncommitted and changing concurrently. This is not a release approval or independently executed test receipt. The owner's fixed-source manifest was reviewed at 11:42 UTC, the next source-input manifest at 11:47 UTC and the final test-only delta at 11:53 UTC, as recorded below. No unresolved P1/P2 source defect remains in those examined bytes. This report is sealed after the 11:53 review.

## Findings reported promptly

### P2: hidden reference geometry bypasses input limits

Status: source fix reviewed and closed against the 11:42 fixed-source manifest. Capture now charges the hidden root's own reference inputs. Preparation propagates reference need and revalidates the hidden root/descendants against actual raster node, depth and cumulative/per-node point limits, without resource preparation or double counting its already-counted root. Owner-run regressions reproduced failure and then passed. The additional multiple-hidden-sibling cumulative regression was inspected and its owner-run pass read at 11:47 UTC.

`plan.rs::capture` returns an invisible/zero-opacity node before charging its own Path/Pen/Polygon/Star point count. The new `capture_reference` charges only that hidden root's descendants. An ancestor spread Group nevertheless reads the hidden root's geometry through `vector.rs::reference_bounds`. Thus a directly hidden Path can exceed per-node `max_points`, and multiple directly hidden siblings can exceed `max_total_points`, while still influencing the visible ancestor shadow.

There is a second route requiring a preparation-time fix: a `DomainSource` captured under default limits can later be rasterized under reduced `PixelBudget` limits. `Preparation::node_at` returns at the hidden root before visiting retained descendants or counting its own path inputs, while `reference_bounds` traverses that geometry. A visible spread Group, visible ink child, hidden Group and hidden Path are four source nodes, but preparation sees only three nodes; a later `max_nodes=3` therefore does not enforce the requested cap. Hidden-chain depth and reduced point limits have the same problem. Capture-time checks alone are insufficient.

Requested regressions: hidden direct Path per-node exact limit/cap-minus-one; multiple hidden siblings cumulative exact limit/cap-minus-one; hidden nested node/depth/point limits when capture and raster limits differ; preserve producer skipping and identity. Count each retained geometry node exactly once and do not request hidden Image/Text resources.

Examined hashes: `plan.rs` SHA-256 `d6a6a30387403ac93854ce50fb32a5c4fb482e58f0364aa743cf5baaadef65fa`; `vector.rs` `51d6b7dd79ed9ee838e2548ce3e88594402c520e3dcbb20986e2c18a0aa7041d`.

### P2: positive subnormal blur silently becomes zero-filter bypass

Status: source fix reviewed and closed against the 11:42 fixed-source manifest. The original f64 sigma admission range is checked before narrowing, with positive authored blur unable to select the true zero bypass. Actual f32 sigma still controls support. Owner-run default/active-mode regression reproduced failure and then passed.

`shadow.rs::FilterPlan::parameters` casts the computed sigma to f32 before checking `sigma != 0 && sigma < 0.001`. With authored `blur=f32::from_bits(1)` and identity transforms, the positive f64 sigma is `2^-150`, which rounds to f32 zero. The new guard therefore accepts the effect as a true bypass, whereas the previous positive sigma lower-bound guard rejected it. Zero is reserved for authored zero blur, and the accepted device sigma minimum remains 0.001. Retain support/kernel derivation from the actual f32 sigma but reject positive authored blur narrowing to zero.

Examined `shadow.rs` SHA-256 `078826d4c2cd75ea8d4a21ef6eef94ec473be66ddac0b845ce2d3a3b18d39334`. This finding is proved by source/arithmetic, not an executed regression.

### Compatibility correction: preserve default physical-shadow parameter rounding

Status: source fix reviewed and closed against the 11:42 fixed-source manifest. Default physical shadows once again use the former matrix-narrowing path. Owner-run parameter regression failed before and passed after this change; no differing output pixel was executed or claimed by this reviewer.

Before the change, ordinary shadows in a positive foreground-blur domain used `FilterPlan::matrix(shadow, a.local(...))`, so the composed physical affine was narrowed to f32 before parameter calculation. The first mode implementation changed every physical shadow to f64 affine composition, before deciding whether any new mode was active. This changes the offset/sigma parameter boundary even when all new fields retain defaults, conflicting with the agreed legacy-rounding preservation. Owner will keep the former matrix parameter path for mode-default layers and the new f64 logical/device parameter path for active modes. Actual-f32 sigma support derivation and the intentional A8 correction remain required.

## Source paths checked without finding another defect so far

- The mode plan separates original capture N, transformed/filter M and output Q. Original RGBA and A8 remain live through original-alpha knockout/inner coverage; no oversized union surface is allocated merely because projection is far from source.
- Spread uses canvas-axis signed factors around transformed owner-reference geometry. Resampling evaluates output pixel centers and subtracts the original capture origin plus 0.5 before A8 sampling. Final offset/original-alpha sampling uses the equivalent pixel-index convention consistently. Negative factors use min/max transformed endpoints. Exact axis collapse avoids inversion: outer has no output; inner retains and tints original alpha.
- The source is replayed through `contents(..., draw_shadow=false)`. Own outer/inner effects, foreground blur, opacity, blend and inherited clips do not feed the capture. Descendant completed effects and child clips/masks/erasers still execute. Original A is sampled at final, unoffset coordinates. Box uses rounded fractional-alpha attenuation; inner box is inert.
- Admission and runtime use the same immutable plans, bytes and work methods, identical outer-collapse early skip, recursive source replay and destination-composite charge. Mode reservation is `5N + FilterPlan.bytes(M) + 4Q`; original initialization/extraction, transformed sampling, filter and output tint/composite are charged. Cancellation is checked per extraction/resampling/tint row and around the A8 filter. Working-set restoration remains in the public raster wrapper.
- Device resolution runs before destination viewport clipping; parent reference planning is not gated on legacy optional ink bounds. A descendant's entering blur halo can provide source support to a paintless Frame whose declared reference box stays fixed.
- Canvas now carries `logical_effect_camera = local_to_canvas * inverse(source.root_transform())`, separate from the DPR-packed raster matrix. Export carries request.scale separately from pixel_ratio. Runtime effect scale excludes DPR and source/capture origin translations. The existing stricter fixed-stroke camera checks and Canvas subtree refusal remain in place.
- Recursive equality and placement equality include all Shadow fields and the new logical affine. Retained hidden geometry participates in source identity. Image/Canvas/Text resource traversal still stops at hidden/zero-opacity ancestors. Complete-source text requests continue to propagate visible ancestor shadow/inner/blur effects.
- Outer effect world bounds use a conservative circle/operator-norm envelope over canvas-axis spread orientations and scale-fixed compensation, with an owner-local reference separate from descendant effect bounds. Actual raster destination bounds come from device-completed support whenever modes are active, not the coarse legacy preparation extent.
- SVG rejects visible outer/inner shadows through its existing explicit refusal. New fields retain strict serde unknown-field rejection and booleans; the model validates finite spread within ±4096. Published surface/node/depth/point/work ceilings are unchanged.
- The A8 one-tap branch shares the foreground branch's exact adjacent-f32-weight predicate. Positive-sigma filter work now includes `64*K*K` for legacy shadows as well as modes, so default-mode near-budget inputs can reject earlier even when their pixels would otherwise stay unchanged. The final documentation must distinguish that conservative admission change from pixel compatibility and the intentional tiny-sigma byte repair.
- Locked image source was also read: `get_gaussian_kernel_1d` evaluates the same adjacent-weight expression; `prepare_symmetric_kernel` removes front/back zero entries through repeated vector removal, explaining the quadratic allowance. The generic GrayImage call converts one channel to two f32 planes, uses the bounded ring/full-plane path, then produces A8. Complete filter surfaces have the halo on both sides, so ring warmup is bounded by the padded surface height. Examined image 0.25.10 `sample.rs` SHA-256 `f5dd7c0697010b8f5e8dd6d92b82294dd38ed364b8df6a575ca0b5fa8c212641` and `filter_1d.rs` `03074917710ef7c899d63e18f6249b6b389f721214e82d2d73ee0ca3cc695b66`.

## Initial examined production hashes

| File relative to checkout | SHA-256 |
| --- | --- |
| crates/flow-composite/src/shadow.rs | 078826d4c2cd75ea8d4a21ef6eef94ec473be66ddac0b845ce2d3a3b18d39334 |
| crates/flow-composite/src/blur.rs | eaad4b3b58728ae846d768076d66bdac4e798fc1e61230bb2eb51d1a3135b184 |
| crates/flow-composite/src/raster.rs | 4c92478f321118f69058390305f78106e637e771eb0beb24919cae8fc0558291 |
| crates/flow-composite/src/raster/admission.rs | 92c61481258e265e8638bca54515ebeda44d53610a4d692e2a995c4e0d20df49 |
| crates/flow-composite/src/plan.rs | d6a6a30387403ac93854ce50fb32a5c4fb482e58f0364aa743cf5baaadef65fa |
| crates/flow-composite/src/vector.rs | 51d6b7dd79ed9ee838e2548ce3e88594402c520e3dcbb20986e2c18a0aa7041d |
| crates/flow-composite/src/canvas_surface.rs | 3a4ce2dfbebe3a0932197a4d81bd94a107ad55a4807c2a1c818555e8e8f258de |
| crates/flow-composite/src/scene_export.rs | 834fe0d4c419388543bdc6e38c276fddaa8f97d4fd4092d168501349c078f655 |
| crates/flow-composite/src/lib.rs | 73c60bfe4b8204929412790b6909abab4d332235ec9a218a6f7a1244b84174f4 |
| crates/flow-scene/src/style.rs | 21c98045fcdb55af2aeb088c54c084b99afb41737920599a2294fcc3f4d61e3d |
| crates/flow-scene/src/object.rs | 9b8ec24d11372cc244a34fee3af6d8936a64652750cba19652ca202d34d68fc0 |
| crates/flow-scene/src/scene.rs | a52dfac52342616468eb934c0b70abdfc34531b3fef19b99f57ec2f4f1aed247 |
| crates/flow-engine/src/layout.rs | d6a6ad8490e61bcf87ee44c61e162fdce803f1640cc3315c6c653e4254ff73ff |

## Remaining review conditions

Source review is complete against the 11:53 final manifest below, with no unresolved P1/P2 source defect. Any subsequent production changes require a delta review. Test source was inspected and owner receipts were read, but no tests were run by this reviewer. Parent owns broader compiler-input/assets provenance, documentation reconciliation, the unbuilt new native fixture, native evidence and publication; those are not certified here.

## Fixed-source delta review, 11:42 UTC

Read `/workspace/shared/shadow-modes-validation/review-fixed-source.json`, dated `2026-10-03T11:42:03.967367+00:00`. All 499 listed runtime/test/Cargo files were independently hashed and exactly matched the manifest, with zero mismatches. Declared aggregate: `da4a38dcdce45169046975b3d00e2d220ea9ea53d6eaedc39a659fe7f5949193`; manifest file SHA-256: `5a404162273ea5495bf162fe96724e59592b42ff4ec5e8b23029a025440b2238`.

Updated examined files:

| File | SHA-256 |
| --- | --- |
| crates/flow-composite/src/shadow.rs | 330cae78caa5032a9a511fb0e6e6977d146b4e64431d7d87f6c75df6b29d45af |
| crates/flow-composite/src/plan.rs | cb44f0e8b62c31db477e7273c584cae96861910804860be1974e492ae6e414f4 |
| crates/flow-composite/src/vector.rs | ba73135d5ce325b5108683a2591dad05c5f6f48e00713e813db897ed5348f4ad |
| crates/flow-composite/src/shadow_modes_tests.rs | b3ed4eb0f82ce7c4ea1d3b1be9073e9af9693d834ccacc6fb41157245a5d9b20 |

The owner's `review-prefixed-failures.log` shows four discriminating regressions failing before the correction: capture root-point limit, tighter raster geometry limits, positive-sigma zero underflow, and default physical parameter narrowing. `review-fixed.log` shows all four passing after correction. These are owner-executed receipts read by the reviewer, not reviewer-executed tests. Log SHA-256 values: `c3f13ce26521963c210398f01af795ab8c022d5796525925c2af2dde962d4a41` and `e325750211958a53444e0ff7a56fb7c2bfa7c7750a909d0a83b5733bc37c8a23`, respectively.

The pre-fix default-parameter regression records an actual offset difference: new f64 `[14.004755569574042, 1.5545618299173354]` versus prior narrowed `[14.004755301234098, 1.5545619285534826]`. The fix restores exact equality at this boundary.

## Final source-input delta review, 11:47 UTC

Independently rehashed all 499 entries of `/workspace/shared/shadow-modes-validation/final-source-inputs.json`, dated `2026-10-03T11:46:09.776737+00:00`, with zero mismatches. Declared aggregate: `d9adbb322da123441c00e7ee0787a51b3c2cfdb24dcd92125687897b784780b2`; manifest file SHA-256: `d4c50b5dc7308ade9c985bdb4dd7460f4c8ab56d46d1b7798f79702ed3dfa7f2`.

Relative to the 11:42 review manifest, only two compositor test files changed; all production code and Cargo files are identical. Inspected final `shadow_modes_tests.rs` SHA-256 `fe730973aee13adc2417f056341cb1e9a565844a8147ad243ed2edefcdaa200e` and `shadow_mode_consumer_tests.rs` `1dfa96ec7d5bc1b0966bb55c7c7ba487eafff35065d48c0ce35a3f062a4d6be6`.

`review_multiple_hidden_siblings_share_cumulative_reference_point_budget` now supplies two directly hidden three-point polygons. Per-node cap 3 and cumulative cap 6 succeed, while cumulative cap 5 rejects both initial capture and later rasterization of a default-captured source; rejection preserves retained bytes and zero pixel work. The owner-executed `20-composite-final.log` records this regression passing and the compositor suite completing with 456 passed, 0 failed, 0 ignored. This reviewer read that receipt and did not execute it.

The source owner reported broader strict checks still in progress and a possible UI test-only iterator lint adjustment. Such a test-only adjustment does not change this production source conclusion, but the publisher must preserve accurate final input/receipt provenance and review any production delta separately.

## Final cross-check and contract review, 11:53 UTC

The refreshed `/workspace/shared/shadow-modes-validation/final-source-inputs.json`, dated `2026-10-03T11:50:39.831283+00:00`, declares aggregate `201e73d2ace7b2d6f9a714f9c1aa79db14deb5c525d33de7d4ad4528205f2747`. All 499 listed files were independently rehashed and matched, with zero mismatches. Manifest SHA-256: `470cd4d0f83e1e9acb3680c91e1d351b66dad159c9f7242cfe03202b532dd055`. This is a scoped Rust/TOML/Cargo runtime/test inventory, **not** a complete compiler-input or asset manifest. The parent's planned broader asset/archive proof was not performed by this reviewer.

Compared with the full successful headless/UI snapshot `da4a38dcdce45169046975b3d00e2d220ea9ea53d6eaedc39a659fe7f5949193`, only these three test files differ: `shadow_modes_tests.rs`, `shadow_mode_consumer_tests.rs` and `flow-ui/src/display/composite/presentation_tests.rs`. The first two retain their 11:47 hashes above; final UI test hash is `332020e7f7e00f2dcbf130e27c2fe14dca01d71461cd7226eb9b1623b760213c`. Production source and Cargo inputs remain identical to that full headless/UI snapshot. The final UI text assertion uses `as_chunks::<4>().0.iter()` on RGBA data, equivalent to its prior complete four-byte-chunk iteration, and the same test was rerun successfully.

Read and cross-checked `summary.json`, `implementation-handoff.md`, `post-ui-test-only-delta.json` and the corresponding logs without executing Cargo or tests. Independently parsed receipt totals are 1788 passed/10 ignored for the full headless aggregate, 287 passed for full UI, 456 passed for the final compositor, and 1 passed for the focused UI text rerun. The final compositor includes one newly added cumulative-budget case beyond the 455 compositor cases in the earlier headless aggregate. These are separate receipt scopes; the repeated compositor/UI-text passes must not be added as new independent tests. Both strict-check receipts, format and diff-check receipts report exit 0; their logged source hashes are consistent with the declared test-only deltas. No new native fixture was built or exercised as part of this review.

Receipt/control-file SHA-256 values:

| File | SHA-256 |
| --- | --- |
| summary.json | 06b01e9672b05bfede9f45568e217cbb15154d209980f39a3a3fec62bfe8ca27 |
| implementation-handoff.md | 78cefe9324bb138b9fac82c197cc270548b93019377e79a93819b1dbbe2e336f |
| post-ui-test-only-delta.json | 2fa46a4fe7d5801aa418d49c2ee973d649b07fe9059dd3fbb715a8dd24a93f57 |
| 10-headless-final.log | d18541d2782d23cc3c8bfa2567a629af83153fb379f4cee5cd1eeac2e2dc4799 |
| 12-ui-final.log | 85123ce5538554bea319ebe49159c429541ef00e95c906a5cb4299f2fbbab11e |
| 20-composite-final.log | 85e78b0868464fd6339093836d2f32504745f53f45e4f2cd4e9b0e41ad4e0564 |
| 22-ui-text-final.log | 77f6fa36973016b5e502293437d7498a43f377098b75a2d2058c469e77eb5f7c |
| 21-headless-strict-final.log | 46c4308330be9984a5478afb890f70476c9027fe91cb3b6f0e9c8efb30fd6df6 |
| 23-ui-strict-final.log | 0a8e867ed48a85af16e6eac71511f787e18853cf13cce0507816b0c89b39b620 |
| 14-format-final.log; 15-diff-check-final.log (both empty) | e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855 |

The contract draft `/workspace/shared/shadow-modes-contract-draft.md`, SHA-256 `2e2b1be12ba053a5f7a2d833c2c0fcb9163054bb25cc01f31dbcfcee2fa9ef99`, matches the examined mode semantics, source ownership, reference policy, consumer provenance, preserved gates, exact-sigma/default-rounding distinctions and conservative work changes. One terminology correction was sent to the publisher before publication: replace **“128 MiB tracked live pixels”** with **“128 MiB known working set”** or **“128 MiB tracked working-set bytes”**. The reservation includes A8/RGBA, float intermediates, kernels/scratch and retained input storage; naming only live pixels understates its scope. No other draft/source discrepancy was identified. Final receipt wording and the new native fixture remain parent-owned.
