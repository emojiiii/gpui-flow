# Native outline font binding acceptance fixture

Status: source fixture awaiting build and physical native acceptance. No native
result is claimed by this document. This fixture exercises the startup-only API
from [static-font-registration.md](static-font-registration.md), same-App GPUI
shaping and binding rejection. It does **not** implement Canvas `strokeText`,
stroke rasterization, outline placement, or atomic Canvas pixel publication.

## Sources and opt-in

The exact source edit allowlist is:

- `examples/studio/src/bin/outline_font_binding.rs`
- `examples/studio/Cargo.toml`
- `docs/outline-font-binding-fixture.md`

The new binary is omitted from ordinary Studio builds by `required-features`.
`outline-font-binding` enables registration API/helper checks only.
`outline-font-binding-x11` and `outline-font-binding-wayland` explicitly opt into
Flow's corresponding Linux factory capabilities. Enabling backend features
elsewhere does not silently grant Flow capability. Registration-only and other
targets retain the API's explicit `Unsupported`; generic Application factories
remain unsupported. No implementation files or dependency additions are needed,
and no Cargo.lock update is intended.

Both embedded source fonts are first-party geometric CC0 fixtures, included in
place rather than copied or regenerated:

| Source | Bytes | SHA-256 |
| --- | ---: | --- |
| `crates/flow-text/tests/fixtures/outline-quadratic.ttf` | 1540 | `315383d2601dcd74cbbaa73d7b897e38997123668a12f5e664403ffde4af4429` |
| `crates/flow-text/tests/fixtures/outline-cubic.otf` | 1576 | `cfb44080e7dd0a928b1baf5e870e3b382a71ff43049b3e25c6b9e8bf31537b71` |

The source manifest for a frozen acceptance snapshot must include the **actual
binary bytes** of both files, their generator
`crates/flow-text/tests/fixtures/generate_outline_fonts.py`, the three allowlisted
files, Cargo.lock, and the API implementation/dependency files used to build.
A hash listing or text-only source bundle is insufficient. No system/private font
is copied. Ambient native fallback is intentionally exercised by negative tests;
its font bytes are neither read back nor redistributed.

## Exact build and headless commands

After a build slot and disk allowance are granted, in the existing host native
toolchain environment:

```sh
. /workspace/shared/gpui-flow-native-env.sh
CARGO_PROFILE_TEST_DEBUG=0 cargo check --locked --offline -p flow-studio \
  --features outline-font-binding-x11 --bin outline_font_binding
CARGO_PROFILE_TEST_DEBUG=0 cargo clippy --locked --offline -p flow-studio \
  --features outline-font-binding-x11 --bin outline_font_binding -- -D warnings
CARGO_PROFILE_TEST_DEBUG=0 cargo build --locked --offline -p flow-studio \
  --features outline-font-binding-x11 --bin outline_font_binding
"$CARGO_TARGET_DIR/debug/outline_font_binding" --helper-tour
```

The sourced environment fixes jobs to one, incremental compilation off and dev
debug info to zero. Use the approved free-space watchdog, stop before available
space reaches 3.125 GiB, and preserve the 3 GiB hard reserve. Initial cache-based
estimate is 250–550 MiB additional build/link scratch; an independent frozen
executable copy needs additional real space (reflinks are unavailable). This is an
estimate, not authorization to build or delete files. Reassess measured space
before linking or making the frozen copy. No parallel Cargo processes.

An optional separate test harness runs four ordinary, headless helper tests:

```sh
CARGO_PROFILE_TEST_DEBUG=0 cargo test --locked --offline -p flow-studio \
  --features outline-font-binding-x11 --bin outline_font_binding
```

Prefer `--helper-tour` on the existing executable if another test-harness link
would threaten the reserve. It prepares both exact font sources and checks the
candidate reducer without constructing an Application, platform or window. Unit
checks additionally freeze source hashes/lengths and exercise an unbound run in
every list position. These helpers are not native-registration evidence.

## Native launch and receipts

Only the desktop-slot owner launches the frozen binary, for example:

```sh
FLOW_OUTLINE_BINDING_DIR=/absolute/authorized/evidence/root \
  /absolute/frozen/path/outline_font_binding
```

Each launch allocates a unique session directory with `create_new` receipt files:

- `0-source.font`, `1-source.font`: exact original bytes compiled into the binary
- `0-registered.font`, `1-registered.font`: immutable privately renamed bytes
  actually passed to startup registration, with per-launch SHA-256 in the trace
- Ordered `NNN-action.json` receipts, also emitted as JSON lines to stdout

Preparation uses fresh private aliases, so prepared-byte hashes differ across
sessions. Compare bytes/hashes **within** the session. The binary constructs one
Application through `application_with_static_outlines` with exactly two faces.
The startup receipt distinguishes the factory's verified direct-native→App
identity invariant from publicly observed App IDs: private direct IDs are not
exposed or independently queried by the fixture. Every actual shaped run then
uses `static_outline_font` with its real FontId, the owning App TextSystem Arc,
exact static style, epoch and cancellation state. Success must return the same
immutable PreparedFont object retained at startup, with glyph IDs in its range.

The visible sample panels paint the initial bound candidates' exact GPUI
`WrappedLine` objects, via their ordinary native `paint` method: TTF at 28px, CFF at 38px, both
wrapped at 340px. They use only `m`, `c`, space and `é`. These fonts intentionally
show geometric shapes rather than recognizable letter designs. Each panel names
its private native family and App ID. No independent shaper or fake outline
stroke is used. JSON includes actual run FontIds, GlyphIds, byte indices,
positions, line metrics and wrap boundaries. Glyph ID zero is a failed check,
not acceptable fallback evidence.

## Physical button matrix

The 1000×800 window contains both sample panels, all eleven checks in four rows
of three, per-button status, the latest result and the trace location. There are
no hidden offscreen controls. Visit each button physically; a startup-only
receipt does not replace the click tour.

| Row / column | Button | Required evidence |
| --- | --- | --- |
| 1 / 1 | TTF bound | All real shaped IDs equal its App ID; all binding lookups accepted |
| 1 / 2 | CFF bound | Same proof for the distinct second face |
| 1 / 3 | Both + wraps | Both accepted with more than one wrapped row each |
| 2 / 1 | Fallback only | `WQ` is absent from tiny faces; actual nonzero unbound native run(s) reject candidate |
| 2 / 2 | Mixed runs | `mc WQ mc` produces both accepted bound and rejected unbound run(s); whole candidate rejected |
| 2 / 3 | Wrong owner | Reuse real numeric ID with separate Noop TextSystem Arc; `WrongTextSystem`; no second native App |
| 3 / 1 | Wrong style | Wrong weight and wrong slant both return `StyleMismatch` |
| 3 / 2 | Cancelled | Cancelled candidate rejects with `StaleSnapshot`; fresh uncancelled lookup recovers |
| 3 / 3 | Invalidate + fresh | Old epoch rejects; both immutable bindings accept the new epoch |
| 4 / 1 | Add unrelated | Register original TTF under `Flow Fixture`, invalidate, new unbound ID rejects; original IDs/shape evidence stay unchanged |
| 4 / 2 | Add same alias | Register identical immutable TTF prepared bytes again, invalidate; original IDs, hashes and shaped evidence stay unchanged |

Both add-font buttons perform their native registration only on the first click;
later clicks reverify instead of growing the native database without bound.
Post-registration checks shape fresh lines and compare their IDs, glyphs,
positions and metrics with the initial displayed samples. These later receipts
do not claim those fresh objects themselves were painted.
The same-alias test is specifically **identical-byte duplicate registration**;
it does not claim hostile conflicting-byte replacement was exercised. Expected
rejections retain the displayed samples, and buttons remain usable. Repeat both
bound checks, fallback/mixed, cancellation and invalidation after font additions.

For each candidate, all actual runs are collected and logged before deciding;
no early rejection hides later runs. The reducer rejects empty/missing-glyph
results, any lookup error, cancellation and a stale final epoch. The mixed test
must observe both bound and unbound runs, otherwise it is a failure rather than a
fabricated pass. A host without usable fallback may fail the fallback check;
retain that evidence and report the environment limitation.

Capture screenshots before and after the tour and preserve the real JSON and
font files. Screenshots are lossy JPEG in the capture flow and are visual evidence
only, never exact byte identity, framebuffer equality or Canvas transaction proof.
The subsequent gate remains actual outline stroking, placement/transforms/DPR,
clips/alpha, work budgets and atomic Canvas publication.
