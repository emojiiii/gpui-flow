# gpui-flow-animation

Opt-in, deterministic property tweens driven by an explicit host clock. Translation,
opacity, solid fill color and registered custom numeric properties update through
one engine transaction per tick. No GPUI, thread, timer, provider or implicit callback.

```rust
use gpui_flow_animation::{AnimationController, AnimationSpec, Tween};
use gpui_flow_editor::SceneEditor;
use gpui_flow_scene::{Scene, DisplayObject, Point, display};
use std::time::Duration;
let mut scene = Scene::new();
scene.insert(DisplayObject::new("card", display::Rect::new(100., 80.)), None, None)?;
let mut editor = SceneEditor::new(scene)?;
editor.transaction(|scene| scene.update_object(&"card".into(), |object| object.transform.tx = 10.))?;
let token = editor.begin_playback()?;
let target = editor.engine.handle(&"card".into()).unwrap();
let mut animations = AnimationController::new(&editor.engine);
animations.animate(&editor.engine, target, AnimationSpec::new(1000,
    vec![Tween::translation(Point::new(100., 0.)), Tween::opacity(0.5)]))?;
animations.tick_editor(&mut editor, token, Duration::from_millis(1000))?;
animations.finish_editor(&mut editor, token)?;
assert!(editor.undo()); // One complete animation, not one entry per frame.
assert_eq!(editor.engine.object(target)?.transform.tx, 10.);
assert!(editor.undo()); // The earlier user edit is still present.
assert_eq!(editor.engine.object(target)?.transform.tx, 0.);
# Ok::<(), Box<dyn std::error::Error>>(())
```

The host chooses when to tick. `sync`, `info`, and playback operations retire
missing/detached/replaced targets and externally overridden tracks. Editor hosts
use begin_playback/tick_editor/seek_editor, then finish_editor (one Undo step) or
cancel_editor (restore baseline, preserve prior Undo/Redo). New edits interrupt
ownership before an old frame can overwrite them. Bare engine tick/seek/cancel
remain low-level APIs for hosts that manage their own history; cancel keeps values. Completed/cancelled records are retained until `remove`, with
256 records and 32 tweens per record maximum. Drop releases only controller data;
there are no timers or engine subscriptions left behind.

This is one-interval interpolation, not a full keyframe timeline. No loops, delay,
reverse/swing, physics, state-style transitions or motion-path animation are
claimed. Color interpolates encoded RGBA channels. Transform animation currently
changes translation only, preserving the other affine coefficients. See
`docs/animation.md` for lifecycle, conflicts, editor history, and source references.
