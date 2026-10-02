# Desktop screenshot evidence: format erratum

Recorded 2026-10-02. A read-only file-signature audit identifies **327 historical
desktop captures as JPEG bytes saved with `.png` filenames**. The format
names in earlier descriptions were incorrect. Original capture bytes and
filenames, retained raw QA reports, measurements and recorded hashes are
unchanged. Raw reports outside this repository predate this correction.

## Audited scope

| Evidence set | Desktop captures | Actual encoding |
| --- | ---: | --- |
| [Historical APP-05 state geometry](state-geometry.md) | 18 | JPEG, historical `.png` suffix |
| [INS-02 layered app](layered-app.md) | 38 | JPEG, historical `.png` suffix |
| [APP-05 coherent state](state-coherent-native.md) | 57 | JPEG, historical `.png` suffix |
| [Scene SVG desktop preview](scene-svg-native.md) | 25 | JPEG, historical `.png` suffix |
| [INT-06 drag/viewport](drag-viewport-native.md) | 58 | JPEG, historical `.png` suffix |
| [Canvas current-path functional run](canvas-current-path-native.md) | 63 | JPEG, historical `.png` suffix |
| [Canvas footer layout-only run](canvas-current-path-native.md#original-defect-and-layout-only-follow-up) | 8 | JPEG, historical `.png` suffix |
| [Open-stroke native run](open-stroke-native.md) | 60 | JPEG, historical `.png` suffix |
| **Historical total** | **327** | **JPEG** |

The first two rows share the state-layer package (56 captures). This inventory
is limited to these evidence sets and the current pattern package; it does not
establish the format of other historical captures.

The current Canvas-pattern package separately has **34 JPEG originals**, now
correctly named `.jpg` with identical bytes, plus **six clearly labeled decoded
PNG derivatives**. A PNG made from decoded JPEG preserves that decoded raster;
it does not recover a lossless renderer image. These are not included in the
327 historical captures.

## Interpretation of historical measurements

- Current-path's **20 pairs × 2 regions** with zero differences, SVG preview
  retention, and open-stroke's five history-region equalities remain observations
  of decoded JPEG screenshots. They do not establish lossless Canvas/GPU buffer,
  native/CPU renderer, browser or cross-platform pixel identity
- Screenshot-derived RGB, background patches, antialiasing, widths and threshold
  bounds concern captured pixels. JPEG encoding may alter those values. Hashes
  establish byte preservation, not lossless capture
- Open-stroke retains **41 focused decoded-capture checks** and the separate
  **20/21 within-one-pixel** auxiliary comparison. The original failed auxiliary
  `native-pixel-audit-initial.json` remains unchanged. The sample at `(948,662)`,
  RGB `(20,39,56)` versus `(11,15,24)`, extends a threshold box by four pixels.
  JPEG encoding may affect this low-contrast sample; its cause is unestablished
  and the capture alone does not identify a renderer defect
- The current pattern audit's **48/50** strict screenshot result also remains
  unchanged. The two exterior 5×5 samples contain 5 and 10 non-background
  decoded pixels. Their low-amplitude deviations may be affected by JPEG;
  neither renderer leakage nor compression is established as the cause
- Runtime trace generation/snapshot identity and temporary-surface path replay,
  source/binary hashes, native exits and qualitative window/control observations
  retain their own scope. Historical state-transition disappearance, loading
  intervals and footer clipping are not retroactively relabeled passes

## Actual PNG export evidence is separate

The SVG package contains **two actual native PNG exports and 24 actual embedded
PNG producer payloads**, all verified by signature. These are separate from its
25 desktop captures. Exported-file lengths/FNV/hashes, independent PNG decoding
and alpha checks, SVG structure/bytes, native resvg execution and native
decoded-export difference receipts remain valid within their recorded limits.
No PNG export or embedded-payload claim is converted to a JPEG claim.

No task status, frozen acceptance criterion or ledger count is changed by this
format correction: **31 bounded verified / 11 partial / 3 not started** remain.
Future pixel-precision claims require a verified lossless source; checking the
MIME type and file signature must precede choosing a capture suffix.

## Audit provenance

The retained audit package is
`/workspace/shared/canvas-pattern-native-validation-20261002T2243Z`.
It records every inspected raster file's signature, dimensions, size, category,
suffix mismatch and SHA-256. All 327 historical captures and 26 actual PNG
artifacts retained their hashes during the read-only inspection. The audit did
not rerun historical pixel checks or alter their numerical results.

- `capture-format-inventory.json`, SHA-256
  `5e302c16c543a3825ea6f2589b542019762289cab6926c17efd14fc08e93ad94`
- `capture-format-inventory.md`, SHA-256
  `0a0d3aa4431e8adae4e8554a7cda5110abf331ebc7966479be74a2ddb37040e4`

Those retained artifacts are not bundled into this repository. The summaries
linked by the reports above add explicit actual-format and interpretation
fields while retaining historical labels such as `original_pngs` where needed
for continuity; those labels must not be read as a file-format determination.
