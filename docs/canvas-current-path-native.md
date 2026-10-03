# Canvas current-path native acceptance — 2026-10-02

The bounded current-path functionality passed in a real Linux GPUI window at
Canvas DPR 1 and 2. The original run found a clipped footer row; a second,
layout-only run verified its fix. These are separate executables and evidence
sets. ELM-10 remains **partial** for strokeText and createPattern repeat/no-repeat
with transform. Its exact required current-path constructor set is covered;
arc/rect conveniences and full browser context state are exclusions, not new
closure criteria.

## Source, build and evidence publication

| Evidence | Source/provenance | Native result |
| --- | --- | --- |
| Full functional run | Published [`39fcf56a`](https://github.com/emojiiii/gpui-flow/commit/39fcf56a2e91f59d5da7a04ee519cade66f8cb69), tree `565b687a9392635ac9adfead5260a070358fbd78`; all 475 repository inputs match frozen archive and pre/post-link manifests | 21:12:30–21:18:56 UTC; exit 0; 55 physical buttons, 84 JSON records, 63 JPEG desktop captures |
| Footer layout follow-up | Footer-only fix published as [`8dd6f110`](https://github.com/emojiiii/gpui-flow/commit/8dd6f1104d2e134b8b15f83e64c8fdb336ee4044), tree `3c25a050e5e01ad0be205377660dfdb76653f084`; tested executable is a frozen 475-input shared tree including unpublished-at-build open-stroke work | 21:26:06–21:27:04 UTC; exit 0; 2 physical buttons, 11 JSON records, 8 JPEG desktop captures; layout only |

The follow-up is **not** an exact build of one published commit, does not replace
the original functional acceptance, and grants no acceptance to open-stroke work.
Both manifests exclude external dependency sources, compiler and system
libraries; neither is a full reproducible-build claim. This documentation
revision adds this report and its summary after the source commits. Their
`evidence_commit` is null rather than incorrectly naming either source commit.

Original executable SHA-256:
`531921c529dd27894d5b194d699439c26d17e6adf0a2c9b35eba8e759a823a8c`.
Layout executable SHA-256:
`d2793b833a532e43bdeaadcfd0b3ac9dfc8e11c5b21f446cd8be6aa47939828c`.

## Original functional acceptance

Trace references are original `native.stderr` line numbers; capture numbers refer
to the original functional package. Its `.png` filenames contain JPEG bytes, as
recorded in the capture-format erratum (historical record in Git history). Both surfaces are independently checked,
not treated as one distributed transaction.

| Exercised contract | Evidence and result |
| --- | --- |
| Cross-call Q/C outline and evenodd hole; Save/Restore path independence | L1–5, L11–13: Build/Append publish false with generation/snapshot unchanged; later Draw publishes; capture 02→10 identical |
| Clear versus BeginPath; reusable geometry | L6–10, L70–71: Clear retains path; BeginPath retains pixels but empties geometry; subsequent empty Draw publishes false; capture 51→53→54 identical |
| Hole append and duplicate contour parity | L17–22, L72–79: Clear/Draw exposes appended hole; duplicate hole contours cancel under evenodd; capture 16–19,56 |
| Draw-time paint/pen transform | L23–24: gradient and stroke thickness change while stored silhouette/hole placement remains; capture 19,21 |
| Frozen clip and both ways out | L26–27: translated clip does not relocate geometry; stripes respect hole; yellow post-Restore and orange next-batch squares escape clip; capture 24 |
| Clip reuse and late-invalid rollback | L29–30; L14,25,28,62: original shape replays after clipping; four rejections preserve pixels/generation and path replay on both surfaces |
| Safe resize and Undo | L31–37: only top-left physical intersection survives; cropped pixels do not return; empty-path Draw is false; configuration Undo does not restore path |
| Reset resize and Undo | L43–49: pixels/path clear; dimensions/safe flag restore without runtime artwork/path |
| DPR swap, Undo and regeneration | L55–69: physical pixels are copied without scaling; path resets; regenerated logical artwork returns at current DPR; Undo does not replay path |

The six Regenerate button clicks generated 24 internal edit records, not extra
physical actions. There are five startup records; **55 physical actions** account
for the remaining button records. The trace audit passed **27/27 assertions**.
`accepted=true` for Reject late means the outer diagnostic ran successfully;
`expected_batch_rejection=true` and per-surface intact/replay receipts establish
the invalid inner batch's rollback. Replay uses temporary clones without
publishing their diagnostic pixels to the live canvases. Runtime edits preserve
document revision; five configuration changes and five Undos end at revision 10.

Twenty original JPEG capture pairs have **zero differing decoded pixels in both displayed
300 × 210 regions**: left `130,278,430,488)`, right `[487,278,787,488)`.
Pairs: 02→10, 10→12, 10→14, 16→17, 10→19, 21→22, 24→25, 10→27,
29→30, 32→33, 32→35, 02→37, 02→42, 44→45, 47→48, 02→51,
51→53, 53→54, 02→58, 02→61. These are within-run decoded-JPEG region
comparisons, not lossless Canvas/GPU buffer identity, DPR1-versus-DPR2 or
browser/cross-platform equality. Trace-backed generation/snapshot/path-replay
checks remain independent of screenshot encoding. Canvas DPR comes from
configuration receipts; device display DPR was not separately queried.

## Original defect and layout-only follow-up

The original fixed 156px footer clipped the Right final status row in expanded
and restored windows. Original capture 59 records restored content size; capture 61
preserves the expanded defect. Full per-surface receipts remained in JSON, so
the defect did not invalidate the functional checks. Original screenshots also
preserve expected immediate `Compositing...` transitions or preceding states.
Settled captures establish final visuals; screenshot pixels and the subsequent
trace-count observation are not asserted to be simultaneous or all-frame proof.

The follow-up restored **1090 × 670** content at screenshot coordinates
**x92…1182, y262…932**. capture 04 (Regenerate settled) and capture 06 (Reject late settled)
show complete Left and Right rows. The Right row ends around y919, about **13px
above the content bottom**. Trace L10 is physical button 1 (Regenerate), L11 is
button 2 (Reject late); both surfaces report intact pixels/generation and replay.
Eight JPEG desktop captures and 11 JSON records document this narrow recheck, followed by normal
close. The full functional button matrix was not repeated.

## Automated/native boundary and retained evidence

Automated validation is recorded separately: **15 new composite + 7 new UI**
regressions; full affected libraries **309 composite / 252 UI** passed; focused
**88 / 29** overlap those full runs. Strict affected all-target/fixture Clippy,
rustfmt, whitespace checks and native link passed. No new tests, build or native
run was performed while preparing this documentation.

These native controls did not establish geometry-pool rejection, font failure,
upload/OOM injection, every malformed-command case, safe A→B→A before resource
preparation, or every renderer frame. Reduced admission/rollback tests and
reviewed commit ordering must not be relabeled native fault injection. No
Windows/macOS/browser-equivalence or new workspace-wide/CI result is claimed.

Original evidence remains outside the repository, unchanged:

- `/workspace/shared/canvas-current-path-native-validation-20261002T2109Z`: `native-qa.md`, `native-audit.json`, `pixel-region-comparisons.json`, `published-source-match.json`, trace/receipt, hashes, original JPEG captures with `.png` filenames, frozen inputs and binary
- `/workspace/shared/canvas-current-path-footer-validation-20261002T2122Z`: `native-layout-qa.md`, `handoff.json`, trace/receipt, hashes, original JPEG captures with `.png` filenames, frozen inputs and binary

The [native summary (historical record in Git history)
retains report/manifest hashes and source distinctions. Both packages' evidence
and screenshot checksum manifests were verified during documentation review;
this is an integrity check, not a second native execution or independent rerun
of the 27-assertion audit. A separate integrator re-read all 84 records and
recomputed all 20 selected pairs (40 RGB region comparisons), confirming exact decoded-JPEG
region matches in `integration-verification.json`; it also visually checked the full
footer rows in follow-up capture 06. This narrow independent recheck is not a rerun
of all 27 assertions or a new native execution. Large raw traces, binaries,
archives and screenshots are not bundled into this repository.

See [current-path contract](canvas-current-path.md),
automated summary (historical record in Git history), and
[prior fillText/measureText slice](canvas-text-commands.md), whose evidence remains
[`003288b3`](https://github.com/emojiiii/gpui-flow/commit/003288b3a9b4178890c162a4f1271f3b8dd41f4f).
