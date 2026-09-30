# Named states, text placeholders and button inheritance

## Status and reuse boundary

This is a source implementation of the retained-scene state slice. It adds real
presentation behavior through `SceneStateStyles::presentation_at`, already
consumed by `SceneView` rendering. It is **not** a claim of full Leafer state
parity. Its 14 named-state regressions and the full local headless suite now pass,
as do workspace/all-targets check and strict clippy. See
[local validation](local-validation-2026-09-30.md) for exact coverage; real-window
interaction and CI remain unverified.

The upstream reference is the actual Leafer state plugin at
[`3eb445bf34debb638766615282224b77108bf610`](https://github.com/leaferjs/leafer-in/tree/3eb445bf34debb638766615282224b77108bf610/packages/state/src):

- [`style.ts`](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/state/src/style.ts) resolves named, selected, placeholder, focus, hover, press, disabled, in that order
- [`helper.ts`](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/state/src/helper.ts) checks the parent and grandparent for the nearest button, and does not inherit into a button itself
- [`check.ts`](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/state/src/check.ts) combines local and button flags; child paint definitions remain local
- [`Text.ts`](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display/src/Text.ts) displays a nonempty placeholder only when text is exactly empty

The existing locked [Kit StateStyle](https://github.com/longbridge/gpui-kit/blob/0067bafcf3160cb871eff2521e98a51e526da175/crates/base/src/state_style.rs)
was inspected before adding this adapter. It wraps GPUI `StyleRefinement`; its
`into_refinement` and `resolve_style` are crate-private. Its native-control layer
order is supplied by each control and has no scene `ElementHandle`, attachment
epoch, canonical Scene, or editor playback/history contract. Importing it would
bring GPUI into this headless crate without replacing the required retained-scene
policy. The thin state adapter therefore stays here. Existing native controls
should continue using Kit. Existing paint interpolation is reused through
`flow-animation::TweenValue::interpolate`; no easing curve or interpolation
implementation is duplicated.

## API

Existing `StatePaint`, `StateStyles`, `VisualState`, `StateTransition` and
`StateTransitions` field layouts remain unchanged. Existing `bind` calls remain
valid. Additional methods require an existing binding:

- `set_named_states(engine, handle, BTreeMap<String, StatePaint>)`: atomically replace the local palette, including validation of inactive entries
- `select_state(engine, handle, Some(name))`: select a locally defined name
- `select_state(engine, handle, None)`: clear the local override and allow button-name inheritance again
- `set_placeholder(engine, handle, Some(TextPlaceholder::new(content, paint)))`: configure actual fixed-box Text placeholder rendering
- `set_placeholder(engine, handle, None)`: remove the placeholder
- `set_button(engine, handle, bool)`: make the binding a state source for descendant bindings
- `set_inheritance(engine, handle, StateInheritance::ParentButton | StateInheritance::None)`: control button inheritance; default is `ParentButton`
- `resolved_state(editor, router, handle)`: inspect the effective builtin flags, named-state name, placeholder activity and source-button handle

`StatePaint` remains an optional solid base-fill and/or local opacity patch. It
cannot clear fill, replace geometry, redefine nested styles, or add animation
metadata. Clearing a state is distinct from setting an empty paint.

Example, using a live Text handle `label` and a live branch handle `button`:

```rust
use gpui_flow_interaction::scene::{SceneStateStyles, StatePaint, StateStyles, TextPlaceholder};
use gpui_flow_scene::Color;
use std::collections::BTreeMap;

let mut states = SceneStateStyles::new(&editor.engine);
states.bind(&editor.engine, button, StateStyles::default())?;
states.bind(&editor.engine, label, StateStyles::default())?;
states.set_button(&editor.engine, button, true)?;
states.set_named_states(&editor.engine, button, BTreeMap::from([
    ("busy".into(), StatePaint::default()),
]))?;
states.set_named_states(&editor.engine, label, BTreeMap::from([
    ("busy".into(), StatePaint::fill(Color::hex(0x888888))),
]))?;
states.select_state(&editor.engine, button, Some("busy"))?;
states.set_placeholder(&editor.engine, label, Some(TextPlaceholder::new(
    "Enter a name", StatePaint::fill(Color::hex(0x999999)),
)))?;
// The existing presentation_at + SceneView render path consumes these settings.
```

There are at most 1,024 bindings, 128 named states per binding, 128 UTF-8 bytes
per nonblank state name, and 65,536 UTF-8 bytes per placeholder. There is no new
package dependency or serialized document field.

### Missing names, replacement and clearing

Explicitly selecting an undefined local name returns `MissingState(name)` and
leaves the previous selection and motion intact. This intentionally makes the
host API stricter than Leafer's permissive arbitrary `state` assignment. An
inherited name without a matching child palette entry simply contributes no
named paint. Parent paint is never copied to the child.

Palette replacement preserves the selected name. Removing that name makes its
layer absent; redefining it makes it effective again. `None` clears the local
selection. To suppress inherited state entirely, use `StateInheritance::None`.
An empty locally defined paint may also intentionally override an inherited
name while contributing no named fields.

Rebinding builtin styles preserves the disabled flag, palette, selected name,
placeholder, button policy, timing and in-flight sample. `unbind` removes all
configuration for that handle. `clear` removes all bindings. None creates an
editor command, Scene revision, listener, timer or Undo item.

## Resolution and lifecycle

For each binding the canonical style is resolved in this order:

1. Local paint for the selected effective named state
2. Selected paint
3. Active Text placeholder paint
4. Focus paint
5. Hover paint
6. Press paint
7. Disabled paint

Disabled suppresses steps 4–6, even when its own paint is empty. Higher layers
override only their specified fields. The named layer can continue supplying
opacity while a higher state supplies fill. Disabled is visual only; listener,
command and editing permissions stay under existing host policy.

For a non-button binding using `ParentButton`, the nearest registered button
among its parent and grandparent is consulted. Local nonempty named selection
wins; otherwise the direct button's selected name is used to look up the
**child's** palette. Local selected/disabled/router flags are OR-ed with the
button's direct flags. Button state is not recursively resolved through another
button. Placeholder activity is local and is never inherited. A button binding
itself does not inherit an outer button. Setting `None` disables this extra
inheritance, not the router's pre-existing hover/press ancestor-path behavior.

The current selection is read, never rewritten. Input still comes from
`SceneEventRouter::state`, including its live handle/epoch checks, target-path
hover/press behavior and exact logical focus. No event dispatch, capture,
Space latch, queued focus action, gesture selection or authorization is changed.

Detached bindings remain registered but resolve inactive. Dead generations and
document resets are pruned. Reattachment retains intentional runtime palette,
selected name, placeholder and disabled settings, but does not restore old
pointer/focus input or an old transition sample. Frames additionally key their
source button by handle and attachment epoch. Changing that source or remounting
it starts a fresh presentation frame; old-source motion is not reused. Sources
are looked up from the current scene every frame, never cached by public ID.

## Placeholder and resource contract

Only a canonical `Shape::Text` with exactly empty `content` and a nonempty
configured placeholder activates the layer. Whitespace is real content. The
presented Scene copy receives placeholder text and paint and reaches the normal
paint-list/GPUI text-shaping path. It retains the canonical Text size, family,
weight, line height, wrapping, transform and hit geometry. Placeholder text is
discrete; its fill and opacity participate in ordinary transitions.

The canonical `engine.object(handle)` text is always unchanged. Image/font
resource synchronization must use canonical objects and live instance identity;
it must not copy placeholder text out of the presentation Scene into document
properties or canonical measurement results. A successful resource result for
canonical text is not proof that the substituted placeholder's actual shaping
succeeded. A renderer that reports shaping outcomes must retain this distinction.

The placeholder does not drive intrinsic sizing or reflow. Applications needing
auto-sized placeholder content require the geometry/layout overlay contract
below; the fixed-box substitute is not a claim that this gap is solved.

## Transition and atomicity contract

Named-state entry/replacement and placeholder entry use the binding's existing
entry transition. Clearing the selected name or removing an active placeholder
uses the exit transition; missing exit timing falls back to entry timing. Palette edits with unchanged
state identity are entry retargets. Replacement of an active name by another
active name is an entry, even when the old one is also leaving. Existing disabled
priority is retained. Each fill/opacity channel retargets from its current sampled
value and unchanged target channels keep their original timing.

Canonical edits or playback changes cancel only stale motion for the changed
channel, using the existing transition implementation. Presentation still stages
all candidate frames and commits the accepted clock/motion only after every
Scene update succeeds. Failed validation does not partially advance another
binding or write canonical state. Detached/dead binding pruning remains existing
lifecycle cleanup. `finish_transitions` snaps paint motion; unbind/clear releases
the overlay.

## Remaining Leafer gaps and explicit geometry proposal

Implemented here: solid fill/opacity named state, state selection/clear, useful
fixed-box Text placeholder content+style, two-level parent-button state policy,
existing binding-level transitions, live input/epoch integration and no-history
presentation.

Not implemented here: arbitrary `IStateStyle` geometry/transforms/font-size,
placeholder-driven intrinsic measurement, nested state-style definitions,
per-state timing overrides, state-triggered animations, gradients/layered-paint
interpolation, fill-to-None interpolation or state-driven hit/layout geometry.
The canonical JSON schema still does not serialize runtime state registrations.

An engine-backed arbitrary-state implementation should be a separate,
explicitly reviewed interface, not fields silently ignored by this adapter:

1. Add a validated `PresentationPatch`/`PresentationSnapshot` facility to the
   engine, keyed by live handle, attachment epoch, canonical revision and overlay
   revision. Patches must declare affected properties and ownership and use the
   same property validation as canonical edits, without committing editor history.
2. Derive transforms, bounds, clips, opacity, layout/text measurement and spatial
   queries from that snapshot. Rendering and picking must consume a declared
   common snapshot or an explicitly chosen canonical-hit mode; do not mix a
   deformed rendering with silently stale caches.
3. Introduce a layout/text-provider overlay path for font/content/size changes.
   Reuse the existing numerical/layout and shaping backends. Validate the complete
   derived candidate before publishing it; failures keep the last valid snapshot.
4. Preserve animation/property-channel ownership and canonical cancellation rules,
   and never write overlay fields back to history baselines or playback snapshots.
5. Coordinate host hit-testing, native focus, resource identity, damage bounds and
   invalidation. Specify how an active pointer capture responds when its geometry
   changes. Prove generation/epoch checks and atomic rollback across these layers.

This requires engine/layout/UI interfaces outside this slice's ownership. The
proposal is documentation, not an implemented engine API.

## Regression sources

`crates/flow-interaction/tests/named_states.rs` covers priority and field merging;
missing names, clear and palette replacement; real placeholder paint-list content,
fixed bounds and canonical hit/history invariants; local-name precedence and
missing inherited definitions; direct parent/grandparent limits and nested button
boundaries; selected/disabled/router inheritance; validation and capacity errors;
detach/remount/destruction/reset; named and placeholder transition continuity;
failed-frame rollback; playback/Undo/Redo; and Space/capture/selection preservation.
Existing `state_styles.rs`, `state_transitions.rs`, focus and keyboard tests are
preserved. These are added regression **sources**, not executed test results.
