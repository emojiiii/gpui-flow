# Opt-in GPUI frame layout batching

`SceneView::set_flow_frame_queue` installs an explicit `FlowFrameQueue`.
`enqueue_flow_edit` validates and stages host edits without modifying the
committed editor. It posts one actual GPUI `Window::on_next_frame` callback;
subsequent edits before that callback join the same queue. Existing editor
transactions remain immediate. The callback runs after the requested frame,
commits the combined layout/input transaction once, then notifies the view for
painting. This is not an implicit debounce of every editor operation.

`SceneViewEvent::FlowFrame` records staged edit count, generation, before/after
revision and either a `FlowReport` or an error. This is a document commit receipt,
not proof of rendering or a GPU fence. A changed revision also emits `Changed`.
The callback captures only a weak view entity, so it cannot keep a released view
alive. Queue replacement and cancellation invalidate old callback generations;
a delayed callback cannot flush a replacement batch.

A failed callback preserves pending edits for explicit retry or cancellation.
Foreign engine/document/revision/provider stamps reject in the model queue;
window migration rejects in the adapter. An outstanding callback associated
with another window cannot be retargeted by enqueue/retry. Cancel and restage
if its old window has closed. `invalidate_flow_frame_layout` forces the next
batch to arrange fully after the host separately invalidates its font backend.

This is a trusted programmatic host API, like direct editor transactions.
Disabling pointer editing is not a security boundary for host code. The bounded
model queue, size ownership, percentages, baselines and dirty-selection rules
are defined in [advanced Flow layout](advanced-flow-layout.md).

## Current validation

Five pure adapter-state tests pass: twenty-one staged edits generate one commit
and one Undo; repeated scheduling reuses a callback; cancellation/replacement
ignore old tokens; external edits are not overwritten on rejection/retry;
window migration and generation exhaustion fail without publication. The
native UI library compile also passes. These tests invoke the isolated adapter
state machine, **not** a physical frame callback. Actual scheduled-callback and
pixel acceptance remains pending and is required before closing LAY-03.
