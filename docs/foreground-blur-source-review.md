# Foreground blur source review

Review started 2026-10-03 09:56 UTC against local HEAD `7d4892c075d93dfdcc4f4a7eba0149d4bc4c3798` and the in-progress, unstaged foreground-blur changes in `/workspace/shared/gpui-flow`. The design at `/workspace/shared/app04-blur-design-20261003.md` was read in full. This is a source review, not an execution receipt. No Cargo command, build, test, native window, source edit, repository documentation edit, or Git mutation was performed by this reviewer.

## Findings reported during implementation

### P2: own shadows discarded before child blur enters a container clip

Initial source: `vector.rs:645–663`, `684–721`; SHA-256 `8c6a3ab2694192a4f8ecfa270e87134077c0aca7dd61f44c2cfa5ca9c47cf02f`.

Preparation intersected the legacy, pre-blur child world bounds with a Frame/Box child clip, then constructed the container's own shadows only when that extent was nonempty. An unfilled 20×20 Frame containing a red child at x=-5, width=4, blur=1 has no legacy child ink inside its clip, but the child's completed blur contributes visible alpha inside it. The physical resolver restored the visible child contribution but could not restore previously discarded Frame shadows. Own outer and inner shadows silently disappeared.

Reported to the parent and implementer immediately. The implementation now retains `PreparedShadow` records with optional legacy ink bounds, with physical source bounds supplied later. The added `blur_child_halo_entering_empty_frame_casts_own_shadows` test asserts both entering red child alpha and the otherwise missing green Frame shadow, then covers inner shadow. Source repair reviewed; test execution is owned by the implementer and is not asserted here.

### P2: early f32 origin composition destroys the physical lattice

Initial source: `blur.rs:33` and the `world_to_layer * node_world` operations in `raster.rs`; `blur.rs` SHA-256 `0ea2cb8fe8eef5d662eeeaa07045a4bb8e3ee89d4bc95b236b70c370b3e5141b`.

The device resolver plans with camera×world but `SurfacePlan` first folds the integer source origin into a f32 camera, before world composition. For camera.tx=-1e9 and node.world.tx=+1e9, a 10×10 leaf with blur=1 plans S.left=-7. f32(-1e9+7) rounds to -1e9, so the source paints at x=0 instead of x=7. It then blits at -7, shifting/cropping pixels and potentially allowing nontransparent clamp-edge input. Existing matrix inverse and physical-coordinate checks accept this case. Large cancelling translations are already an established supported scenario in the repository.

Reported to the parent and implementer immediately. The implementer replaced early origin composition with `PhysicalSpace::local`, which combines camera, world, DPR, and integer origin before final narrowing. At the 10:05 snapshot, all vector/image/path-eraser/child-clip/inherited-clip painting callsites use it; physical shadow parameters and source planning use the same provenance, and recursive scratch inherits it. The legacy no-blur path stays separate. This source repair was reviewed. The new ±1e9 cancellation test compares complete bytes and extents at DPR 1/1.25/2; it directly detects the reported failure. Test execution is not asserted by this reviewer.

## Locked image one-tap correction

The implementer's independent convolution oracle found a real image 0.25.10 dependency issue. `sample.rs::get_gaussian_kernel_1d` can produce exact zero adjacent f32 Gaussian weights at very small positive sigma. `filter_1d.rs::prepare_symmetric_kernel` trims the kernel to length one, while the ring row arena still forces one pixel of padding. Its one-tap horizontal sampler reads that padding position. The ring loop also starts at y=1 with half_kernel=0 and omits the first destination row.

The proposed narrow foreground-only correction computes the exact adjacent f32 unnormalized weight using the dependency's expression and returns the source only when it equals zero. For this range the kernel width is three and the mathematically correct eight-bit result is byte identity. Positive blur must retain its own full-source planning, isolation, conservative bytes/work reservation, and cancellation checks; this is not the zero-blur path. The currently inspected implementation does that.

This deviation is acceptable if stated explicitly in the contract. It must not be described as preserving unmodified image's output for every positive sigma. The added `blur_exact_f32_one_tap_transition_corrects_identity_and_preserves_nonzero_kernel` test binary-searches adjacent representable f32 values at the exact adjacent-weight transition. Its asymmetric first pixel detects the old library output error, the corrected branch asserts byte identity, and the immediately nonzero side asserts direct dependency output. This is a discriminating characterization; it was not run by this reviewer. Added cancellation tests cover before/after Gaussian, output rows and the narrow identity branch.

**Known retained limitation:** A8 shadows still call the same image generic Gaussian with one channel. A device sigma of .001 (for example `shadow.blur=.002` at unit scale) therefore still has the one-tap horizontal +1 physical pixel displacement. The transparent guard makes the omitted top output row typically irrelevant, but not the horizontal shift. This remains observable for own shadows formed before foreground blur and descendant shadows sampled inside a blurred ancestor: foreground filtering does not correct an already displaced shadow. Per the parent instruction this review does not request a legacy-shadow change, which would violate this increment's zero-foreground-blur byte-preservation scope. It is a separate APP-04 shadow repair item, and the new foreground special case must not be presented as a general Gaussian bug fix.

## Source review observations

- The bottom-up device plan accumulates child filter guards, intersects child contributions with guarded clip bounds, excludes erasers from positive parent support, and retains operation/mask markers. Own blur runs after the complete source phase stream; own shadow capture omits own filters but recursively runs completed descendant filters.
- Runtime and no-pixel admission use the same full source dimensions, reserve the same blur allowance while recursively rendering source contents, charge source-sized grayscale and destination-sized composition separately, preserve depth across shadow replay, and release full reservations. Failure restoration remains owned by the outer rasterize transaction.
- The 56P + 32WK + 32W + 128K byte reservation was traced against the locked image 0.25.10 generic RGBA Gaussian: two full f32 planes, either a full intermediate f32 plane or ring storage, row arena, column clamp padding, kernel copies, row references, byte input/output. The bound is conservative for the admitted dimensions. The four-channel work formula has room for both separable passes, ring warmup, byte/float conversions and initializations. This is a source accounting assessment, not measured memory or time.
- Alpha is filtered with RGB in premultiplied encoded color; source/output ownership transfers avoid a byte-plane clone. Final premultiplication clamping, opacity/blend/Pixel application and inherited clip placement are visible in source.
- Full-source Text propagation includes both own and ancestor blur. Native gate and domain boundary selection include active blur. SVG planning rejects it before host resource preparation; strict Leafer export checks retained nonzero blur, including hidden objects. Complete source/placement identity includes the amount, and broad full-paint invalidation covers blur-bearing scene edits without claiming analytic bounds include device guards.

## Test evidence and open acceptance scope

The inspected new convolution test is independent of the production filter helper and builds a scalar f64 separable reference over independently constructed premultiplied rectangles. Sigma 9 crosses the dependency's 55-tap ring/full-plane cutoff. Crop tests compare different output crops sharing the same physical lattice. Exact-cap tests measure the runtime ledger, rerun with those caps, and assert cap-minus-one fails before pixel work; these are meaningful checks of admission/runtime parity.

At 10:07 the implementation added and this reviewer read further discriminating tests: a captured complete pre-filter phase stream is independently convolved for both Frame orders, grayscale is independently applied to blurred byte channels, opacity is checked once, a shifted green parent shadow is compared to finished child alpha, a rounded inherited clip is compared to one unblurred coverage mask, and a separately rendered blur source is mapped into a nonseparable destination at nonzero offset. The nonseparable oracle intentionally reuses the already-existing per-pixel blend helper while independently choosing source coordinates, so it tests the new offset integration rather than revalidating blend math. The blurred alpha-mask test compares completed mask alpha, with an explicit nonzero halo assertion. These are stronger than the original exact-ledger test alone.

Still unresolved in the reviewed 10:07 snapshot: final aggregate/test results, foreground-specific retained OwnedCanvas backdrop/Pixel regression, fractional fixed export crop and lossless codec evidence, transactional Canvas late-failure evidence, exponential blur/shadow-chain preflight and depth boundaries, and the separate native fixture. The current producer test uses synthetic Text ink; it is not native text evidence. Sigma boundary tests validate parameters only; a sigma=64 filter can still exceed unchanged work/size ceilings, which is consistent with bounded admission but should not be presented as rendered coverage. These are evidence/scope limitations, not additional demonstrated source defects.

No tests were run by this reviewer. APP-04 remains partial; no native gate result, screenshot golden, performance claim, arbitrary host backdrop support, or GPU fence is established here.

## Snapshot tracking

Initial 09:56–09:57 snapshot hashes:

- blur.rs: `0ea2cb8fe8eef5d662eeeaa07045a4bb8e3ee89d4bc95b236b70c370b3e5141b`
- blur_tests.rs: `a8f0a6374c4bf2e48941a599b330e94c101768237a723b66a98fe768584f2f5a`
- raster.rs: `4693022097eb573ba1c01151afe78e2e3227c7f8968e32dd89fea79bc5a1591a`
- vector.rs: `8c6a3ab2694192a4f8ecfa270e87134077c0aca7dd61f44c2cfa5ca9c47cf02f`
- shadow.rs: `0311f958c7094d5c6896a0154af41795656ce25275f5619779a0e8471fb6da2d`
- plan.rs: `392906b7b10b3ec0eaab2f75649e157f7e0b2a7060574ec4732f2236d4e510cb`
- raster/admission.rs: `493bd97827ba53b1ddc11470e7bda7c5a36735cfb10cc5bed317cbbe480254ee`

At 10:00 the source had changed during review. New vector hash `8a6d87a7e48bed6053bdd0c15195aff593b908b893ff14e4cb43f75d633dba85`, raster hash `82a6d33fe534c94a1e29ca92be344b0e88df2222b0c857d3e0def4a868eb9f15`, test hash `9201915b366177d0a8a2b6e0a4a66f408ba3a64691d9524e3cca59f8a7f72990`. These changes were reread for the shadow-record repair and one-tap correction. A final hash set will supersede this snapshot after implementation stabilizes.

Reviewed 10:05–10:07 checkpoint (the implementation remains in progress):

- blur.rs: `d50584a79f40cb472251b42a1c2b5c98896f4aad45ec121dbb6e2330ce1652d2`
- blur_tests.rs: `264eeb9e430d6d0c3883c422ff806b13f3094662071737c3339819bd3fb83167`
- raster.rs: `4e1d3ac38cbf8db07936d6452010dcb518fc6124d835f49fd1ede081477282ca`
- vector.rs: `8a6d87a7e48bed6053bdd0c15195aff593b908b893ff14e4cb43f75d633dba85`
- shadow.rs: `2fca8e27b4d5aebb45a24cae7ead10f3024d71b49ac218c7853ed17e6fcb9cee`
- plan.rs: `392906b7b10b3ec0eaab2f75649e157f7e0b2a7060574ec4732f2236d4e510cb`
- raster/admission.rs: `493bd97827ba53b1ddc11470e7bda7c5a36735cfb10cc5bed317cbbe480254ee`

At this checkpoint both newly reported P2 defects have source repairs reviewed, with no further demonstrated foreground P1/P2 finding. This is not a stable final-source sign-off: later edits and test results require reconciliation, and the legacy tiny-sigma shadow limitation remains explicit.

## Final frozen-source delta review, 2026-10-03 10:30 UTC

The final selected input manifest is `/workspace/shared/foreground-blur-validation/final-source-inputs.json`, with 528 code/assets/Cargo/script and related inputs, aggregate SHA-256 **`365ad834c6e3138277ba9c0a774192046a4daf3df0eec8ab0e7ecf305f89db05`**. This reviewer independently read and rehashed every listed on-disk file, checked lengths, and recomputed the manifest aggregate using its documented canonical JSON encoding. There were **zero mismatches**. Comparing its entries to `source-checkpoint.json` independently confirmed **527 identical inputs** and only `crates/flow-composite/src/blur_tests.rs` changed from aggregate `b0590aeef76290a3584b958b9d987b9976907ba98271d2af4b556c1f7421bc4c`. The final test file hash is **`30447f0ebe01caf877371971bd9d7b0f6c6ccd4ddb2e25eb3509c374fa9b670f`**. This was read-only integrity verification, not a test or build invocation.

The final core hashes are unchanged from the reviewed 10:05–10:07 checkpoint: blur `d50584a7…`, raster `4e1d3ac3…`, vector `8a6d87a7…`, shadow `2fca8e27…`, plan `392906b7…`, admission `493bd978…` (full values above). This reviewer rechecked the final transform callsites: physical path/image/eraser/clip painting uses `Raster::local_to_layer`, shadow sigma/offset mapping uses the physical composed matrix, recursive shadow/blur scratch retains physical provenance, and device-space culling uses absolute resolved support. Legacy `world_to_layer` multiplication remains in the intentionally separate zero-blur path and as an unused culling argument when a device plan is present.

### Final delta and test-source assessment

All added consumer and limit tests since the earlier review were read:

- `blur_canvas_draw_keeps_snapshot_and_rolls_back_prepared_resource_budget_failure` renders a captured blurred snapshot after the mutable scene changes, verifies old snapshot immutability, then checks an already-prepared Image input plus a work-limit failure leaves Canvas generation, snapshot identity, and resource bytes intact. The failure is after resource preparation and before admitted output work; this is not a claim of inducing a post-Gaussian allocation failure.
- `blur_fixed_fractional_export_crop_matches_full_source_and_lossless_formats` decodes PNG and WebP and compares full RGBA bytes, compares the crop to a generous render on the same physical pixel lattice, and checks the explicit background in JPEG with a tolerance. Its source is partly outside the export crop; the separate new all-offscreen test supplies the stronger wholly-offscreen coverage.
- `blur_svg_rejects_before_resources_and_hidden_policy_is_unchanged` requires the blur-specific planning error without prepared Image/Text resources and checks the existing hidden/zero-opacity/zero-blur policy.
- `blur_owned_background_and_prior_siblings_are_never_filter_sources` checks the unaffected prior green sibling, foreground halo compositing over opaque green, and a blurred Pixel eraser revealing the retained blue OwnedCanvas background without changing distant pixels. It is a meaningful ownership check; it does not claim a new arbitrary-backdrop facility.
- `blur_recursive_replay_depth_and_chain_cost_reject_before_output` builds repeated nested foreground blur plus outer/inner shadow replay and checks WorkLimit, reduced depth, and reduced input limits with retained bytes/peak/work unchanged before domain pixels. The reduced depth/input branches exercise preparation limits; the WorkLimit branch exercises recursive no-pixel replay.
- The sole b059→365 delta, `blur_sources_wholly_outside_each_view_edge_and_corner_keep_visible_halos`, covers all four edges and corners at DPR 1/1.25/2, explicitly requires nonzero in-view alpha, and compares every tight-output pixel to the generous rendering. This would detect both source culling and pre-filter viewport cropping; it is not a helper-versus-itself convolution test.

These added tests address the identified 10:07 consumer/limit source-coverage gaps. They remain source-reviewed tests; the implementation owner's final command receipts, aggregate results, strict checks, and any native evidence are separate artifacts. This reviewer did not run tests, Cargo, builds, or windows during either review pass.

### Contract cross-check

The contract draft read at `/workspace/shared/foreground-blur-contract-draft.md` had SHA-256 `a1f1766371277e8ace134b7db7d51313261d875da60e470054449902ecfb6d66`. Its model/range, transform, premultiplied encoded-color order, one-tap correction, ownership, bounded ledger, rejection, and evidence-boundary descriptions match the inspected implementation.

`ChangeSet.full_paint_invalidation` is precisely: at least one PAINT/TRANSFORM/GEOMETRY/OPACITY/ORDER/STRUCTURE dirty flag, and any positive blur in the old or new scene. The draft correctly describes broad whole-scene/owning-domain invalidation and the DATA-only/no-op exceptions; HIT-only changes likewise are nonvisual and do not trigger it. This is an invalidation signal, not GPU damage processing. The draft also accurately retains the old A8 small-sigma shadow displacement for own and descendant shadows inside foreground blur.

One narrow wording correction was sent to the parent: replace the unqualified statement that fractional crops share a lattice with “Viewport/content-mask crops under the same physical transform share the lattice and do not change the source filter origin.” A new arbitrary fractional export origin changes the export transform and can change sampling phase. The export test intentionally offsets the two world crops by an integer number of physical pixels. The parent applied that correction, and this reviewer reread the final paragraph. The updated contract draft SHA-256 is `13697bbffac4912ccd7fc49b7762a40ff614511139ef1b530e093c4cf05bcfda`. No contract mismatch remains from this review.

### Final review conclusion

**No new concrete foreground P1/P2 finding at the verified frozen source.** Both originally reported P2 defects have reviewed repairs and directly targeted regression source. The foreground one-tap specialization is narrowly justified and explicitly documented. The legacy A8 tiny-sigma shadow displacement remains a known inherited limitation and future shadow repair item, not silently classified as fixed. Real host Text/native producer and physical presentation evidence remain outside this source review; APP-04 remains partial. Final test/pass claims must come from the implementation owner's exact-source receipts, not this review.

## Report sealed, 2026-10-03 10:32 UTC

Before sealing this report, the reviewer read the implementation owner's receipts for final composite/headless/headless-strict/fmt/diff stages: each records exit code 0 and identical source-before/source-after aggregate `365ad834…`. The earlier UI library and UI strict receipts each record exit code 0 and unchanged aggregate `b0590aee…`; the independently verified delta is only the composite all-offscreen test source, with all other 527 inputs identical. The successful log summary totals were read separately: composite 428 passed, headless 1,741 passed / 10 ignored, UI 284 passed. These are audited owner-produced receipts/logs, not executions by this reviewer.

The separate forthcoming native fixture is outside this sealed source snapshot. Adding it changes the aggregate inventory and requires its own source/binary/execution evidence; it does not retroactively extend this review or its test receipts. No additional source changes, tests, Cargo/build invocations, Git mutations, or native windows were performed by this reviewer.
