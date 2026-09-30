# Synchronous reactive layout

`FlowLayout::install_reactive(&mut engine)` binds an already arranged flow
configuration to live instances. Ordinary engine/editor property and tree edits
then derive layout inside the same atomic commit. `SceneView` already uses that
engine, so normal host property edits and routed editor commands consume the
result immediately. They do not need a second explicit reflow call.

This is an opt-in, synchronous precommit pipeline. It does not install a watcher,
background timer, frame queue or application-wide observer, and it does not claim
full Leafer flow or input/computed-property compatibility.

## Installation and ownership

```rust
// Configure/arrange initial geometry before starting the editor's history.
layout.apply_to_engine(&mut engine)?;
let binding = layout.install_reactive(&mut engine)?;
let mut editor = SceneEditor::from_engine(engine);

// This ordinary edit also updates the parent's content size and sibling slots.
editor.transaction(|scene| {
    scene.insert(card, Some("frame".into()), None)
})?;
// A single Undo restores the insertion and all dependent geometry.
```

Installation requires the current scene to already be a fixed point of the
provider. An unarranged scene returns an error instead of silently changing the
scene or history. For an existing editor, perform the initial layout with
`apply_to_editor` so the host explicitly chooses its ordinary Undo boundary.

There is one optional provider per engine. Installing a replacement first
validates it completely; failure preserves the existing provider. The returned
`LayoutProviderId` is engine-scoped, and stale/foreign tokens cannot remove a
replacement. `engine.remove_layout_provider(binding)` only stops future automatic
layout; it leaves current geometry, revisions, Undo/Redo and playback intact.

The configuration is runtime-owned and not serialized into Scene JSON or copied
into history snapshots. Hosts retain their `FlowLayout`/`FlowDocument` and decide
when to install it. Changing configuration normally means remove the old provider,
explicitly arrange using the new configuration, then install its stable bindings.
These configuration changes are not themselves document Undo entries.

## Commit order, trigger and failure

For ordinary geometry/structure changes, the engine performs:

1. Apply caller input on a private candidate
2. Normalize registered custom-element properties into geometry
3. Derive layout to a fixed point, validating every candidate
4. Compute the final net changes, instances, cached bounds and damage
5. Commit once and notify observers once

Shape input, affine transforms, parent/child membership and child order trigger
layout. Paint, opacity, names, classes, application data, locks and visibility
alone do not. Flow measurement continues to use intrinsic boxes; paint extents
and hidden/collapsed policy do not silently become new size inputs.

`ChangeOrigin` remains the original edit/animation/lifecycle origin. Derived sizes
and positions appear in the same net `ChangeSet`, not a second Layout-origin
commit or Undo entry. A no-op result creates no document revision.

Providers receive immutable engine/candidate references. Recursive preparation
is explicitly rejected, including recursion through property-validation APIs.
Each engine derivation has at most 8 full passes; the flow provider's own nested
measure/arrange pass retains its existing 32-pass limit. Nonconvergence, invalid
geometry, unsupported size ownership or provider panic discards the candidate
without committing any partial scene, instance lifecycle, notification or history.
Custom geometry-definition panics during shape building also return an error.

## Strict layout write boundary

A general `LayoutProvider` may change only:

- An object's affine transform
- The `size` field of Rect, Ellipse, Text, Image, Box or Frame

It cannot change IDs, topology/order, styles, metadata or any other shape field.
Text content/font family/font size/line height/alignment/wrap and Image source/fit/
natural_size are preserved. Corner radius and Frame clipping are preserved.
Path commands, Polygon points, Line/Star/Pen data and Custom bounds/contours/input
are not writable through a layout provider; they may be positioned by affine
transform. Custom geometry continues to belong to its schema definition.

The explicit variant allowlist is checked before commit. Same-tag changes to text
content, image resources, paths, polygons or clipping are rejected atomically.
This boundary is narrower than all possible geometry changes by design.

## Live lifecycle and history

The flow adapter captures live handles, not public IDs alone. New children of an
active configured container participate with default item settings. A bound item
reparented away from configured containers stops using its item override.

Detach temporarily omits the affected subtree; reattachment keeps its handle and
resumes its bindings. Destroy, kind replacement or new-document reset cannot
transfer old configuration to a new instance using the same public ID. Reset
clears the installed provider. Destroyed item overrides do not apply to a
replacement child, which participates using defaults under an active parent.

Undo/Redo, gesture cancellation and playback cancellation restore exact document
snapshots and bypass reactive derivation. They do not reinterpret an old snapshot
using today's runtime layout configuration. A later geometry edit derives using
the currently installed provider. Restoring a destroyed configured container
creates new live identity, just like listener/resource restoration: the host must
explicitly rebind its layout configuration when it wants that new instance active.

A successful ordinary editor edit during playback records the last animation
frame and the new edit+layout as separate Undo steps. Failed edits leave playback
and its Redo branch intact. The editor now ends playback only after a successful
engine commit, not before fallible validation.

## Animation boundary

Custom numeric animation that changes intrinsic geometry can update content-size
ancestors and sibling positions each tick, still in one owned playback frame.
Finish stores one Undo step for the animation and derived geometry; Cancel restores
the entire baseline exactly.

A layout-owned transform or shape cannot simultaneously be an animation target.
Animation endpoint preflight and frame commit reject a requested transform/shape
that derivation would overwrite. The clock and all frame values stay unchanged
on failure. Animate an unowned outer container or an independent property instead.
Regular edit transactions may set input geometry that layout subsequently derives;
this module does not add a general input/computed property store.

## Examples and remaining work

```bash
cargo run --locked -p flow-studio --bin reactive_layout
cargo run --locked --manifest-path docs/examples/Cargo.toml --bin reactive_layout
```

The native example installs once and then uses ordinary `SceneEditor::transaction`
for width changes and adding/removing cards. Its controls never call reflow after
an edit. The executable headless example asserts resulting wrap heights and exact
Undo boundaries. Compilation/linking is separate from manual GUI verification.

Missing: frame-level coalescing, partial dependency scheduling, serialized runtime
configuration/history, full input/computed separation, percentage/shrink
layout, font intrinsic measurement and complete Leafer API compatibility. Active
layout still uses private scene copies and repeated validation; there is no new
large-scene performance guarantee.

Source grounding uses the pinned official sources already recorded in the
[source map](leafer-source-map.md):

- [Watcher](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/partner/watcher/src/Watcher.ts): relevant property/tree change collection and layout requests
- [Layouter](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/partner/layouter/src/Layouter.ts): ordered layout, bounded repeats and before/after change boundaries
- [Flow integration](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/flow/src/index.ts): measurement, automatic dimensions and nested second layout

Rust implements these change/derivation/commit boundaries directly, without
copying upstream observer/prototype code or claiming the same asynchronous timing.

Reverse row/column traversal and reverse wrapped-line placement compose with this
provider using the same atomic commit, identity and history contracts.
