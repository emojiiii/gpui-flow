# Custom sizing and asynchronous resources

`ElementDefinition` has optional pure `measure`, `resize_policy`, `resize` and
`resource_key` hooks. Existing definitions remain inert unless they opt in.
Measurement must agree exactly with generated local bounds and declare positive
finite minimum/maximum dimensions (at most one million). Dimensions and Uniform
resize policies return new schema-validated properties; the engine regenerates
geometry and rejects an incorrect resulting size, panic or layout override.

`SceneEngine::preview_custom_resize` preserves affine linear terms and a
normalized local anchor. `SceneView::resize_inner_tool` applies shape and anchor
translation atomically through the active inner-tool token and normal edit
policy. Multiple updates produce one Undo; Cancel restores the owned baseline.
Stale tokens, detached/locked targets and readonly views cannot acquire authority
through the callback. This is the sizing protocol for a host widget; it does not
add arbitrary Custom objects to built-in transform handles.

A nonempty `resource_key` (up to 4096 bytes) declares a required resource.
The host explicitly registers a `SceneCustomResourceProvider` per kind. Imported
keys grant no I/O permissions. The request contains the live engine ticket,
immutable properties and a cancellation token; its future returns owned bytes
or an error. Missing producers fail explicitly. A view admits at most 64 jobs,
each with at most 1 MiB returned bytes/capacity; host I/O, temporary allocations
and external retained copies remain the host's responsibility.

Custom resources participate in the existing pending/loaded/failed/cancelled
events and Ready/ViewReady/Complete receipts. Read accepted payloads with
`custom_resource_bytes`. A geometry/data change, detach, replacement or disposal
invalidates the generation and wakes its cancellation token even before another
render. The adapter drops the future and never publishes its stale result.
External threads/processes must honor the token or own a cancellation-on-drop
guard; arbitrary host work cannot be forcibly terminated. Provider replacement
explicitly invalidates all custom resources in that view. Redraw alone never
retries a failure.

Run `cargo test -p gpui-flow-engine --test custom_protocol` and
`cargo test -p gpui-flow-interaction --test custom_resize` for the core protocol.
`cargo run -p flow-studio --bin custom_protocol` exercises a real widget resize,
Undo/Cancel, asynchronous completion/failure, supersession and view release.

Validation: focused engine/interaction/async regressions cover policy and bounds rejection, anchored resizing, one Undo/Cancel, stale handles, parent layout ownership, source changes before render, explicit cancellation wakeup, dropped waiters, output limits and producer panic. The combined final run passed 1880 default tests/doctests (11 ignored), 304 UI tests, strict all-target checks and formatting. A Linux window exercised sizing/history, real payload completion, pending supersession, failure, readonly and release; all ten started producers were dropped and the final live count was zero. Both sample windows exited normally.
