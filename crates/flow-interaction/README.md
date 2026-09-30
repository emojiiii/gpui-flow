# gpui-flow-interaction

Composable, toolkit-independent input behavior for graph canvases and retained
scenes. This is an event-to-command plugin system, not a list of booleans wrapped
around a monolithic mouse handler.

## Shared contract, two document adapters

- `CanvasPlugin<D = Editor, C = CanvasCommand>` defines lifecycle, ordered event
  hooks, and command middleware
- `PluginContext<'_, D>` contains immutable document/editor access and policy
- `PluginOutput<C>` collects typed commands, effects, and propagation decisions
- `PluginHost` applies graph commands through `gpui-flow-core::Editor`
- `scene::ScenePluginHost` applies scene commands through `SceneEditor`
- Both hosts use the same `CanvasEvent`, `AccessPolicy`, `PluginLifecycle`,
  `CanvasCursor`, `CanvasEffect`, and `DispatchReport` protocol

A GPUI adapter maps pointer coordinates and hit targets, dispatches an event, and
renders the returned effects. It does not own drag, resize, snapping, connection,
or marquee state machines.

## Actual independently installed behaviors

`PluginHost::default()` is empty. `with_defaults()` installs:

- `selection`: node/edge/group selection and marquee state
- `drag`: screen-pixel threshold, selected geometry snapshots, one undo gesture
- `resize`: resize state and configurable minimum size
- `connection`: a `ConnectionDrag` controller, normalized source/target intent
- `snapping`: command middleware for compatible port proximity, optional grid
  snapping, and node alignment
- `pan`: middle-button / space-drag and unmodified-wheel navigation
- `zoom`: pointer-anchored command-wheel zoom
- `keyboard`: selection, history, deletion, clipboard/file requests, fit, and grid

Graph defaults enable port/alignment snapping but no grid. Configure a replacement
`SnappingPlugin` with `SnapConfig` to change this policy. Disabling snapping removes
proximity attraction while exact port hits still connect.

The scene host installs separate scene selection, drag, resize, pan, zoom, and
keyboard implementations with those same IDs. Optional `SceneSnappingPlugin`
adds world-grid middleware. Scene drag correctly moves selected roots under
transformed parents. Scene resize uses local subtree bounds and the shared
negative-origin-safe resize helper; Shift, text, and image resizing stay uniform.
Scene marquee uses visible clipped bounds and hit/lock policies.

Lower `priority()` values run first; equal priorities preserve installation order.
Selection changes are committed before the drag plugin receives its context, so
it sees the current selection. A disabled selection plugin cannot accidentally
make drag move unrelated previously selected objects.

## Custom behavior

```rust
use gpui_flow_interaction::*;

struct MinimumWidth;
impl CanvasPlugin for MinimumWidth {
    fn id(&self) -> &str { "minimum-width" }

    fn filter_command(
        &mut self,
        command: &mut CanvasCommand,
        _: &PluginContext<'_>,
        _: &mut Vec<CanvasEffect>,
    ) -> Result<(), String> {
        if let CanvasCommand::ResizeNode { size, .. } = command {
            size.width = size.width.max(320.0);
        }
        Ok(())
    }
}

let mut host = PluginHost::with_defaults();
host.install(MinimumWidth)?;
```

An event plugin instead implements `on_event`, emits typed commands, and can call
`prevent_default()` or `stop_propagation()`. Default-action plugins are skipped
after default prevention; custom observers may still run. Pointer capture routes
default pointer behavior to its owner while retaining custom event observers.
Command middleware executes in priority order before final validation. For a
scene-specific extension use `CanvasPlugin<SceneEditor, SceneCommand>` with the
same interface.

## Lifecycle and transactions

`install`, `uninstall`, `replace`, and `set_enabled` are real lifecycle actions.
Hooks observe Installed, Enabled, Disabled, Cancelled, and Uninstalled states.

- Removing/disabling the gesture owner cancels its editor transaction
- Removing participating snap middleware also cancels an affected gesture
- Replacement cancels an active gesture before swapping behavior
- Live `install` is rejected while a gesture is captured; cancel first or use
  replacement with an editor argument
- Policy changes cancel in-flight gestures before the new policy takes effect
- Missing pressed-button state, Escape, focus loss, pointer cancellation,
  mismatched buttons, and navigation interruptions roll back; they never commit
  a partial drag just because a release was missed
- Releasing pointer capture with an unfinished owned transaction cancels it
- Commit produces one undo entry; cancellation leaves no new history entry

The host tracks transaction ownership. Toolbar or unrelated plugin mutations
cannot merge into another plugin's in-flight gesture. Cleanup commands cannot be
rewritten or rejected by command middleware. If a command fails, the host cancels
the gesture and does not apply stale cursor/overlay effects from that plugin.

## Read-only means capability enforcement

`AccessPolicy` separates editing, navigation, selection, and clipboard-read
capabilities. `read_only()` disables editing while retaining navigation and
selection; configure either independently.

Every external UI command and plugin command is checked before middleware and
again afterward. Read-only blocks add/delete/group, connection creation,
resize/move, paste, undo/redo, field data edits, and document import/load. Middleware
cannot rewrite an allowed selection operation into an unauthorized mutation.

Applications should route toolbar and input callbacks through `execute`, not just
pointer events through `dispatch`. `execute` returns
`Result<DispatchReport, InteractionError>`; `dispatch` returns a report containing
any errors. UI controls can inspect `allows(Capability::Edit)` and `is_enabled`.

The editor belongs to the application. Direct editor access remains a trusted
host escape hatch. These policies enforce UI/plugin command paths; they are not a
sandbox for adversarial native Rust code.

## Platform requests and persistence

Effects request operations that need a toolkit or host context: Paste, Save,
CopyDocument, Fit, and ToggleGrid. Subsequent edits must still use host commands.
Ctrl/Cmd+S requests normal save; Ctrl/Cmd+Shift+S explicitly requests document
copy. Keyboard input inside a focused text editor must be filtered by the GUI
adapter before canvas dispatch.

Graph `ImportGraph` is undoable; `LoadDocument` resets history. Scene `ImportScene`
is undoable; `LoadScene` resets history. `PasteScene` validates the fragment,
allocates fresh IDs, preserves parent/child relationships, and offsets roots in
world coordinates.

No gestures, callbacks, installed plugin instances, or transaction ownership are
serialized with documents. Persistent plugin preferences belong to the host.

## Verification

```sh
cargo test -p gpui-flow-interaction
cargo clippy -p gpui-flow-interaction --all-targets -- -D warnings
cargo fmt -p gpui-flow-interaction -- --check
```

The crate includes 29 graph-host and 16 scene-host integration tests covering real
behavior, replacement, cancellation, read-only commands, screen-space thresholds,
port snapping, affine scene editing, clipping-aware marquee, history, and custom
plugin interception. Native pointer wiring and platform clipboard/file behavior
are tested in the GPUI adapter separately.
