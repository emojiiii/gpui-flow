# Cloud workspace recovery — 2026-10-03

At approximately 08:51:48 UTC, the execution host changed from `0e15a1f8c187` to `d2b65af346d2`. The original `/workspace/shared` project, caches and raw QA packages were no longer available. This was an environment replacement, not an observed gpui-flow runtime failure.

The latest already-published source was independently verified at `761b626b5c9c77de1c2fdf5fe4c3751114ffb1e7`, with tree `1e8a27f6fac93ac1c425b2cfa3ef0d93bbb7b5dd`. The successful remote write was not repeated. Its local publication receipt could not be saved before the old directory vanished; fresh remote ref/commit reads confirmed the result afterward.

A new checkout restored all 679 repository files, 7,882,661 bytes, from immutable commit paths through the existing authorized repository connection. All file sizes, Git blob hashes, byte contents and modes were independently verified, as were all 77 tree objects. Git independently produced the exact expected tree. Local checkpoint `43d5061c3ec649be2d08fc50b41b7f0c38250fbf` is a fresh matching-tree recovery commit; it is not a claim to have restored the complete prior local Git history.

Official Rust/Cargo 1.98.1 and rustfmt/clippy were restored with vendor SHA-256 verification. Native development files came from signed Debian package metadata, with package hashes checked. Compiler, native linking and libclang parsing smoke checks passed. Locked dependency fetch completed successfully without changing Cargo.lock. Project rebuilding and its later native outcomes must be recorded separately.

The interrupted grayscale run in the old `0844Z` package reached row 6. Rows 7–15 were unfinished, and no normal process-exit receipt was obtained. Seventeen capture metadata entries and 27 click records could be preserved from already-returned observations; no original JPEG or native trace file was recovered. This metadata is not reconstructed raw evidence and does not establish a whole-tour pass. The final visual observation of the first burst is likewise not a sealed acceptance result.

Earlier published validation reports and source/asset files remain in the repository. Their reported historical runs are distinct from the original raw screenshots, traces, binaries and archives that are now unavailable. Existing report hashes are historical integrity references, not proof that those raw files have been restored on this host. Future runs use new binary hashes, source manifests, directories and evidence packages; old paths and missing images are not fabricated.

The fixed ledger remains 33 bounded verified / 9 partial / 3 not started. Grayscale remains a bounded source increment under partial APP-04. The interruption does not upgrade its native status or erase the remaining required effects.

## Checks on the replacement host

The grayscale fixture rebuilt successfully at 09:23:11 UTC after a 10m34s cold build. All 679 tracked repository files were unchanged before/after linking. This is a conservative repository snapshot, not a compiler dependency graph; compiler, dependencies and system libraries are recorded separately. The new executable SHA-256 is `b9a943e40e7bb6bc15258ca4165870fd0411fbc6ef381992e44cd318688dcc6c`, 179,993,592 bytes. It is a new build, not recovery of the old executable.

New-host checks passed: 24 focused UI/compositor tests, 7 model/editor/state tests, the headless planner/history helper, the CPU helper with 18 premultiplied cases and seven PNG exports, and 115 independent Pillow decode/sample checks. The earlier 1,701 headless and 279 UI full-suite results remain explicitly historical; they were not rerun wholesale during this recovery. Exact commands, logs and hashes are referenced in the [recovery summary](validation/2026-10-03-cloud-recovery-summary.json).

The new `0927Z` native package starts a complete independent tour on this host. Its current native verdict is pending. Actual original screenshots and stable trace/source/CPU evidence are backed up to a new private Library identity in clearly partial versions after row groups, then a complete version after final sealing. The old metadata-only backup remains separate. No missing old image or trace is fabricated.
