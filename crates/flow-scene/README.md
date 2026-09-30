# gpui-flow-scene

Source-only update (2026-09-30): Kurbo tight bounds and advanced centered stroke
styles/mesh integration are **UNCOMPILED / UNRUN**. New regressions are authored,
not passed. Earlier baseline validation does not cover this source batch.

A retained vector scene graph that runs without GPUI or a window server. This is
an independently implemented Rust scene layer with a common display-object
model, not a graph-node alias or a collection of canvas demo callbacks.

The separation between shared display properties and individual primitives is
informed by [Leafer's display-object concepts](https://www.leaferjs.com/ui/en/reference/display/UI.html).
This crate does not embed Leafer or claim API compatibility with its full engine.

## Layers

- `display/*.rs`: each primitive owns its local geometry and data
- `DisplayObject`: identity, name/classes, arbitrary JSON data, local affine
  transform, style, visibility, locking, z-order, and hit policies
- `SceneNode`: parent and ordered child references
- `Scene`: atomic hierarchy edits, transforms, bounds, painter order, clipping,
  picking, validation, and versioned persistence
- `EventDispatcher`: independent runtime listener registry with capture, target,
  and bubble propagation; callbacks are never serialized

Scene data is `Clone + PartialEq`, so an editor can maintain snapshot history
without depending on a renderer. Graph connectivity and execution remain the
responsibility of the separate flow-core/runtime layers.

## Display primitives

- `Rect`: rectangle with optional rounded corners
- `Ellipse`: ellipse/circle geometry with analytical fill picking
- `Line`: two endpoints; default round strokes or opt-in cap/join/dash mesh picking
- `Polygon`: closed polygon or open polyline
- `Star`: configurable point count, radii, center, and angle
- `Path`: absolute move/line/quadratic/cubic/close commands; analytical Bézier
  bounds and bounded adaptive curve subdivision
- `Pen`: fluent, serializable authoring builder over Path
- `Text`: text content, explicit layout size, font family/size, line height,
  alignment, and wrapping preference
- `Image`: resource reference, explicit size, optional intrinsic size, and
  contain/cover/fill geometry
- `Group`: geometry-free container whose bounds derive from its descendants
- `Box`: painted rectangular container, optional clipping
- `Frame`: painted artboard container with clipping enabled by default
- `Custom`: stable application type key, JSON properties, declared bounds, and
  validated vector fallback contours

Use `display::Rect` for the primitive and `Bounds` for an axis-aligned geometric
rectangle. `Point` and `Size` are shared with flow-core.

## Create a scene

```rust
use gpui_flow_scene::{display, Affine, Color, DisplayObject, Scene, Style};

let mut scene = Scene::new();
scene.insert(
    DisplayObject::new("artboard", display::Frame::new(800.0, 600.0))
        .with_style(Style::fill(Color::WHITE)),
    None,
    None,
)?;
scene.insert(
    DisplayObject::new("badge", display::Star::new(5, 40.0, 18.0))
        .with_transform(Affine::translation(100.0, 80.0))
        .with_style(Style::fill(Color::hex(0x8765ef))),
    Some("artboard".into()),
    None,
)?;
```

`insert` takes an optional parent and optional sibling index. `None` appends.
`update_object` gives a callback access to common properties and shape data,
validates a private candidate, then commits it. Errors or callback panics cannot
partially mutate the original scene. IDs are immutable through this API.

`reparent(id, parent, index, preserve_world)` can preserve visual placement when
moving across transformed containers. It rejects cycles, invalid indices, and
noninvertible destination transforms. `remove` removes an entire subtree and
returns its objects in parent-first order. Low-level scene edits are
authoritative; the `locked` property controls picking and is available to an
editor when applying its own mutation policy.

## Coordinate systems and rendering

Affine matrices use column vectors:

`world = parent * local * point`

`Affine::rotation` and `skew` take degrees. Negative scales are valid reflections.
Singular transforms are legal scene state (for collapsed objects), but cannot be
picked or inverted. Accumulated matrices, transformed bounds, and aggregate
bounds are checked for overflow. A maximum depth of 256 protects hierarchy
traversal from malformed serialized trees.

`Shape::flatten(tolerance)` returns local contours. A rendering adapter can
transform their points with `PaintItem::world_transform`. Curves retain their
original control points in storage, and callers can request different flattening
precision. Ellipse/rounded-rectangle subdivision is bounded; paths additionally
expose `flatten_with(FlattenOptions)` for tolerance/depth control.

`Scene::paint_list()` emits back-to-front objects, effective opacity, inherited
lock state, and the exact transformed rounded-rectangle clip ancestry. Sibling
`z_index` values are sorted stably; descendants cannot escape their parent's
stacking position. Invisible or zero-opacity subtrees are excluded. Group
entries are present but have no geometry of their own.

`Style` supports solid color, linear gradients, radial gradients, optional
stroke, additional ordered fill/stroke layers, opacity, and nonzero/even-odd fill
rules. Strokes are centered in local space, with solid-round defaults. New source
supports Butt/Square/Round caps, Bevel/Miter/Round joins, miter limits and signed
dash offsets/patterns. Inside/outside and scale-fixed strokes remain unsupported. The rendering adapter must explicitly declare unsupported
paint/clip features; it must not silently replace them with an unrelated fill.

`local_bounds` and `world_bounds` describe a subtree, including strokes and
hidden descendants, while respecting clips within that subtree. They are
conservative axis-aligned bounds after nested transformations. `world_bounds`
does not intersect clips outside the requested subtree. `visible_bounds`
intersects ancestor clip bounds for the visible scene; rounded/rotated clip
bounding rectangles are conservative. Picking checks the actual clip geometry.

## Picking

`hit_test(world_point, options)` returns the frontmost hit; `hit_test_all` returns
front-to-back hits with IDs, local/world points, and root-to-target paths.

- Hidden, zero-opacity, and locked subtrees are excluded by default
- `include_invisible` and `include_locked` override the respective filters
- `hit_children = false` suppresses descendant picks without hiding their paint
- `HitPolicy::None` suppresses self-picking
- `Painted` uses active fills/strokes and shape geometry
- `Geometry` ignores paint presence and tests underlying geometry
- `Bounds` uses subtree bounds, so a geometry-free Group can be a hit target
- Nested clips use inverse world transforms and rounded-corner tests

Rectangles and ellipses have analytical fill tests. Polygon/path fills use
winding tests, with adaptive path flattening at 0.1 local-unit precision. Default
solid-round stroke picking retains distance-to-segment tests. Advanced strokes
reuse Kurbo 0.11.3 dashing and Lyon 1.0.22 tessellation; the bounded local
`StrokeMesh` triangle union is shared with the native renderer. Its fixed
0.1-local-unit approximation and actual dash gaps/cap/join shapes also define
advanced picking. Nonzero world-space hit tolerance is converted conservatively
under anisotropic transforms and dilates picking only; it is not extra paint.

## Events

```rust
use gpui_flow_scene::{EventDispatcher, EventKind, ListenerOptions, SceneEvent};

let mut events = EventDispatcher::new();
events.on("artboard", EventKind::PointerDown,
    ListenerOptions { capture: true, once: false },
    |event| {
        // Inspect event.target, current_target, phase, and local_position.
        // event.prevent_default() can suppress an editor's default action.
    });
let result = events.dispatch(
    &scene,
    SceneEvent::new(EventKind::PointerDown, "badge"),
)?;
```

Capture traverses root to parent; target capture listeners run before target
noncapture listeners; bubbling travels parent to root. Listener order within a
phase is registration order. `stop_propagation` retains other listeners on the
same target, while `stop_immediate_propagation` also suppresses those listeners.
`prevent_default` respects `cancelable`. Pointer-enter/leave do not bubble by
default. `once`, `off`, subtree-listener cleanup, and custom event names are
supported. Dispatch uses a fixed ancestor path and routing identity.

## Extension and persistence

`Custom.kind` is a stable renderer/factory registry key. `Custom.data` stores
type-specific properties; ordinary `DisplayObject.data` can store unrelated
application metadata. Fallback contours are validated against the declared
bounds and remain renderable without the custom renderer. Registration and
custom application callbacks belong in the host layer, never in deserialization.

Use `Scene::to_json/from_json` for validated, versioned persistence. Unknown
versions, missing references, conflicting parent/child links, duplicate children,
cycles, invalid geometry/styles, excessive depth, and overflow are rejected.
Bare serde deserialization alone does not perform semantic validation. Missing
advanced Stroke JSON fields retain the previous solid-round defaults. External
Rust Stroke literals need the new fields or `..Stroke::default()`; constructors
remain available. New serializer output includes these fields, so backward
reading compatibility does not promise byte-identical JSON.

## Deliberate boundaries

This crate does not perform font shaping, text auto-layout, image fetching or
decoding, per-pixel alpha picking, animation, boolean path operations, filters,
GPU tessellation, or browser DOM emulation. Text picking uses its explicit layout
box; image picking uses its fit rectangle. The native renderer may support a
subset of scene paint, transformed text/image, or clipping features and should
report those capabilities explicitly.

Traversal and validation are correctness-first and do not use a spatial index
or retained GPU cache. Applications accepting untrusted large documents should
also apply document-size limits appropriate to their deployment.

## Verification

The commands below are validation entry points, not checks run for the current
source-only batch. See `docs/appearance.md` for stroke budgets and
`docs/source-only-status.md` for the uncompiled/unrun integration scope.

```sh
cargo test -p gpui-flow-scene
cargo clippy -p gpui-flow-scene --all-targets -- -D warnings
cargo fmt -p gpui-flow-scene -- --check
```
