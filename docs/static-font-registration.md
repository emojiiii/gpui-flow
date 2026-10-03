# Startup-only static font registration

Status: an opt-in registration/binding checkpoint with state-machine tests,
Linux backend compilation and [bounded X11 native acceptance](static-font-binding-native.md):
28 physical actions, 21/21 trace checks and independent font-byte audits passed. This does **not** enable Canvas `strokeText`, stroke rasterization,
new shaping, or cross-platform outline support. The CPU parser/outline foundation
is documented separately in [static-font-outlines.md](static-font-outlines.md).

## API and boundaries

Enable `gpui-flow-text/static-outline-x11` and/or `static-outline-wayland` for
native capability. `static-outline-registration` alone compiles the API and mock
checks but the factory returns `OutlineBindingError::Unsupported`. Other targets
also return Unsupported even when one of the Linux features is enabled. Feature
unification through another dependency does not silently opt Flow into this API.

Prepare host-authorized bytes with `PreparedFont::prepare`, place each immutable
font in an `Arc`, and pass a `Vec<StartupOutlineFont>` to
`outline_registration::application_with_static_outlines`. Each request includes
its exact `StaticStyle`. There must be 1–32 fonts, at most 64 MiB of cumulative
prepared bytes, and no duplicate private aliases. Admission validates the complete
batch before constructing the platform. A style needing synthesis is rejected.

The factory creates its own fresh platform with the audited pinned
`gpui_platform::current_platform(false)`. It accepts no existing application,
platform, native text-system handle, callback or executor task. Do not use it to
add fonts to an already-running application. It must run on GPUI's main thread.
Native creation/registration/resolve panics are converted into an error under
unwinding builds; aborts and process termination cannot be recovered.

The existing `GpuiTextRasterizer::application(platform)` stays source-compatible
and always has unsupported outline capability, regardless of the concrete
platform a caller supplies. The public GPUI trait cannot prove backend identity,
face bytes, private ownership, or the absence of an earlier cached query.

## All-or-nothing construction

1. Check exact styles, unique aliases, font count and cumulative bytes
2. Privately create the pinned Linux platform and retain its exact text-system Arc
3. Probe every alias directly through `PlatformTextSystem::font_id`, with default
   features, its admitted weight/style, and `fallbacks: None`
4. Treat only the exact outer error
   `requested font family contains no font matching the other parameters` as
   absence. Success is a collision, including a match through a secondary family
   alias. Every different error or panic fails closed
5. Register exactly one immutable prepared single-face buffer per call. For that
   face directly resolve the previously unused descriptor with the same features,
   weight and style, but `fallbacks: Some(FontFallbacks::default())`
6. Reject duplicate numerical native FontIds within the batch
7. Construct the Application with that same private platform and verify that every
   `app.text_system().resolve_font(live_descriptor)` equals the corresponding
   directly resolved native FontId
8. Only after the whole batch passes, return the Application and raster bridge

The native loader can silently ignore invalid font bytes while returning `Ok`,
so `add_fonts` success is never the binding proof. `TextSystem::resolve_font`
silently falls back on error and is never used for the absence probe or initial
live resolution.

Cosmic caches an empty family lookup. Its key contains family, features and the
fallback **option**, but excludes weight and style. The `None` probe is deliberately
sacrificial; changing weight after poisoning a key cannot repair it. A failed live
`Some(empty)` resolution ends the entire construction. Nothing retries that alias
inside the same native system. No partial App/bridge or registration record is
returned on failure. This is disposal of the private system, not transactional
removal of fonts from a shared font database.

## Immutable bindings and the next integration gate

The bridge's records and constructors are private. Each binding retains the
native FontId, exact live descriptor, immutable PreparedFont (including exact
bytes, source/registration hashes, index zero and actual style), and registration
epoch. The records additionally retain the exact owning App TextSystem Arc.
Integer FontId equality or a family string is never an ownership proof.

`static_outline_descriptors()` returns clones of the admitted descriptors in
startup order. `static_outline_font(system, font_id, style, epoch, cancelled)`
first checks the current bridge epoch/cancellation, then the exact TextSystem Arc,
then the known ID, registered epoch and exact static style. It returns an immutable
reference to the PreparedFont. The append-only native loaded-font vector means
unrelated later font registrations cannot reassign an existing bound ID. The
host still invalidates the bridge on resource changes; future snapshots use the
new epoch without mutating the original bindings.

A later outline consumer must use the existing same-App GPUI shaped runs and their
positions, check **every actual run FontId**, and reject an entire candidate on
any unbound fallback. The primary descriptor resolving successfully is insufficient:
shaping can choose another face for individual glyphs. The separate native fixture exercises actual shaped IDs for its two regular
first-party faces; that evidence does not establish every possible font or run. Cancellation and epoch must be
rechecked while doing work and before any atomic publication; a successful lookup
is only a moment-in-time check. Stroke geometry, placement, transforms/DPR, clips,
alpha, allocation/work budgets and Canvas transaction publication remain separate
gates.

## Audited source assumptions

This implementation depends on the exact GPUI source in Cargo.lock:
`10d005166a9f9e079d55ebfb700035f3860c3e71`. The optional `gpui_platform`
dependency uses the same Git source identity as workspace GPUI to avoid duplicate
incompatible GPUI types; the lockfile, rather than a different Git `rev` query,
pins both. Re-audit these invariants before updating that lock:

- `crates/gpui_platform/src/gpui_platform.rs:57–74` and
  `crates/gpui_linux/src/linux.rs:30–59`: fresh platform/client construction
- `crates/gpui_linux/src/linux/platform.rs:159–184,272–274`: a fresh Cosmic Arc
  when x11 or wayland is enabled, and clones of that exact Arc. Linux alone can
  choose Noop, which is why explicit backend features are mandatory
- `crates/gpui/src/app.rs:785–800`: App captures this platform's text system
- `crates/gpui_wgpu/src/cosmic_text_system.rs:27–30,161–179`: native family cache
  key and sticky query behavior
- Same file `:299–305,345–389`: buffer registration, all family aliases, real `m`
  admission, and append-only loaded-font IDs
- Same file `:531–537,655,725–767`: native shaping can change run FontIds
- Same file `:822–892`: exact empty-match error for both matching implementations

## Verification scope

Focused tests exercise real prepared fixture bytes through a deterministic model
of the pinned native cache and registration protocol, plus real GPUI TextSystem
Arc identity with a Noop system. They cover single/multiple fonts, byte identity,
exact styles, duplicate aliases, primary/secondary alias collisions, the exact
absence error, `None` versus `Some(empty)` poisoning, ignored loads, errors/panics
at each phase, partial-batch disposal, duplicate IDs, App resolution mismatch,
wrong-App numerical IDs, unbound IDs, immutable bindings after unrelated
registration, generic/no-backend rejection, epochs and cancellation.

They do not register a real native font, exercise a native window or prove an
actual shaped-run FontId. The separate [native fixture](static-font-binding-native.md) has exercised two
registered faces and actual fallback/mixed-run rejection. An outline consumer
still requires its own placement, geometry and publication acceptance.
Cross-platform builds are untested.

### Local receipt (2026-10-03)

- Registration-only full crate test: 53 unit tests (13 binding/registration tests)
  and one existing doctest passed, zero failures/ignored tests
- Strict all-targets Clippy passed both registration-only and x11+wayland feature sets
- x11+wayland all-targets `cargo check` passed; no native platform was constructed
- Changed Rust files passed formatting, and `git diff --check` passed
- No native-backend test binary was linked and no window was launched
- Jobs were limited to one in the existing native target cache, with a watchdog
  stopping at 3.125 GiB available. Backend checking grew the cache by 287,387,648
  bytes; the final test/Clippy group grew it by 3,457,024 bytes. Final availability
  was 3,753,873,408 bytes (above the 3 GiB hard reserve)

Reproduction, with the existing host native toolchain environment configured:

```sh
CARGO_BUILD_JOBS=1 CARGO_PROFILE_TEST_DEBUG=0 cargo test --locked --offline \
  -p gpui-flow-text --no-default-features --features static-outline-registration
cargo check --locked --offline -p gpui-flow-text --no-default-features \
  --features static-outline-x11,static-outline-wayland --all-targets
cargo clippy --locked --offline -p gpui-flow-text --no-default-features \
  --features static-outline-registration --all-targets -- -D warnings
cargo clippy --locked --offline -p gpui-flow-text --no-default-features \
  --features static-outline-x11,static-outline-wayland --all-targets -- -D warnings
```
