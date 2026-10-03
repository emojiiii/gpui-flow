# Canvas image pattern native acceptance — 2026-10-02

The bounded Linux control tour completed with **41 accepted physical button
activations**, **23/23 independent trace checks** and **48/50 decoded-JPEG
screenshot checks**. Two exterior background patches remain unresolved. No
functional failure was observed in the exercised controls; this is not an
all-pixel-checks pass. **ELM-10 remains partial: actual glyph-outline `strokeText`
is not implemented.**

## Source and run

Published implementation: [`6e6af034`](https://github.com/emojiiii/gpui-flow/commit/6e6af03495a67b78d18be604c9389b3cb1e18b0c),
tree `d3d157491bc857570afb7bd7ee2954eadc9f30c4`.
The later `published-source-match.json` records **479 repository build inputs**
matching the published tree and the frozen archive/manifests. Publication
followed the sealed QA report, whose original pending-publication wording is
unchanged. External dependencies, compiler and system libraries are outside this
comparison; this is source/binary provenance, not full build reproducibility.
This documentation is a later revision than the implementation commit.

The actual native window ran **22:45:50–22:51:04 UTC**, closed through its titlebar
and exited **0**. Its 44 JSON records comprise 41 button records numbered 1–41,
one startup record and two source-regeneration substeps. Stdout is empty; stderr
has 48,297 bytes and no non-JSON runtime errors. The independent audit covers
64 per-surface reports, eight rollback reports and configuration revisions 0–10.
The final restored **1230×800** client area exposes all three control rows and
both complete footer lines (`32-configured-window-final.jpg`).

Hashes, receipt details and verification scope are in the
native summary (historical record in Git history).
Original captures, raw audits, trace, executable and archive remain in the
retained evidence package, not this repository.

## Exercised behavior

- Both Canvases, initially DPR 1/2, rendered repeat-filled curves with an evenodd
  hole, a single NoRepeat source extent with transparency, a dashed open patterned
  stroke, an affine reflection and patterned current-path fill/stroke
- Replacing the source preserved captured colors; recapture changed them.
  Nearest/bilinear regeneration visibly changed filtering and restored the image
- Clear retained current geometry; BeginPath discarded geometry without clearing
  existing ink. Build restored 448 path bytes without publishing pixels, and
  subsequent paint reproduced the captured identity result. Draw-time CTM changed
  pattern/pen appearance; screenshot bounds do not prove exact geometry identity
- Nested frozen clips and an evenodd hole, inner/outer Restore and a fresh-batch
  yellow square were visible, subject to the two strict exterior-sample exceptions
- Four late rejections preserved generation, snapshot identity and retained-path
  replay on both surfaces. Snapshot identity is an Arc witness; the separate
  temporary-surface path replay compares premultiplied RGBA
- Safe shrink/grow preserved the intersection and discarded cropped content;
  reset resizing cleared pixels and geometry. Undo restored configuration only.
  DPR swap/Undo copied physical pixels without scaling, and Regenerate restored
  logical artwork from live patterns

NoRepeat was physically exercised as explicit fill, not every repeat-mode,
path-target and operation combination. Safe grow and the second resize Undo
were followed by regeneration without an immediate path probe; their zero-path
states are not separately established by the trace. Filter toggling alone also
changes presentation sampling. Gallery/clip actions span multiple batches and
surfaces, not one cross-surface transaction.

## Screenshot result and unresolved patches

The package contains **34 original JPEG captures** (5,935,004 bytes), correctly
renamed `.jpg` without changing bytes, and **six decoded PNG derivatives**. The
derivatives preserve the decoded JPEG raster, not lossless renderer pixels.
The central capture-format erratum (historical record in Git history) separately
corrects 327 older desktop captures; they are not part of this run.

There are **24 zero-difference full-Canvas region comparisons** for rollback,
source replacement, filter recovery, BeginPath/build and gallery recovery.
These establish equality only within decoded lossy captures, not underlying
Canvas RGBA, native/CPU renderer, browser or cross-platform equality. The other
checks cover expected changes, blank interiors, crop discard, holes and source
extent samples.

In `18-nested-clips.jpg`, strict 5×5 screen patches `[431,427,5,5]` (left) and
`[806,427,5,5]` (right) contain respectively **5 and 10 pixels** differing from
background RGB `[25,43,67]`. Both map to logical x `307,312)`, y `[115,120)`;
all outer-clip controls have x ≤ 303. They are wholly exterior, at least four
logical units beyond that bound, rather than boundary-straddling samples.
More distant samples match the background. **The cause is unestablished:**
neither renderer leakage nor compression is demonstrated. Both failed checks
remain in the original audit; they are not filtered out or relabeled as passes.

A read-only integration recheck reproduced all 50 recorded screenshot
measurements, including these two failures and all 24 zero-difference regions.
It verified capture signatures, the six derivative rasters, hash manifests,
44 trace records, contiguous button counts and 479 archived repository build
inputs. It did not rerun the native application or independently recheck the
remote publication; the published-tree link is the supplied later record.

## Separate automated evidence and boundaries

The [automated summary (historical record in Git history)
records 1,602 default-member passes (10 existing ignored), 258 UI passes,
36 focused composite pattern passes, six focused resource pattern passes and
one fixture helper-tour pass. Focused/full counts overlap. Strict Clippy,
formatting and native link passed. Strict rustdoc still fails on the pre-existing
bare W3C URL; only `rustdoc::bare_urls` was allowed for the successful docs run.
These prior runs were not repeated by native QA or this documentation integration.

This is one Linux fixture run at Canvas DPR 1/2, not hardware-display DPR testing.
Immediate captures retain previous-state/`Compositing…` intervals; settled
captures support comparisons, and the later trace-count sample is not a pixel/log
synchronization guarantee. No video or frame-by-frame timing measurement exists.
No exact hit-ID/CPU comparison, browser/cross-platform equality, performance
budget, all-transform Cartesian matrix, singular/tiny-scale runtime control,
missing-resource/budget failure or source-Canvas lifecycle was physically tested.
RepeatX/Y, pattern text and persistent scene Paint remain outside this fixture.
The API scope and automated rejection/resource guarantees remain in
[canvas-patterns.md](canvas-patterns.md); actual outline `strokeText` remains absent.
