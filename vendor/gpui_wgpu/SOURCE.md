# Controlled gpui_wgpu vendor

Source: https://github.com/longbridge/gpui-fast/tree/26b0dccc97ebd14bbf9715741b6688132ba69491/crates/gpui_wgpu

Commit: `26b0dccc97ebd14bbf9715741b6688132ba69491`

Only the original 23 files under `crates/gpui_wgpu` were extracted. No other
upstream crate, workspace, Cargo cache, or Kit source is vendored. Original
`LICENSE-APACHE` contains the byte-for-byte original license text, materialized
from the upstream symlink chain `crates/gpui_wgpu/LICENSE-APACHE` →
`../../LICENSE-APACHE` → `LICENSE` so it remains valid in this standalone vendor. The upstream crate declares
Apache-2.0; this local bridge is supplied under the same license.

The local Cargo.toml expands inherited upstream workspace package/dependency
settings to the same explicit versions. gpui, collections and gpui_util retain
the same official git URL and explicit revision as Flow's single dependency graph.
The official gpui-pre facade crates re-export that same core; Kit is independently
pinned to its compatible official revision. Do not update those
packages independently or switch to a different revision for this vendor.

The headless host is adapted from the original
`src/fast/layers/tests.rs::Harness`. The production frame recorder, shader
sources, pipeline construction, atlas and tile cache are the implementation
used for all resulting pixels. The original upstream test harness is untouched.

See PATCH.md for the complete changed-file and API contract inventory.
