# Controlled gpui_wgpu vendor

The vendored backend is based on the official
[gpui-fast-wgpu 0.1.19 crate](https://crates.io/crates/gpui-fast-wgpu/0.1.19).
Its upstream source is [gpui-fast v0.1.19](https://github.com/longbridge/gpui-fast/tree/fd15f82cd04839f1fd5e00111dd0cd9c9fe6468a/crates/gpui_wgpu)
at commit `fd15f82cd04839f1fd5e00111dd0cd9c9fe6468a`. The published source includes
the wgpu 30.0.1 compatibility adjustments. The retained benchmark comes from
the tagged source.

`LICENSE-APACHE` matches the published crate's license byte for byte. The
local bridge uses the same Apache-2.0 license.

The local manifest uses the published dependency versions, including
`gpui-fast` 0.1.19, `gpui-pre-collections` and `gpui-pre-util` 0.3.14, and
`wgpu` 30.0.1. The workspace patches the backend package to this directory so
native platform crates and Flow use one renderer and a shared GPUI core.

The Flow headless adapter uses upstream's production `WgpuRendererCore`,
including its frame recorder, shaders, pipelines, atlas and retained tile cache.
See [PATCH.md](PATCH.md) for the local changes and API contracts.
