# Independent shadow-modes artifact audit

**PASS for the final CPU/helper artifacts only.** Audited 2026-10-03; no Cargo, Git, fixture execution, native window, product-source edit, helper-evidence edit, tolerance change or publication was performed by this auditor. Final machine-readable result: `final-audit-v3.json`, SHA-256 `520a3bba28c5da68f34aa824f7d1471a3b22e7a8d02d799ed66a77d0834433dd`.

## Exact audited batch and identity

All session paths below are under `/workspace/shared/shadow-modes-fixture-integration/`:

- Receipt `15-final-headless-tour.receipt.json`; session `final-headless/shadow-modes-headless-6-1791030732435188946`
- Receipt `16-final-cpu-tour.receipt.json`; session `final-cpu/shadow-modes-cpu-5-1791030749490658628`
- Receipt `17-final-text-cpu-tour.receipt.json`; session `final-text-cpu/shadow-modes-text-cpu-5-1791030758082889622`

All three exits are 0. Their complete before/after inventory contents and aggregate digests agree. Saved numbered events equal independently parsed stdout events: 274 headless, 38 CPU and 49 Text. Embedded compiled fixture bytes match their execution inventory and FNV identity.

- Final fixture SHA-256: `6b625d2935c54ebaf821e3d6b4efea03f1fc8ea476352cbfff423f828d34839c`
- 535-file source aggregate: `bd3da34d1ffd098703070ca03dd93e51fb34c75d23b071d765db73bbf4aa7149`
- `/workspace/shared/rust-target-native/debug/shadow_modes` SHA-256: `d3341d1be0ee3602277d9ddc1eb2cc36ca37f571770589f7724776b11f70d2cf`

The auditor independently rehashed the live 535 source inputs and executable before and after the final audit, at 12:37:37–12:37:41 UTC; both matched those identities. The 534 nonfixture inputs are unchanged from the first successful build inventory. This is a conservative code/assets/Cargo inventory, not an exact compiler dependency graph.

Read contracts: original `author-contract.md`, runtime `docs/shadow-modes.md`, and final `integrated-contract.md` (SHA-256 `12f540d62c28f04cd17eef44cfded221b1de3382b332acd0b684b20e01a0c6be`). The post-preflight fixture patch changes metadata/evidence retention only. Original numeric allowances remain unchanged.

## Reproduced results and precise counts

The three final sessions contain **1,003 raw files: 974 actual CPU output buffers plus 29 independent expected arrays**, not 1,003 rendered frames. There are also 33 genuine PNG files. All 1,409 final-session files are inventoried with byte counts and SHA-256; total 12,433,544 bytes. Individual session sizes are 9,580,173, 1,220,891 and 1,632,480 bytes, below their separate 16 MiB / 384-event limits.

- Matrix: exact scheduled state names, sources and six transforms reproduced for 37 platform-free states / 222 requests and eight Text states / 48 requests. Their outputs total **840 + 96 = 936 actual domain buffers**. Every saved output dimension, placement, byte length, FNV, premultiplication invariant and sample is checked. Per-request retained bytes independently equal the sum of its actual output lengths. Variant geometry/graph are unchanged apart from the two explicitly requested container paint-order fields.
- Numeric oracles: all 28 saved expected arrays independently regenerated **byte-for-byte** from analytical holed-path alpha, signed canvas-axis spread, f64 Gaussian, offset sampling and integer composition. Actual raw buffers and PNGs compare against this fresh reference, with maximum error **1 byte** for each class. Fixed original raw allowances are 1 byte at zero blur / 2 with blur; PNG allowances are 2 / 3 after explicit straight-RGBA-to-premultiplied conversion. No widening occurred.
- PNG: all 33 pass PNG signature, chunk CRC/structure, complete Pillow decode and dimension checks. The 28 oracle PNGs also match recorded decoded FNVs and independent arithmetic. The four canonical PNGs match recorded samples. Complete 128×64 decoded Consumers and Path-paint-zero PNG arrays are exactly equal.
- Tiny A8: blur 0 versus .004, zoom .5 / DPR 1, exact equality on the absolute 32×24 lattice; **39 nonzero pixels**, zero reference error. Both saved actual buffers are 14×11 at physical origin (5,5). Unshifted source probe (8,8) is [0,0,128,128].
- Crops: broad/tight absolute-lattice overlap errors are all zero. Tight alpha sums are **165** for paintless Frame, **9,952** for overflow Group, and **35,845** for nested source root.
- Text: complete offscreen-source output is 41×34 at physical origin (0,12), alpha sum **17,971**. Intentionally viewport-pre-cropped output is explicitly null and has zero alpha; no nonexistent raw file is counted. Source uses actual host Text producer calls; font/backend cache behavior is outside the artifact audit.
- Late failure: the saved valid retained buffer is **2,688 bytes**, FNV `e7b611986931d9c5`, matching all three error records. Exact reported errors are TooLarge for sigma below .001 / above 64, and UnsupportedTransform for nonuniform camera. Domains and OwnedCanvas report the exact guard UnsupportedShadowBackdrop; the public zero-width Box update reports the specific Scene Validation issue.

Source-reviewed defaults remain 32,000,000 work units, 128 MiB tracked pixels, 4,194,304 pixels per surface, 4096 per dimension, eight domains and depth 16. Matrix maxima reproduced from records: platform-free work **6,472,682**, peak **590,800 bytes**, retained **77,076 bytes**; Text work **2,155,000**, peak **462,736 bytes**, retained **106,000 bytes**, producer work **23,584**. Work/peak/release, model atomicity and host provenance remain instrumented/source-reviewed observations; the auditor independently verifies output arithmetic/counts, not allocator lifetimes or all internal work steps.

## Preserved earlier outcomes and audit correction

The first strict stage 01 exited 101. First helper stage 04 exited 1 after **48 events, zero matrix requests, zero raw files and zero PNGs**. Its 52-file, 149,045-byte partial evidence remains. The fixture expected the wrong model-error variant; the later exact Validation assertion corrected that expectation. This original run is never counted as successful coverage.

Successful preflights 07/08/09 retain 273/38/49 events and the same 222/28/48 coverage as later repeats. Their 841/65/97 raw files and 0/33/0 PNGs are historical repetitions, not extra coverage. `preflight-audit-v1.json` preserves missing metadata failures for late retained, tiny, crop and Text raw outputs plus the incorrect CPU platform-free manifest field. Final sessions repair those metadata gaps.

`final-audit-v1.json` passed the byte/artifact checks. `final-audit-v2.json` deliberately remains as a failed audit attempt: the audit excluded a nonexistent `paint_phase` key rather than the actual `paint_order` field, creating six false container geometry failures. Direct document comparison proved the only relevant changes were phase-box and phase-frame `stroke_after_children` → `before_children`, expressly required by the original contract. `final_checks_v3.py` corrects only that audit assumption. Original scripts and reports remain unchanged; no product/helper evidence or tolerance was altered.

## Acceptance boundary

No native-display pixels, screenshots, physical DPR, pointer routing, GPU upload generation, burst cancellation, native history interaction, close/exit acceptance, browser parity, cross-platform behavior, whole-process memory or performance is established here. The finite native gate and APP-04 owned-background-blur work remain separate. Retrospective environment provenance is correctly labeled in the integration contract; an original complete helper-process environment dump was not supplied. Default-budget success does not replace boundary/cap-minus-one/runtime race tests.
