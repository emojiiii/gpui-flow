# Reversible native evidence storage

Recorded 2026-10-03. Twelve completed acceptance executables are now stored as
lossless zstd archives in their original evidence directories. Their executable
bytes are fully retained; only verified redundant uncompressed copies were
removed. This reclaimed **1,776,095,262 bytes (1,693.8 MiB)**. No source, captures,
logs, historical receipts or active Cargo target was removed or rewritten.

| Completed fixture | Original bytes | Archive bytes |
| --- | ---: | ---: |
| `focus_keyboard_trace` | 177,528,168 | 30,200,355 |
| `resource_lifecycle_trace` | 177,300,696 | 30,150,296 |
| `custom_geometry_trace` | 177,152,872 | 30,124,679 |
| `scene_export` | 178,139,688 | 30,329,996 |
| `advanced_flow_layout` | 182,076,608 | 30,596,082 |
| `image_crop_trace` | 177,629,112 | 30,172,349 |
| `state_geometry` (original failed APP05) | 177,628,352 | 30,188,599 |
| `state_geometry` (coherent APP05) | 178,007,632 | 30,246,802 |
| `scene_export` (SVG) | 179,224,296 | 30,493,938 |
| `drag_viewport_constraints` (INT06) | 178,364,520 | 30,311,995 |
| `canvas_current_path` (functional acceptance) | 178,201,464 | 30,273,915 |
| `canvas_current_path` (footer-only recheck) | 178,214,344 | 30,283,484 |

## Recovery and rechecking

Each evidence directory contains `restore-binary.py`, a new `run.sh` or
`run-archived.sh`,
`binary-archive.json`, `BINARY-ARCHIVE.md` and its executable `.zst` archive.
Run `./restore-binary.py` to restore the original executable path. It verifies
the archive and restored SHA-256/size, restores the recorded mode and mtime,
refuses to overwrite a different existing destination, and enforces a 3 GiB
remaining-disk reserve.

Run the new restoration-aware entry named in `binary-archive.json` only when
a native recheck is intended. It restores first
and records results in a new timestamped subdirectory. Original historical run
scripts remain unchanged and do not automatically restore the executable.

Every restoration script was exercised against an independent temporary path.
For the first five and the final two, restored SHA-256, size, permissions and mtime matched before
raw removal. Under tighter storage headroom, the middle five first passed full
streamed decompression SHA and byte-for-byte comparison against raw; only then
was the redundant raw copy removed, immediately followed by the same actual
temporary restoration/metadata verification. Every recorded step stayed above
3 GiB. The original failed APP05 run remains a failure; its screenshots, logs
and old run.sh are unchanged. Hashes of **883 existing evidence/source files**
remained unchanged. No native application was launched during this work.

Historical binary manifests and reports are unchanged. Their raw paths become
available again after restoration; the stored original executable SHA-256
remains authoritative. This storage change does not create new native acceptance,
change any prior failure, or alter the 45-task ledger.

The [machine-readable receipt](validation/2026-10-03-native-evidence-storage-summary.json)
records original paths/metadata, archive and script checksums, recovery tests,
and protected-file counts. Archives remain local to the retained evidence
packages and are not bundled in this repository.
