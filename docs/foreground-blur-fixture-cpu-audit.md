# Foreground-blur CPU artifact audit

PASS: 136 independent original-artifact checks and 128 frozen-replay/provenance checks; no failed audit assertions. Original artifacts and source were read only. No Cargo, build, fixture/native execution, Git operation, or remote write was performed by this auditor.

## Canonical CPU findings

- All 45 original printed JSON events equal their numerically ordered saved event objects; all 45 frozen events also match. Events are direct objects. Dictionary key order is ignored, while event sequence and array order are preserved.
- All 50 original raw premultiplied RGBA files passed actual byte length, dimensions, FNV-1a64, every recorded pixel sample, and premultiplied-channel checks. These are 36 matrix outputs plus 8 Gaussian oracles and 6 crop outputs.
- Six real PNGs passed signature and independent Pillow 12.3.0 decoding, encoded length/hash, decoded dimensions/hash, every metadata sample, and source-document checks. Every image is 180 × 110 straight RGBA after decoding; raw frames remain separately identified as premultiplied RGBA.
- Exactly 20 safe requests cover five documents × DPR 1/2 × zoom 1/2. The Reject case uses its explicit Normal recovery for this matrix. Domain counts are 2/2/1/3/1. Limits remain the defaults: 32,000,000 surface writes, 134,217,728 tracked bytes, and eight domains. Maximum observed work is 10,993,880; maximum tracked peak is 2,013,424 bytes; maximum retained output is 1,548,800 bytes. All 20 retained counts equal independently summed actual output byte lengths, and all record release to zero. Work, peak, and release are helper instrumentation checked against artifacts/source; this audit does not independently rerun allocation accounting or measure process/GPU memory.
- All three generous/tight comparisons are exactly byte-equivalent throughout the requested viewport, with transparent zero-fill outside retained frames: edge 280 comparison bytes/alpha sum 112; Frame 1,104 bytes/224; nested clip 1,368 bytes/68,058. The Frame’s 12 × 23 request legitimately retains a bounded 12 × 22 frame; the additional row is checked as transparent.
- Independently decoded offscreen root halo is [255,0,0,14]; paintless Frame entering child is [255,0,0,14], and its parent shadow is [0,255,0,14]. Exported source confirms the Frame has neither fill nor stroke.
- Soft alpha mask halo is [255,0,0,38], Pixel eraser halo [255,0,0,217], hard Path interior [0,0,0,0], and Path exterior [255,0,0,255]. Later green paint survives both eraser cases. Path blur 0 and 1 differ only in that source field and produce identical whole decoded and encoded PNG bytes.
- Eight independent Python binary64 finite Gaussian references reconstruct the input and apply explicit transparent padding, then grayscale and opacity when requested. At sigma 0.001/0.5/1/2, plain maximum channel errors are 0/0/0/0 against fixed tolerance 1; grayscale+opacity errors are 0/1/1/1 against fixed tolerance 2. All independently computed expected FNV hashes match the recorded expected hashes. Tolerances were not widened. The tiny-sigma kernel is [0,1,0]; this verifies the documented local one-tap correction, not upstream image-crate byte parity.
- Planner checks, typed physical rejections with zero work/peak/retained bytes, geometry/hit invariance, atomic invalid-model rejection, undo/redo/navigation assertions, and unsafe PassThrough rejection metadata are consistent. The unsafe export produces the expected UnsupportedBlurBackdrop and no unsafe PNG.

## Identity and formal replay

The final strict/build/original-helper source inventory contains 529 files with aggregate SHA-256:
61413cb6eef31855db361366a93376d42b587d3e8c5e3c33931f87d56f604fd1

Every recorded manifest entry was checked against the current source bytes during the original audit. The two embedded compiled-fixture-source files equal the integrated fixture and their FNV identities. The inventory is a conservative code/assets/Cargo selection, not a compiler dependency graph. Original strict attempt 01 remains preserved with exit 101 (missing .iter()); final strict 02, build 03, and helpers 04/05 have verified exit-zero receipts and log hashes.

Historical limit: helpers 04/05 did not record executable SHA at invocation time. Their current executable hash is only a read-only observation, not retrospective run-time identity proof.

Formal helpers 06/07 ran the frozen copy and recorded the same executable SHA before and after each run:
164a5130ff32d177f49a1ac382ff175fffa13bb1046c57feb7c69a89d3ce9beb

The frozen binary is 180,366,392 bytes. Its current SHA, copy receipt, binary.sha256 file, both run-time before/after receipts, source manifests, and log hashes agree. Independent direct comparisons prove all 56 corresponding raw/PNG files are byte-identical to the original audited outputs. All 57 session JSON files are structurally equal; both embedded source files are byte-identical. The parent’s frozen-helper-byte-replay.json was independently verified by keyed (kind, filename) correspondence. Thus the independent pixel/oracle audit carries over to the formal SHA-bound batch without weakening tolerances or replacing the original batch.

## Boundary and reproducibility

This is canonical CPU/headless evidence. It establishes no native application/input acceptance, physical display DPR result, GPU upload success, or native/framebuffer pixel equivalence. Those stages were not performed by this audit.

- Original checker: audit_cpu_artifacts.py
- Original full result and original-file hashes: audit-pass-01.json
- Frozen checker: audit_frozen_replay.py
- Frozen result and all 56 payload pairs: frozen-replay-pass-01.json

Both checkers preserve prior result files and create a separately named failed-draft result on an exception/assertion failure. Both first runs passed; there was no failed audit draft to correct. All writes made by this audit are inside this artifact-audit directory.
