# Mixed SVG export: native evidence

Source publication: [`93ee7f7c`](https://github.com/emojiiii/gpui-flow/commit/93ee7f7cabfb7979aeca93ec137ccf81644baeb5), verified remote tree
`a8f08d035dde97d13eecc932630e8fdda65dfb42`. This report identifies the
frozen shared-worktree executable. [Compact machine-readable evidence](validation/2026-10-02-scene-svg-native-summary.json)
links the earlier raster proof and records exact binary/archive/output/receipt
hashes. Original outputs, screenshots and raw trace remain in the retained local
evidence directory; they are not bundled in this repository.

## Result and ELM-11 decision

The Linux run **2026-10-02 19:59:44–20:03:57 UTC** exited normally with **0**.
**13 physical button-triggered exports** produced **8 SVG + 2 PNG files** and
**3 expected atomic rejections**. Stderr contains 14 intended JSON records
(the attempts plus one Undo witness), with empty stdout and no unrelated error.
Twenty-five original screenshot PNGs record source and decoded preview states.

Together with the earlier [PNG/JPEG/lossless-WebP native acceptance](scene-raster-export.md#native-acceptance-and-independent-file-audit),
this closes ELM-11's fixed four-format, fixed-region contract. ELM-11 is
**bounded verified**, not complete upstream export parity. JPEG/WebP were not
rerun in this SVG-specific pass. The supported SVG effects/producer boundary
remains [scene-svg-export.md](scene-svg-export.md).

## Actual files and physical behavior

- Full 440×320 DPR1 opaque SVG and PNG; full DPR2 SVG at 880×640; transparent
  crop `(20,30,360,240)` at DPR2 gives 720×480 SVG and PNG
- Every successful SVG parses as XML with five paths, five clip paths, real
  vector painting and exactly three separate producer PNGs: Image, Canvas and
  host-shaped Text ink. There are no raster children under the vector, editable
  text nodes or external resources. This is not a whole-scene screenshot
- Image remains 96×72 and Canvas 100×70 across DPR settings; text ink is 252×104
  at full DPR1, 504×208 at full DPR2 and 504×120 for the DPR2 crop. Independent
  PNG decoding finds actual transparent glyph gaps, not a solid text rectangle
- Font bridge off, pending Image and failed Image reject attempts 7/8/9 without
  SVG files. Original preview pixels stay byte-identical in the audited region
- Resource recovery restores baseline SVG bytes. Canonical color edit changes
  bytes and preview; Undo restores exact canonical JSON while preserving the
  previous preview until export. Re-export restores baseline bytes
- Source-only pan/zoom visibly changes the source pane but leaves fixed-world
  export bytes and preview unchanged. All export receipts assert unchanged
  scene JSON/revision, selection, viewport and owned Canvas identity

The full DPR1 SVG SHA-256 is
`4c6a0637cb286c9df07bdaac0bd86f9a41a47d2fa810ff04f615f7c68e025de5`.
Crop attempts 4/6/10/12/13 are byte-identical:
`4483ed3c11f1f0fd6322a1b14580c0cf12a359d4b738c335b470423e5fef5e29`.
Edited attempt 11 differs. Every encoded length/FNV receipt matched independently
computed bytes; SHA-256 was also recorded.

## Readback is real, not pixel-equivalence certification

Native `resvg 0.46.0` parses and renders the complete exported SVG and displays
its decoded pixels in a separate preview. Only embedded PNG data URLs are
allowed; external file/URL resolution is disabled. The independent audit parses
XML and embedded PNGs, decodes PNG references, and checks preview preservation;
it does not perform a second independent whole-SVG render.

Observed SVG-versus-PNG differences are preserved:

- Full DPR1: 23,833 changed pixels; mean absolute channel delta 0.14247/255, max 50
- Full DPR2: 96,293 changed pixels; mean 0.15237/255, max 60
- Transparent DPR2 crop: 92,656 changed pixels; mean 0.34547/255, max 255

These are measurements, not a passing equivalence tolerance. Vectors remain
vector; Image/Canvas/Text ink remain raster. No editable text or font-outline
promise is made. Broader effects, animated frames, stale Canvas tickets, hardware
DPR, other consumers/platform fonts, driver failures, exhaustive budgets and
large-scene performance are not physically established here.

## Automated and source scope

The core checkpoint passed **28 SVG tests**, **294 total composite tests** and
strict composite Clippy. The fixture passed **5 tests**, check, strict Clippy and
native build. Counts are overlapping package-scoped evidence, not a new full
workspace pass. [Core record](validation/2026-10-02-scene-svg-summary.json).

Binary SHA-256:
`1ce5d72467dfd4bcef27188e71798bd9867bb2af8e6103606bcb54938423c3b7`.
Source archive SHA-256:
`ed6bee80ee5eb628ca51966b5a238f6dd013526a95ca4e267ecab2a515be3bfa`.
Native QA matched all **574 archive entries** to its manifest and checked the
binary again after exit. Publication does not retroactively make this a
commit-pinned or reproducible build. This documentation audit checked retained
hash manifests without builds, native windows, code changes or Git-index writes.
