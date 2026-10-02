# Open-contour stroke native acceptance — 2026-10-02

The exercised Linux open-contour geometry, history and navigation matrix passed.
The auxiliary native/CPU-panel decoded-capture bound comparison is **20/21 within one pixel**;
one isolated low-contrast pixel is retained below. This is not an all-pixel-checks
pass or backend pixel-equivalence claim. **APP-02 remains partial for scale-fixed.**

## Source and run

Source: [`a6639014`](https://github.com/emojiiii/gpui-flow/commit/a6639014ceb276d2b33c64e04ad3e73adc770b30),
tree `465ed194ff3f565d1ceaeca0ecdd06a64cbfefb5`. All **475 repository build inputs** match
the frozen archive, before/after-link manifests and this published tree.
External dependencies, compiler and system libraries are excluded; no full
reproducible-build claim is made. Binary/archive hashes, exact receipt and audit
hashes are in the [native summary](validation/2026-10-02-open-stroke-native-summary.json).
This report is added in a later documentation revision than the source commit.

The real window ran **21:40:50–21:47:54 UTC**, closed normally with **exit0**,
and produced **38 toolbar actions,39 JSON records and60 original JPEG desktop captures**.
Stdout is empty; stderr contains the intended JSON stream without unrelated errors.
Original screenshots/trace/binary/archive remain in the retained evidence directory,
not in this repository. The configured **1060×720** window exposes all six panels
and complete status/footer rows.

## Exercised behavior

Native and CPU rows each show Center/Inside/Outside. The seven scenarios were
Open L, quadratic/cubic curve, straight Line, closed outer plus open inner,
SVG open arc, Ellipse open arc and the existing closed baseline.

- Implicit end-to-start closure affects the fill mask only; the actual closing
  diagonals/chords remain unpainted
- The straight Line shows empty Inside and doubled-width Outside; Butt, Round
  and Square caps were exercised
- Miter/Round/Bevel joins, solid/even/odd dashes (negative phase−13), translucent
  gradients and the additional white stroke layer were observed
- Compound EvenOdd/NonZero changes use the combined fill region despite absent
  fill paint, with Undo/Redo restoring the geometry and all panels
- Reflection, nonuniform scale, rotation and rounded ancestor clipping were
  exercised on selected scenarios; geometry/scenario history restored correctly
- Real Ctrl-wheel and empty-canvas dragging visibly zoomed/panned the rows without
  document revision changes. Exact viewport values were not logged

Real clicks produced visible selection feedback on ink and cleared it on blank
space, including the empty Inside line. The fixture has **no exact hit-ID trace**;
shared painted-hit/mesh assertions remain separate automated evidence. This is a
selected physical matrix, not every combination of all controls.

## Pixel evidence and retained outlier

The original audit records **23/23 trace/source assertions** and **41 focused
decoded-JPEG pixel assertions**. The [capture-format erratum](screenshot-evidence-formats.md) records the
historical `.png` filename mismatch; raw reports and hashes predate this
correction and remain unchanged. These measurements do not establish lossless
Canvas/GPU buffer or native/CPU renderer equality:

- Five history region comparisons have **zero changed pixels**:08→12,10→14,
  28→32,30→34 and52→56; region `[125,292,1060,740)` excludes toolbar/status text
- Both native and CPU straight-line samples measure **Center12px, Inside0px,
  Outside24px**
- Thirty sampled5×5 patches on implicit closing edges are exactly the background
  RGB(11,15,24). This supplements observation rather than proving every edge pixel

The separate decoded-capture threshold-bounding-box comparison is **20/21 within one pixel**.
In `01-open-L-initial.png`, the Outside native box is `[13,7,206,129]` and CPU box
`[13,6,206,133]`, relative to origins(758,295) and(758,530). The decoded screenshot sample in the CPU panel
at **(948,662)** is **RGB(20,39,56)** versus background(11,15,24). It extends the
threshold box by four pixels; the continuous main ink ends at native y423 and
corresponding CPU y658. **Its cause is unestablished.** Lossy JPEG encoding may affect this low-contrast
sample and its four-pixel threshold-box extension; the capture alone cannot
distinguish compression from renderer output. It is neither filtered out nor
labeled a proven renderer defect. The original42-check record with this
auxiliary condition unsatisfied is preserved as `native-pixel-audit-initial.json`.

An additional read-only recheck independently parsed all39 receipts, recomputed
all five selected history comparisons and verified the outlier's decoded-capture RGB.
It did not repeat the full23/41 audit or create another native run.

## Coverage boundaries

Immediate captures retain expected `Compositing...`/previous-state intervals;
separate settled captures establish recovery. Screenshot pixels and the trace
count sampled afterward are not asserted simultaneous. No every-frame smoothness,
backend/browser equivalence, cross-platform or device-level DPR claim is made.
Physical controls do not establish memory/precision rejection, zero-length/empty
inputs, every supported shape or every control combination.

The [automated source checks](validation/2026-10-02-open-stroke-alignment-summary.json)
passed1,566 default-member tests (10 existing ignored),252 UI tests and the fixture
control-tour test, plus strict workspace/docs Clippy, formatting and native link.
These are supplied prior checks, not additional native tests. The precise API,
empty-mask semantics and numerical limits remain in [stroke-alignment.md](stroke-alignment.md).
Scale-fixed is still missing; Canvas command strokes remain centered-only.
