# Paint-only state styles

Source-only update (2026-09-30): named states, fixed-box Text placeholders and
parent-button inheritance are implemented in source with authored regressions,
but are **UNCOMPILED / UNRUN**. Historical state/transition checks do not validate
this batch. See [named-states.md](named-states.md) for the complete new contract.

`SceneStateStyles` is an optional, engine-scoped presentation layer in
`gpui-flow-interaction::scene`. `SceneView` consumes it directly. Register
`StateStyles` against a live `ElementHandle`, then routed input and editor
selection resolve the current appearance. This is a bounded Leafer contract
milestone, not complete state/transition compatibility. Optional solid-fill/opacity transitions
use the existing animation sampler and the same non-mutating presentation layer.

## Priority and input

The supported layers merge property by property in this order:

`document style < named state < selected < placeholder < focus < hover < press`

When disabled, the order is
`document style < named state < selected < placeholder < disabled`; focus, hover,
and press patches are omitted even if the disabled patch is empty. An omitted
field inherits the lower layer. Focus is an exact target; hover and press include
the target's ancestors. New source optionally adds a nearest registered button
among the parent/grandparent: a non-button binding defaults to
`StateInheritance::ParentButton`, while `None` disables this extra inheritance.
Flags are OR-ed and the inherited name selects the child's own palette; parent
paints and placeholder content are never copied. A button does not inherit
an outer button's state.

- `StatePaint.fill` replaces a vector/text/custom element base fill with a solid color; Image and Group reject fill patches and support opacity only
- `StatePaint.opacity` replaces local opacity; normal ancestor multiplication
  still applies to children in the real paint pipeline
- Additional fill layers, stroke layers, geometry, transforms, clipping, visibility,
  order and document data are unchanged. An active Text placeholder substitutes
  content only in the fixed-box presentation copy, never in canonical text or
  intrinsic measurement
- The host supplies `disabled` through `set_disabled`; it is a visual state, not
  editing permission or automatic command/listener suppression. Use access
  policies and listener cancellation for those separate contracts

PointerDown must be accepted before press/focus are armed. Prevented PointerUp,
PointerCancel, Escape and FocusLost cannot strand press. PointerCancel also
clears hover; FocusLost clears focus. PointerLeave emits out/leave for the old
hover path and clears hover without dropping an ongoing press/capture. The native
view wires leaving the canvas and missing-button cancellation, including presses
that did not acquire a plugin capture. State reads ignore dead or detached
handles immediately.

## Presentation and history

A state change does not write to `SceneEngine`, produce a `ChangeSet`, add an Undo
step, interrupt animation playback, or save styles into scene JSON. There is no
old-style restoration snapshot. Every frame resolves against the current
canonical style: edits or animation made while hovered become visible when the
state ends, instead of being overwritten by a stale baseline.

`presentation(&editor, &router)` borrows the scene when no effective patch exists.
An active patch creates a private validated scene for drawing. Its existing paint
list handles inherited opacity, transforms and clips, and `SceneView` uses the
same `RenderObject`/custom-renderer pipeline. Without an active patch, rendering
keeps using the engine's cached paint list. Invalid derived presentation is
reported by `SceneView.state_style_error` while the canonical scene is drawn.

Picking, editor geometry, bounds caches, fit, and saved documents deliberately use
the canonical scene. In particular, an opacity-zero overlay does not make its
interaction target disappear and flicker between hover and non-hover. This
paint-only policy is explicit; geometry-changing states or state-dependent hit
areas are not implemented. An active overlay currently copies/validates a scene;
this is not an incremental or large-scene performance claim.

## Registration and lifetime

`bind` validates all patches, including inactive ones, before replacing a binding.
Replacement preserves the disabled flag, named palette/selection, placeholder,
button/inheritance settings, timing and in-flight motion. Invalid opacity (NaN/infinity
or outside `[0,1]`), unsupported style combinations and foreign handles fail
without replacing the prior binding. There are at most 1024 bindings per sheet.
`unbind` and `clear` remove them explicitly. No listeners, observers, timers or
background tasks are installed by a sheet. SceneView requests a next frame only
while a successful presentation reports active motion.

Dead/replaced generations are pruned on bind, presentation or explicit `prune`.
Dropping a sheet releases all registrations. Resetting a document cannot transfer
a binding to a reused public ID. Detached instances retain their style binding
but have no active pointer state. Reattachment keeps the live handle while
advancing `InstanceInfo.attachment_epoch`; the router uses this epoch so a
complete detach/reattach between input events cannot revive old press/hover/focus.

## Paint transitions

`set_transitions(engine, handle, StateTransitions { transition, transition_out })`
sets per-binding timing. `StateTransition` contains `duration_ms` and the existing
`flow-animation::Easing`. Defaults are zero-duration, preserving immediate styles.
An omitted `transition_out` falls back to `transition`; an explicit zero-duration
exit snaps. Adding a state uses entry timing, removing states uses exit timing;
entering/leaving disabled explicitly uses entry/exit. Simultaneous additions and
removals prefer entry. A new patch on the same active state also uses entry timing.
This is a declared binding-level subset of Leafer's per-state/element settings.

- Solid-to-solid RGBA and local opacity interpolate, with exact endpoints and the
  shared animation easing/color rules. Gradient or absent-fill endpoints snap;
  unlike upstream's nullable color interpolation, no synthetic transparent paint
  is introduced. Other style fields always pass through from the current document
- Retargeting or reversing starts from the old transition sampled at the current
  host time. Repeated render frames or changes to an unrelated state/field do not
  restart an unchanged channel target
- A canonical fill or opacity edit/animation cancels existing motion on that
  channel. Its currently active state override still applies; an unowned channel
  immediately follows the new document value. Other channels keep progressing.
  This avoids chasing an animated baseline and never restores stale document data
- Changing timing explicitly finishes that binding's current motion, even if its
  target is unchanged. `finish_transitions` finishes all current endpoints;
  `unbind`/`clear` immediately release presentation back to the document
- Headless hosts call `presentation_at(editor, router, monotonic_duration)`.
  `presentation` samples the last accepted time. A backwards timestamp fails.
  All frame changes and the clock commit only after the entire presentation is
  valid; dead-instance pruning may still occur on a failed frame
- Detach retires motion, and a changed attachment epoch retires it even if detach
  and reattach occurred between two frames. A new binding can animate from the
  current base; old IDs cannot inherit prior motion after destruction/reset
- `SceneView` supplies a monotonic clock and requests further frames only during
  active transitions. Render failure retains diagnostics and canonical fallback,
  and does not start a self-sustaining invalid-frame loop

The sheet is runtime configuration: bindings, clocks and motion are not serialized
or part of edit history. State transitions do not acquire an editor PlaybackToken,
interrupt playback, add Undo steps, or change reactive layout geometry. Hosts
should call `cx.notify()` after changing runtime styles/settings, as for other UI
configuration. The native state_styles example uses 180ms entry and 320ms exit;
hover, press, leave or disable midway to exercise continuous retargeting.

## Usage

```rust
use gpui_flow_interaction::scene::{StatePaint, StateStyles};
use gpui_flow_scene::Color;

let handle = view.editor.engine.handle(&"card".into()).unwrap();
view.state_styles.bind(&view.editor.engine, handle, StateStyles {
    hover: StatePaint::fill(Color::hex(0x239e91)),
    press: StatePaint::fill(Color::hex(0xd38b36)),
    disabled: StatePaint { opacity: Some(0.5), ..Default::default() },
    ..Default::default()
})?;
// Host UI changes should call cx.notify() as usual.
```

Run the native `state_styles` example or the executable headless documentation:

```bash
cargo run --locked -p flow-studio --bin state_styles
cargo run --locked --manifest-path docs/examples/Cargo.toml --bin state_styles
```

The native example provides actual hover/press/focus/selection colors, disabled
visuals, bind/unbind and edits to the canonical fill with Undo/Redo. Compilation
and linking are distinct from manual GUI verification.

## Official-source grounding and remaining gaps

Inspected pinned `leafer-in` revision
`3eb445bf34debb638766615282224b77108bf610`:

- [state/style.ts](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/state/src/style.ts): field merging, priority, disabled suppression, normal-style restoration, getFromStyle retargeting and transitionOut fallback
- [animate/Transition.ts](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/animate/src/Transition.ts): numeric/color interpolation and discrete complex-paint fallback
- [state/event.ts](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/state/src/event.ts): pointer enter/leave/down/up integration
- [state/check.ts](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/state/src/check.ts) and [helper.ts](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/state/src/helper.ts): pointer/fixed-state lookup and parent-button behavior
- [Interaction.ts](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/interaction/interaction/src/Interaction.ts): hover/press path membership versus exact focus

Rust uses a non-mutating presentation layer instead of temporarily assigning
properties and restoring normalStyle. No upstream implementation was copied.
Named palettes, fixed-box Text placeholders and bounded parent-button inheritance
are now source implementations; see [named-states.md](named-states.md).
Missing: geometry/custom-property state changes, declarative serialization,
per-state transition overrides, transition delay/repeat, gradient/absent-fill
interpolation, timeline composition and motion paths.

Programmatic and deferred focus now use the same focus style/transition resolution;
see [focus-routing.md](focus-routing.md). Focus remains distinct from selection.
