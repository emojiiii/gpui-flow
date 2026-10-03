# Explicit state geometry and per-state transitions

## Bounded contract and source

APP-05 now extends the existing state presentation path with explicit geometry and
local per-state transition settings. It does not mutate canonical scene data,
instance identity, revision, editor history, playback, or resource ownership.

Pinned Leafer references read through the GitHub connector:

- [`state/index.ts`](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/state/src/index.ts): state registration and animation exclusions
- [`state/style.ts`](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/state/src/style.ts): layer ordering, disabled suppression, transition/transitionOut fallback, and retargeting

Upstream assigns arbitrary input fields temporarily. This adapter deliberately
uses a finite typed property set and a private presentation projection instead;
there is no stale normal-style snapshot to write back over external edits.
Existing `StatePaint`, `StateStyles`, `StateTransition` and `StateTransitions`
struct layouts remain source-compatible. No new dependency is introduced.

## Supported geometry

`StateGeometry` contains four independently optional absolute values:

- `x`, `y`: local-to-parent affine translation (`tx`, `ty`) on every element kind
- `width`, `height`: intrinsic shape box dimensions on Rect, Ellipse, Text, Image,
  Box and Frame only

Values must be finite; sizes must be strictly positive and satisfy the existing
shape validator. The untouched affine linear coefficients, shape kind, content,
source/crop, corners, path commands, and other fields remain canonical. This is
not a bounding-box scale approximation. Width/height on Group, Line, Polygon,
Star, Path, Pen, Custom and Canvas return an explicit error, including inactive
patches. Canvas translation is supported; Canvas state resizing and indirectly
layout-resized Canvas surfaces are rejected because pixel resources are keyed to
canonical surface dimensions.

There is no rotation/scale/skew state decomposition, arbitrary geometry attribute
map, custom-property state animation, per-state animation timeline, delay/repeat,
gradient/absent-fill interpolation, or declarative state serialization. These are
explicit exclusions, not silently accepted fields.

## API and precedence

First register the existing paint binding, which may be empty. Then use:

- `set_geometry_styles(engine, handle, StateGeometryStyles)` for selected,
  placeholder, focus, hover, press and disabled geometry
- `set_named_geometry(engine, handle, BTreeMap<String, StateGeometry>)` for named
  geometry; `select_state` accepts a name defined by paint, geometry, or both
- `set_state_transitions(engine, handle, StateKey, Option<StateTransitionOverrides>)`
  to set/remove an individual local timing override

Named geometry merges below selected, placeholder, focus, hover and press.
Disabled removes focus/hover/press, even if its own patch is empty. Each field
inherits independently. Button inheritance uses the existing parent/grandparent
rules: inherited names and flags select the child's local geometry and timing;
parent geometry and transition settings are never copied. Placeholder content
is still substituted only after layout, so it never becomes an intrinsic text
measurement input. Explicit placeholder geometry can change the presented box.

Each geometry palette and each named-paint palette is limited to 128 entries;
transition overrides have a 134-entry cap (128 names plus six builtins). Existing
name, binding and placeholder limits remain unchanged. Rebinding paint retains
geometry and timing settings. Unbind/clear remove the whole runtime binding.

```rust
use gpui_flow_interaction::scene::{
    StateGeometry, StateGeometryStyles, StateKey, StateStyles,
    StateTransition, StateTransitionOverrides,
};

sheet.bind(&editor.engine, handle, StateStyles::default())?;
sheet.set_geometry_styles(&editor.engine, handle, StateGeometryStyles {
    hover: StateGeometry { width: Some(180.), ..Default::default() },
    press: StateGeometry { width: Some(150.), ..Default::default() },
    ..Default::default()
})?;
sheet.set_state_transitions(&editor.engine, handle, StateKey::Hover, Some(
    StateTransitionOverrides {
        transition: Some(StateTransition::new(180)),
        transition_out: Some(StateTransition::new(320)),
    }
))?;
```

## Per-state timing

Entry uses state entry, then binding entry. Exit uses the first defined setting
in this order: state exit, binding exit, state entry, binding entry. This mirrors
the pinned upstream fallback, including a binding exit taking precedence over a
state entry. An explicit zero duration snaps. `None` removes an override rather
than configuring zero duration.

When several input flags change in one frame, disabled entry/exit wins; otherwise
the highest-priority newly added layer chooses entry timing, then the highest
removed layer chooses exit timing. Simultaneous additions prefer entry. A changed
patch on an otherwise unchanged effective state uses the highest active layer's
entry settings. These are deterministic frame-level transition rules, rather
than a promise to reproduce the ordering of several upstream JS setters.

The shared `TweenValue` number/color sampler supplies interpolation and easing.
Every numeric geometry axis is independent. Rapid reversals sample the old
motion at the current host timestamp. Unchanged targets do not restart. A
canonical edit to an axis cancels only that axis's old motion; active overrides
still apply, and clearing them reveals the new canonical value. Reconfiguring
binding/per-state timing or calling `finish_transitions` finishes existing motion.

## Layout, paint and hit consistency

Geometry is first converted to the existing typed Transform/Shape property
batch. `SceneEngine::preview_properties_batch_with_origin` reuses the real
normalization, shape validation, installed reactive-layout and animated-property
ownership checks without committing anything. The final endpoint is preflighted
even at zero progress, so an invalid target does not start a seemingly valid
transition and fail one frame later. Layout can move unbound siblings and resize
containers in the same private scene. A provider that overwrites a requested
Transform or Shape is rejected atomically, including overwriting another axis of
the same property. No geometry field is silently discarded.

Paint and placeholders are applied after the geometry preview. Presentation and
picking use the same sampled transforms, shape dimensions, derived layout and
ancestor clips. The accepted geometry-only scene is held in an inert private
SceneEngine with the original registry; it therefore reuses the actual cached
bounds, hit policies, custom hit callbacks and Image candidate path. No parallel
hit algorithm is introduced. Native Image alpha filtering uses presented shape
and local coordinates but canonical live handles and resource tickets.

Picking intentionally retains canonical paint/opacity, preserving the earlier
contract that a transparent hover style does not remove its own hit target.
Editor handles, mutations, fit, saved files and exports remain canonical. Hosts
should use read-only controls for layout-displacing state interactions when a
canonical edit affordance would be confusing; this does not claim state-aware
resize/drag handles.

The read-only `geometry_engine(&canonical)` and `geometry_presentation(&canonical)`
return the last accepted geometry, not a newly sampled time. They return None
when geometry equals canonical or the canonical revision, document epoch or
layout-provider ID changed. Definition registries are immutable per engine.
Repeated identical projected geometry reuses the private engine. Failed
presentations discard stale picking data while preserving the prior clock and
motion. Native conversion fallback calls `invalidate_geometry_presentation()`
before drawing canonical content. Detached/remounted sources retire old motion;
reset/replacement never transfers bindings to a reused public ID.

This bounded implementation clones/validates projected scenes and may prepare
layout twice per changed frame. It makes no incremental-performance claim.

## Verification and native exercise

2026-10-02 dot-cloud Linux: 74 focused tests passed (23 state geometry, 3 pure
property preview, and 48 pre-existing state/named/transition regressions).
`cargo clippy --locked --offline -p gpui-flow-engine -p gpui-flow-interaction
--all-targets -- -D warnings` passed, as did formatting for the owned Rust files.
These are focused results, not a workspace or real-window acceptance claim.
Native integration/example checks are recorded by the integration owner.


Focused headless tests are in `flow-interaction/tests/state_geometry.rs`, alongside
unchanged named-state, state-style and transition regressions. Coverage includes
real cached hit/paint/bounds, clip updates, six shape sizes, independent axes,
rapid retargets, disabled priority, per-state fallback, canonical edits and Undo,
button inheritance, custom hit definitions, Canvas rejection, provider identity,
placeholder measurement isolation, failure atomicity and lifecycle cleanup.

```bash
cargo test --locked -p gpui-flow-interaction --test state_geometry \
  --test state_styles --test state_transitions --test named_states
cargo run --locked -p flow-studio --bin state_geometry
```

The native `state_geometry` demo uses the existing SceneView and Kit controls.
Hover/press resize a rounded Frame; named state also moves it; disabled snaps
size and suppresses pointer layers. Focus/Blur, base edit, Undo and Redo make
interrupted/overlapping states inspectable. Test clicks in the expanded-only area
and the moved card, rapid enter/leave/press, disabling mid-motion, editing base
while active and clearing states. Runtime toggles print canonical revisions;
only base edits/Undo/Redo should change them. Building the example is not native
window acceptance; the integration owner records native results separately.


## Historical native functional result and transition defect

The capture-format erratum (historical record in Git history) identifies these original desktop captures as lossy JPEG bytes
saved with `.png` filenames. Original files, raw reports and hashes remain
unchanged. Screenshot measurements describe decoded captures, not lossless
renderer pixels; trace-backed results retain their separate scope.

The exact source checkpoint [`1b79a3bb`](https://github.com/emojiiii/gpui-flow/commit/1b79a3bb9d895ac9be29fff5dd58ee151dc229da)
ran on the dot-cloud Linux desktop from 19:01:57 to 19:05:14 UTC on 2026-10-02,
with normal exit 0 and empty stderr. All 51 raw records and 18 saved screenshots
are indexed in the evidence summary (historical record in Git history).
The complete 560-blob source archive matches tree `365162c249e55f4975be3498cfee9cea94c29151`.

Real wheel and click input hit the expanded-only area and a named relocated card.
Disabled styling suppressed pointer geometry; clearing an active override revealed
a newer canonical edit, and one Undo/Redo restored the correct canonical poses.
State-only input kept revision zero. Focus/Blur and interrupted retargeting were
also observed. The final shared checks passed 230 UI library tests, 55 focused
wheel/router/focus tests, 3 mirror tests, and strict UI/interaction and fixture
Clippy; these overlap separately reported feature checks and are not additive.

At that checkpoint, APP-05 remained **partial** because an actual rendering defect was observed:
CPU-composited child text or a whole composited domain can disappear during a
transition while changing geometry keys cancel/replace pending work. Settled
pixels recover, but that does not establish correct continuous presentation.
A coherent previous presentation must retain its matching hit geometry until the
replacement is ready; simply drawing old pixels with new hit coordinates is not
an acceptable fix. The frozen receipt remains valid evidence of the defect.

`SceneEvent.local_position` is intentionally calculated by canonical event
routing, like edit handles and commands. State-aware picking uses presentation
geometry. For visual local coordinates in asynchronous `SceneView`, convert
`event.world_position` through the accepted `SceneView::presented_geometry()`
when available. The lower-level `state_styles.geometry_engine(&editor.engine)`
can describe a newer sampled state, so it does not by itself identify displayed
geometry. This is an explicit coordinate boundary, not arbitrary upstream
state/event equivalence.

## Coherent asynchronous CPU presentation follow-up

The follow-up renderer separates the latest sampled state from the displayed
presentation. `SceneView` keeps at most one displayed snapshot and one admitted
CPU snapshot. A snapshot contains all native objects, domain ordering, and the
matching canonical-paint geometry engine. The state clock and retargeting continue
while a worker runs; intermediate samples coalesce, and the next admission uses
current time. A slow admitted worker is not cancelled by subsequent state ticks.

CPU completion alone does not publish. All candidate domains first pass a real
GPUI atlas upload in the paint phase, using positive image bounds inside an explicit zero-opacity parent. Once the complete batch has uploaded and retirement capacity is
available, the next render swaps the entire native/CPU presentation and picking
geometry together. No old-pixels/new-hit interval or synchronous UI rasterization
is used. The old complete presentation stays visible while this happens.
`SceneView::presented_geometry()` reports the displayed geometry; the lower-level
`SceneStateStyles::geometry_presentation()` continues to report the latest valid
state sample and should not be used to infer asynchronous display acceptance.

Canonical engine identity/revision/document epoch, attachment identity, layout
provider, window/viewport/origin/clip/DPR, resource/playback generation, font
backend, compositing mode, layer owner, crop mode, background and grid changes
invalidate retained presentations. Picking checks provenance directly, including
before another render, and is suppressed when no valid displayed presentation
exists. These hard boundaries may still show a loading interval. They must not
reuse removed objects or stale resource pixels. A post-preflight atlas/paint
failure also withdraws the displayed hit snapshot and reports an upload error.

Existing pixel limits remain: one CPU job, 32 MiB output batches, 32 MiB retirement,
and the shared 128 MiB adapter surface ceiling. Warmed candidates superseded by
hard invalidation are charged until retired; a queued obsolete upload batch blocks
new work until it drains, preventing hidden GPU allocation accumulation. This is
coherent bounded asynchronous presentation, not a guaranteed frame-rate claim.

The native `state_geometry` card now explicitly uses Normal isolation, so hover,
press, focus, named motion and disabled transitions always exercise the CPU route.
Its input logs report accepted geometry and compositor status. Recheck rapid
enter/leave/press and named reversal without blank text/card frames, then click the
visible expanded/moving edge. Also recheck canonical edit/Undo/Redo, zoom, disabled
mid-motion and close during work. Native acceptance for this follow-up must be
recorded separately from the earlier functional-only result above.

Initial follow-up local checks on 2026-10-02, before independent source review:
239 UI library tests passed,
including nine new controller/whole-presentation regressions (the prior 230 are
included, not additional). Strict Clippy passed for UI library/tests and the
`state_geometry` demo, and the demo linked successfully. The new tests exercise
partial batch upload, wrong upload identity, held geometry/native-paint coherence,
399 busy samples with a rapid reversal, stale A/B/A completion, live canonical
and ownership checks, resource generation, viewport/DPR changes, upload failure
recovery and retirement saturation. These headless checks are not a native
continuous-presentation acceptance result.


Independent source review identified and corrected four additional cases before
native revalidation: all native/custom and CPU children now share a prepaint/paint
provenance guard; source-alpha hits in retained CPU images use the accepted image
frame after validating its live resource generation; fully clipped/offscreen CPU
results retain an accepted-empty marker so unchanged domains do not stall later
native-only state frames; and upload preflight uses an exact zero-opacity Div
scope instead of an empty clip, whose device-pixel rounding could expose a pixel
at fractional origins or DPR. The upload still has positive image bounds and
executes the real atlas insertion. Regression checks were added for each case.

Revised follow-up checks: all 245 current UI library tests passed, including 13
coherence regressions and two navigation-integration tests. Strict UI library/test
Clippy passed, as did four focus/input mirror fixture tests, strict Clippy for
both affected demos, and the rebuilt `state_geometry` binary. After the final
navigation input-clock hook was connected, its two focused tests and four mirror
fixtures were rerun. A second independent source review found no remaining
blocker in the four corrected paths. The fractional-view and zero-opacity tests
validate the shared guard and style state; they do not substitute for native GPU
pixel validation. Native acceptance was pending at that source checkpoint. The subsequent frozen
Linux revalidation is recorded in [state-coherent-native.md](state-coherent-native.md)
and its compact summary (historical record in Git history):
33 state-only samples retain coherent card/text, and seven clicks match accepted
geometry. Hard invalidations still intentionally load. The earlier failed window
evidence remains unchanged. APP-05 is bounded verified for the original fixed
flags/named/inheritance/per-state transition/geometry set; excluded arbitrary
properties are not additional closure criteria. Native and automated coverage
are separated in the report, including canonical event-local coordinates.
