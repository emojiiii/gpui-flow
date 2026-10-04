# Explicit Leafer compatibility profile

`gpui_flow::LeaferCompatibilityProfile` is a bounded opt-in preset, pinned to the
upstream revisions in `leafer-task-ledger.json`. It is not a new document format
or a claim that Flow is a drop-in implementation of the Leafer runtime.
Existing constructors, absent JSON fields, and previously saved documents retain
Flow behavior. Importing Leafer JSON does not silently select this profile.

## Host integration

```rust,ignore
let profile = gpui_flow::LeaferCompatibilityProfile;
let view = gpui_flow::SceneView::new_with_profile(scene, profile, cx)?;
// Or SceneView::from_engine_with_profile(engine, profile, cx).

// Authoring an explicit layout, not retroactively rewriting loaded documents:
let mut style = profile.flow_style();
style.wrap = true;
style.gap = 10.0;
// Existing styles can use profile.configure_flow_style(&mut style).

// Only after verifying plain, effect-free, mask-free Box/Frame conditions:
profile.configure_plain_container_paint(&mut shape,
    gpui_flow::LeaferPlainContainerContext {
        has_children: true,
        children_overflow: true,
        isolated: false,
    });
```

The SceneView constructors configure Escape, the existing resize/drag plugins,
and its event router. Layout and paint policies are explicit authoring helpers;
they do not guess conditions or modify document contents on open. Hosts without
SceneView can install `profile.resize_plugin()` / `profile.drag_plugin()` and
use `profile.event_router()`. `TextEscapeBehavior::close` provides the same
close-vs-cancel decision for headless text-tool adapters.

## Implemented behavior and retained boundaries

- Text Escape retains the latest accepted content and closes as one document undo
  step. The native panel flushes visible preedit through the existing validated
  close path. A rejected close stays open. Explicit Cancel always rolls back the
  still-owned session; stale ownership never undoes someone else's changes.
  `SceneView::new` still cancels on Escape. This remains a native content panel,
  not Leafer's transformed DOM/WYSIWYG editor.
- Named single-selection resizing uses `SceneResizePlugin`'s existing intrinsic
  built-in policies and `TextResizeMode::Box`. Rect/Box/Frame/Image/Canvas change
  size, path-like shapes use admitted geometry rules, and Groups scale direct
  child transforms while retaining their own pose. Existing capability checks,
  unsupported-kind rejection, layout ownership, and multi-selection/unnamed
  transform behavior remain. No new sizing engine or unrestricted Custom policy.
- Layout selects existing `ContentBoundsPolicy::LeaferShrink` and the new
  `WrapGapPolicy::LeaferExcludePendingGap`. Fixed-gap line admission compares
  accumulated line size plus the next item, before inserting that item's gap.
  Thus a line may extend beyond its content width by one gap. Rows, columns,
  and reversed traversal share this rule. Flow's default still includes the
  pending gap. All finite-number, nonnegative-gap, and ownership checks remain.
- Mouse recognition uses a strict `distance > 2px` tap-cancel boundary and 800ms
  long press. A DoubleTap or DoubleClick listener on the live target/ancestor
  path delays a short single for 120ms; otherwise-eligible presses lasting 170ms to less than 800ms emit an
  ordinary single immediately on release. A recognized pair emits only the
  double events. Without those listeners, single taps remain immediate.
  The default drag plugin is configured to start at 2px (its existing inclusive
  threshold), while the legacy plugin still starts at 3px and legacy router slop
  remains 4px. Router timing for pen/touch is unchanged.
- The plain-container helper selects stroke-after-children only for explicit
  isolation, or a clipped container with known overflowing children. Nonisolated
  unclipped Box and nonoverflowing, nonisolated Frame remain before-children.
  It never infers arbitrary masks, erasers, custom paths, effects, or upstream
  automatic surface isolation from a Shape.

Mouse timing is intentionally bounded rather than a recreation of browser timer
state: both releases must be short, same-device, same live target/attachment, no
more than 2px apart, and the second release must precede the first release's
120ms deadline. A second down does not suspend that deadline as upstream does.
An eligible release on another target flushes the earlier single. No cross-target/global tap
counter, `tapMore`, `long_tap`, or listener-dependent long-press release fallback
is added. Long press continues to suppress release taps for safety. Time must
be monotonic; missing timestamps use immediate singles. At most one pending
mouse single is retained within the existing 16-entry bounded tap table.
Cancel, focus loss, detach/replacement, or document reset retire pending work.
SceneView uses its existing coalesced frame callbacks; headless hosts must call
`advance_time` while `needs_gesture_frame()` is true. No hidden timer/thread.

## Source anchors

All anchors use the exact revisions recorded in the existing task ledger:

- [TextEditor.onKeydown / onUnload](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/text-editor/src/TextEditor.ts)
- [Editor editSize default](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/editor/src/config.ts)
- [scaleResize and kind-specific resize](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/resize/src/resize.ts)
- [flowX admission before gap insertion](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/flow/src/layout/flowX.ts)
- [Pointer defaults](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/interaction/interaction/src/config.ts)
- [InteractionBase.tap and pointerMoveReal](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/interaction/interaction/src/Interaction.ts)
- [Box render order](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display/src/Box.ts)
- [BoxData.__drawAfterFill / __clipAfterFill](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display-module/data/src/BoxData.ts)

## Regression coverage

New tests cover both wrap policies and JSON defaults, Escape commit/Cancel undo,
mouse delay, double suppression, deadline/long-press/slop boundaries, listener
removal, pending cancellation and replaced target retirement. Profile helper
unit tests cover layout selection and conditional container order. The existing
native scripted text test additionally exercises both Escape policies with
preedit plus explicit Cancel under the opt-in profile; it remains ignored by
ordinary tests and does not prove real OS IME candidate input.
