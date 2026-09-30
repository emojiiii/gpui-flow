# gpui-flow-engine

Pure Rust live-instance layer over the serializable `gpui-flow-scene` document. No GPUI, editor, workflow executor, decoder, or network dependency.

## Ownership and contracts

- `SceneEngine` owns the canonical attached `Scene`; `scene()` is immutable. It is deliberately not `Clone` or `Serialize`
- `transaction` edits a candidate, normalizes registered custom geometry, validates, computes a net `ChangeSet` and fresh derived snapshot, then commits. Invalid/no-op edits do not emit committed changes
- `replace_scene` is a history restore: surviving document IDs keep their instances unless the element kind changes. Removed and later restored IDs receive fresh instances
- `reset_scene` is a new-document load: every old attached/detached instance is retired even when IDs are reused. Definitions remain registered
- `ElementHandle` contains engine identity, slot and generation. Foreign/stale handles fail; `instance()` can report a tombstone as `Destroyed` until its slot is reused. Require `Attached` before applying queued document commands
- `detach` retains a complete forest with its parent/child order and runtime identity. The engine owns its document; use `detached_scene` to read it. `attach` consumes its validity, not its Rust token value, so repeat/foreign attachment fails
- `destroy` retires a complete attached subtree. `destroy_detached` retires a detached forest. Scoped listeners, observers and disposers are released exactly once
- Dropping a detached token does **not** destroy its instances. Explicitly destroy unwanted detached trees; engine drop releases remaining disposers, including detached ownership
- Clone preserves input and hierarchy with a returned ID map. Arbitrary application references inside JSON data are not guessed or rewritten. Listeners, resource jobs and runtime identities are never copied

A transaction closure represents an atomic **net state**, not live intermediate edits. Removing and reinserting an identical ID inside one closure is not a destroy/recreate operation; use explicit lifecycle operations when that distinction matters.

## Registered elements

`ElementRegistry::default()` includes twelve validated built-in factories, each publishing a geometry schema/defaults. Image creation requires a source reference. Unknown factory names error. Unknown serialized `Custom` kinds keep their inert validated fallback geometry, so loading does not require executable application code.

A custom definition provides a versioned property schema and actual geometry generation. Committed property changes regenerate bounds/contours, so rendering and picking use the same result. An optional custom hit function can specialize geometry picking inside declared bounds. Built-in `PropertyKey` descriptors expose common mutation types and invalidation categories. Defaults are resolved for evaluation without injecting custom defaults into serialized input. `$version`, when provided, must match the definition; migration is an explicit application step.

```rust
use gpui_flow_engine::{
    CustomDefinition, DirtyFlags, ElementRegistry, PropertyDescriptor,
    PropertyKey, PropertySchema, SceneEngine,
};
use gpui_flow_scene::{display, HitTestOptions, Point, Scene};
use serde_json::{Value, json};

let mut registry = ElementRegistry::default();
registry.register(CustomDefinition::new(
    "badge",
    PropertySchema {
        properties: vec![PropertyDescriptor::number(
            "radius", 10.0, 1.0, 100.0,
            DirtyFlags::GEOMETRY | DirtyFlags::HIT | DirtyFlags::PAINT,
        )],
        allow_unknown: false,
    },
    |properties: &Value| {
        let radius = properties["radius"].as_f64().unwrap() as f32;
        let circle = display::Ellipse::circle(radius);
        Ok((circle.bounds(), vec![circle.flatten(0.1)]))
    },
))?;
let mut engine = SceneEngine::with_registry(Scene::new(), registry)?;
let (badge, created) = engine.create("badge", "badge-1", json!({}), None, None)?;
assert!(created.changed());
engine.set_property(badge, PropertyKey::Custom("radius".into()), json!(30.0))?;
assert!(engine.hit_test(Point::new(30.0, 10.0), HitTestOptions::default())?.is_some());
let (detached, _) = engine.detach(badge)?;
engine.attach(&detached, None, None)?;
assert_eq!(engine.handle(&"badge-1".into()), Some(badge));
engine.destroy(badge)?;
# Ok::<(), gpui_flow_engine::EngineError>(())
```

Factories and observers are trusted native code, not an adversarial-code sandbox. Geometry factories must be pure and deterministic. A failed factory/validation leaves the engine unchanged; arbitrary external effects inside application callbacks cannot be rolled back.

## Changes, layout and rendering preparation

`ChangeSet` contains revision/origin, old/new properties with explicit presence flags, structural changes, lifecycle events, dirty categories, layout counters, and conservative world-space damage. `set_properties` batches multiple changes into one revision; repeated writes coalesce to first old/last new value. Global or instance-scoped `on_change` subscriptions observe committed records.

`LayoutSnapshot` is synchronously current after every commit. It caches world/inverse transforms, local subtree and world bounds, intrinsic content bounds, inherited opacity/visibility/locks/hit policy, and clips. Transform changes propagate downward; geometry changes update ancestor aggregation. Paint-only changes avoid matrix and bounds recomputation. `paint_list`, picking and coordinate queries use these caches rather than revalidating/rederiving the scene each frame.

Current implementation still validates/clones candidate documents and walks the hierarchy when reconciling a transaction. Unaffected cached matrices/bounds are reused, as shown by `LayoutStats`; this is not a claim of O(changed-nodes) total transaction cost. Drawing still traverses paint order, and path flattening belongs to the backend.

Damage includes old/new bounds and clips. This is **incremental CPU preparation**, not partial GPU painting. Existing flat opacity semantics remain unchanged; isolated compositing layers, effects, advanced masks, auto-layout providers and animation are subsequent contracts, not implemented by this crate.

## Subscription/resource scope

`on(handle, kind, options, callback)` and `off(token)` proxy the owned tree dispatcher. Tokens are engine-scoped. Once subscriptions remove both callback and ownership metadata after dispatch. Detach suspends tree reachability while retaining subscriptions; reattach preserves them. Destroy/reset removes them, preventing delivery to a new element using the same document ID.

`add_disposer(handle, callback)` registers deterministic element-owned cleanup; `dispose(token)` releases it early. Disposers and change observers that panic are caught so committed state and remaining cleanup survive; `callback_panics()` exposes the count. Explicit lifecycle notifications are delivered before scoped destruction cleanup, child-first on removal and parent-first on creation. Engine drop performs resource cleanup; it does not run a new public lifecycle-event transaction.

Native input routing/default-action policy is a separate layer. Direct engine mutation is trusted application access; read-only UI policy must gate commands before calling it.

## Files and verification

- `engine.rs`: canonical ownership, atomic commit and live lifecycle
- `instance.rs`: opaque handles, state and scoped token types
- `registry.rs`: schemas, definitions, factories and property metadata
- `change.rs`: net deltas and invalidation classification
- `layout.rs`: cached derived state and damage
- `query.rs`: cache-backed rendering, coordinates and picking
- `subtree.rs`: hierarchy-preserving extraction/insertion and traversal
- `tests/engine.rs`, `tests/layout.rs`, `tests/registry.rs`: identity/lifecycle, incremental/full equivalence, custom schemas and behavior

Run `cargo test -p gpui-flow-engine` and `cargo clippy -p gpui-flow-engine --all-targets -- -D warnings`.

An optional `LayoutProvider` derives relevant geometry/tree edits before their
single atomic commit. Installation requires a stable initial scene and changes
runtime configuration only. Providers may write affine transforms and supported
shape sizes; recursion, scope violations, panic and nonconvergence reject the
candidate. Undo/Redo restores snapshots without derivation. See
[reactive layout](../../docs/reactive-layout.md) for the concrete flow adapter,
live-binding lifecycle and animation ownership.
