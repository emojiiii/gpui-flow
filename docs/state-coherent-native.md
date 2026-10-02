# APP-05 coherent state presentation: native evidence

Source publication: [`93ee7f7c`](https://github.com/emojiiii/gpui-flow/commit/93ee7f7cabfb7979aeca93ec137ccf81644baeb5), verified remote tree
`a8f08d035dde97d13eecc932630e8fdda65dfb42`. This report records the
frozen shared-worktree executable, not a newly rebuilt published revision.
[Machine-readable evidence](validation/2026-10-02-state-coherent-native-summary.json)
contains binary/archive/receipt/log hashes and exact click checks. Original
screenshots and raw trace remain in the retained evidence directory named there;
they are not included in this repository.

## Result and fixed scope

The [capture-format erratum](screenshot-evidence-formats.md) identifies these original desktop captures as lossy JPEG bytes
saved with `.png` filenames. Original files, raw reports and hashes remain
unchanged. Screenshot measurements describe decoded captures, not lossless
renderer pixels; trace-backed results retain their separate scope.

The Linux native run, **2026-10-02 19:43:57–19:48:37 UTC**, exited normally with
**0**, empty stderr and no reported state/composite/input errors. It produced
**72 trace records and 57 original JPEG desktop captures**. All **33 classified state-only captured
frames** retain the card and its child text together, including intermediate
widths, positions, colors and the orange 340-wide press frame. This verifies the
exercised coherent paint/hit repair. It does not inspect every GPU frame or prove
continuous smoothness/FPS.

The earlier failed [51-record/18-capture run](validation/2026-10-02-state-geometry-summary.json)
is retained unchanged. Its observed transition disappearance was real and must
not be retroactively relabeled a pass. This later frozen binary supplies the
repair evidence.

APP-05 is **bounded verified** for the original fixed ledger contract: builtin
flags, named/button inheritance, per-state timing and geometry changes. The
implemented property set is solid fill/opacity, x/y and intrinsic width/height on
Rect, Ellipse, Text, Image, Box and Frame. Arbitrary properties, rotation/scale/skew
and broader interpolation remain explicit exclusions, not added closure criteria.
The finite API is in [state-geometry.md](state-geometry.md).

The first local ledger snapshot `8d98280030ee1f908a4819daa83e55ad63b73256` has the
same acceptance text as the present row; its JSON blob is
`f06bf789f2e8baa49ea945e93c9b899307506127`. The source review matched `StateGeometryStyles::resolve` and
`StateKey` to all builtin flags; named-state/geometry tests to local palettes and
parent/grandparent inheritance; `set_state_transitions`/`timing` tests to per-state
entry/exit precedence; and intrinsic-size, interpolation, retargeting, layout/hit
and history tests to real geometry changes. The compact JSON lists the exact
requirement/test mapping. No new build or test run is implied by this review.

## Physical witnesses

- Small-step hover/exit/reentry, press/release, named move/reversal, disable/enable
  and explicit Focus/Blur overlap retain complete card/text samples
- Seven real clicks match the exact input-time accepted horizontal geometry,
  including misses at a future expanded/moved position and hits in the accepted
  displayed card. These checks do not infer input-time geometry from later captures
- Future-position wheel misses before arrival; a relocated accepted card receives
  actual click/wheel input. A wheel can invalidate the viewport after dispatch,
  so a post-dispatch `accepted_x=None` is not evidence of a picking failure
- Canonical base edit while named state is active gives revision 1; clearing named
  reveals edited x180/width360. Undo restores x130/width300 at revision 2, Redo
  restores the edit at revision 3. Prior state-only input stays revision 0
- Normal titlebar close during a visible named transition exits 0. The exact
  CPU-worker busy state at close was not independently observed

State-only sample IDs: **03–12, 16–28, 40–43, 50–55**. All were visually reviewed;
decoded-JPEG component/label counts corroborate intact content. The press screenshot is
immediately after an atomic physical drag released, while the accepted press
frame remained visible; it is not a screenshot captured with the API holding the
button.

## Intentional hard invalidations and remaining limits

Twelve captures (**13,14,29,30,33,35,36,37,44,45,47,48**) show actual blank/loading
intervals after viewport or canonical invalidation. They are preserved. Recovery
shows a coherent new card/text frame without old-position ghosts. The contract
withdraws incompatible retained content on these hard boundaries; this is not a
global no-loading or flicker-free claim.

No fixture control mutates background properties, and this fixture contains no
Image or animated alpha. Those cannot be counted as native coverage. No driver
upload failure, exhaustive leak, hardware-DPR/multi-monitor or cross-platform
acceptance is established. Selection outlines and `SceneEvent.local_position` remain intentionally canonical.
State-aware picking uses accepted displayed geometry; visual-local coordinates
require converting world position through that presentation, not assuming the
event-local field changed semantics.

Button inheritance, all six shape sizes, reactive-layout projection, accepted
animated-alpha hits, late ABA completion, fractional guard, accepted-empty domain
recovery, upload failure and retirement saturation have automated regressions.
They were not all physically exercised by this card/text fixture.

## Automated and provenance boundaries

The revised UI suite passed **245 tests**, including **13 coherence regressions**
and two navigation-adapter cases. After the final input-clock hook, two focused
navigation tests and four focus/input mirror fixture tests were rerun. Strict
UI/fixture Clippy and the native build passed. The earlier 74 property/state
checks remain separate historical evidence. Counts overlap; none is a new
full-workspace run.

Binary SHA-256:
`d7818f2c9fa087a610687e12b7951e63bc5d082cb5698093c16cfeeb68025621`.
Complete source archive SHA-256:
`b8d81edff2fc0c093a6e3ecff0dbb7e251268c68596137c9166d6fc660800a38`.
Native QA verified all **458 manifest entries** against the archive and checked
the executable before/after launch. The snapshot includes contemporaneous
INT-06/SVG work; it is not evidence of a single published-commit or reproducible
build. This documentation audit checked the retained evidence/hash manifests and
performed no build, native-window operation, code edit or Git-index operation.
