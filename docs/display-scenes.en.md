# Display scenes

Use `Scene` to model a retained tree of visual objects, `SceneEditor` for
selection and history, and `SceneView` to render and edit the scene in a
GPUI window.

| Type | Responsibility |
| --- | --- |
| `DisplayObject` | Identity, geometry, transform, style, and hit policy |
| `Scene` | Hierarchy, bounds, picking, painting order, and JSON |
| `SceneEditor` | Editable selection, viewport, and bounded undo history |
| `SceneView` | Native drawing, GPUI input, and selection overlays |

## Create an artboard

This example creates a frame with a rounded rectangle as its child, then
round-trips the scene through JSON. It runs without GPUI.

```rust
use gpui_flow_scene::{Affine, Color, DisplayObject, Scene, SceneError, Style, display};

fn create_scene() -> Result<Scene, SceneError> {
    let mut scene = Scene::new();
    scene.insert(
        DisplayObject::new("artboard", display::Frame::new(640.0, 400.0))
            .with_style(Style::fill(Color::WHITE)),
        None,
        None,
    )?;
    scene.insert(
        DisplayObject::new("card", display::Rect::new(180.0, 100.0).rounded(16.0))
            .with_transform(Affine::translation(80.0, 60.0))
            .with_style(Style::fill(Color::hex(0x63e2bf))),
        Some("artboard".into()),
        None,
    )?;
    Scene::from_json(&scene.to_json()?)
}
```

For a native view, pass the scene to `SceneView::new(scene, cx)` when
constructing a GPUI entity. See [embedding a canvas](library.md#embed-a-canvas-in-your-app).

## Choose a display object

| Object | Behavior |
| --- | --- |
| `Rect`, `Ellipse`, `Line`, `Polygon`, `Star` | Shape geometry in local coordinates |
| `Path`, `Pen` | Curves and authored vector paths |
| `Text`, `Image` | Text layout or a reference to an image resource |
| `Group` | Container whose bounds come from its children |
| `Box` | Container with its own rectangle and optional clipping |
| `Frame` | Artboard with clipping enabled by default |
| `Canvas` | Pixel surface managed by the GPUI host |
| `Custom` | Application type, JSON payload, and fallback vector contours |

To import SVG path geometry, pass a `d` attribute to
`display::Path::from_svg(d)`. It returns local path commands, converting
elliptical arcs to cubic curves.

## Transforms, selection, and events

A world transform composes the object's local transform with its ancestors.
Use `local_to_world` and `world_to_local` to convert coordinates.
`reparent(..., true)` preserves world placement when the destination
transform is invertible.

`Scene::update_object` validates changes before committing. Use
`SceneEditor::begin_edit` and `commit_edit` to combine a drag into one undo
step, or `cancel_edit` to revert it. When both parent and child are selected,
the editor edits the parent's transform and the child follows it.
Locked ancestors protect descendants from editing.

`SceneView::on` registers a listener on an object. Set the capture option to
handle an event on the way to its target. Call `off` to remove a listener. For
pointer-down events, `prevent_default` suppresses default selection and drag
handling.

Run the layout example from the repository root.

```sh
cargo run --locked --manifest-path docs/examples/Cargo.toml --bin layout
```

For more API examples, see the [scene crate reference](../crates/flow-scene/README.md).
