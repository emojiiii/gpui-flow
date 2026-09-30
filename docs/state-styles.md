# Paint-only state styles

`SceneStateStyles` is an optional, engine-scoped presentation layer in
`gpui-flow-interaction::scene`. `SceneView` consumes it directly. Register
`StateStyles` against a live `ElementHandle`, then routed input and editor
selection resolve the current appearance. This is a bounded Leafer contract
milestone, not complete state/transition compatibility.

## Priority and input

The supported layers merge property by property in this order:

`document style < selected < focus < hover < press`

When disabled, the order is `document style < selected < disabled`; focus, hover,
and press patches are omitted even if the disabled patch is empty. An omitted
field inherits the lower layer. Focus is an exact target; hover and press include
the target's ancestors. There is no implicit parent-button inheritance.

- `StatePaint.fill` replaces a vector/text/custom element base fill with a solid color; Image and Group reject fill patches and support opacity only
- `StatePaint.opacity` replaces local opacity; normal ancestor multiplication
  still applies to children in the real paint pipeline
- Additional fill layers, stroke layers, geometry, transforms, clipping, visibility,
  order and document data are unchanged
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
Replacement preserves the existing disabled flag. Invalid opacity (NaN/infinity
or outside `[0,1]`), unsupported style combinations and foreign handles fail
without replacing the prior binding. There are at most 1024 bindings per sheet.
`unbind` and `clear` remove them explicitly. No listeners, observers, timers or
background tasks are installed by a sheet.

Dead/replaced generations are pruned on bind, presentation or explicit `prune`.
Dropping a sheet releases all registrations. Resetting a document cannot transfer
a binding to a reused public ID. Detached instances retain their style binding
but have no active pointer state. Reattachment keeps the live handle while
advancing `InstanceInfo.attachment_epoch`; the router uses this epoch so a
complete detach/reattach between input events cannot revive old press/hover/focus.

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

- [state/style.ts](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/state/src/style.ts): field merging, priority, disabled suppression, normal-style restoration and transition hooks
- [state/event.ts](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/state/src/event.ts): pointer enter/leave/down/up integration
- [state/check.ts](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/state/src/check.ts) and [helper.ts](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/state/src/helper.ts): pointer/fixed-state lookup and parent-button behavior
- [Interaction.ts](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/interaction/interaction/src/Interaction.ts): hover/press path membership versus exact focus

Rust uses a non-mutating presentation layer instead of temporarily assigning
properties and restoring normalStyle. No upstream implementation was copied.
Missing: named `states`, placeholder styles, button inheritance, programmatic
focus transfer, geometry/custom-property state changes, declarative serialization,
automatic transitions, timeline composition and motion paths.
