# Logical focus and native focus

`SceneEventRouter::set_focus(editor, host, Some(handle))` gives one attached live
instance logical keyboard focus. `clear_focus` clears it; `focused(engine)` only
returns a currently attached owner with the same attachment epoch. Focus does not
select an element or write a style/document property. Selection, hover and focus
remain independent. Disabled state styles remain visual policy; the host decides
which attached elements should be offered focus.

## Notifications and cancellation

A transfer emits the old owner's Blur, then the new owner's Focus. These routed
notifications have related_target, no fabricated pointer position/input, and
cancelable=false. They use the existing capture/target/bubble dispatcher; stopping
propagation affects listeners, while prevent_default cannot veto the transfer.
PointerDown remains cancelable: preventing it still blocks click-based focus.
Manually constructing/dispatching a SceneEvent is a separate low-level API.

Before a programmatic transfer, the router cancels an old pointer gesture/capture
and clears held key/repeat/Space state. Hover is retained until its own pointer or
window-loss event. Repeating a request for the same live owner is a no-op. Clearing
also releases background Space/capture even when no logical element is focused.
An invalid, foreign, destroyed or detached requested handle fails before altering
old focus or input. A detached live handle may be explicitly focused after it is
reattached; its old implicit focus never revives.

## Listener requests, lifetime and bounded dispatch

`SceneFocusRequest::new(engine, origin, target)` captures the live handle and
attachment epoch of both endpoints; prepare it outside the listener, then use
`SceneActionSender::enqueue_focus(request)` inside the listener without borrowing
the engine. If either endpoint remounts, prepare a fresh request. Old prepared
requests cannot acquire the new mount's authority. This avoids recursive access
to a router. It shares the
256-slot input queue with ordinary host commands. Both captured mounts must still be live and attached when processed. Requests
are checked again after the queue batch so intervening commands cannot stale the
pending destination. Ordinary editor commands retain their earlier handle-scoped
protocol; they do not acquire this new focus-specific mount token. Focus requests do not grant editing
permission; queued editor commands still pass the existing host capability policy.

After Blur returns, queued commands run before the candidate receives Focus. If
they destroy, detach, remount or reset the candidate, the obsolete Focus is omitted
and the router reports the invalidation. A valid queued focus request supersedes
the candidate. The last valid request in a drained batch wins. Focus callbacks can
request another transfer; all stages share one 256-action budget for the operation.
Explicit clear and FocusLost reject focus redirects made during their notifications,
so a listener cannot recursively undo the host/window's focus loss.

If mutually redirecting listeners exhaust the budget, the last complete live focus
remains, capture/keys are clean, pending focus redirects are discarded, and a
report diagnostic records the limit. The queue atomically retains pending ordinary editor commands in FIFO order for
the next input operation. There is one shared 256-entry queue, no second backlog.
Concurrent focus senders receive FocusBudgetExhausted until the next router
operation starts; command senders still receive Full if capacity is exhausted.
Callers should handle enqueue errors, including requests attempted by the last
Focus listener after the operation budget has been reached. This prevents an unbounded synchronous loop without silently losing
an edit. A subsequent normal key event works without replaying the focus storm.

Destroyed/reset generations cannot be resolved by a reused public ID. Detach and
reattach between events changes the attachment epoch and invalidates logical focus.
`focused` and state-style reads hide invalid owners immediately; the next routed
operation retires held keys and other stale input. No Blur is fabricated against
an already unavailable instance. Engine lifecycle notifications are the separate
resource-cleanup contract.

## SceneView integration

`SceneView::focus_element(handle, window, cx)` first validates the target and asks
its native GPUI FocusHandle for focus. If the native window declines, it returns
false without changing old logical input. A valid logical request can be redirected
or invalidated by listener actions; the return value says whether the originally
requested handle is the final owner. A redirected live owner retains the view's
native focus. An empty final owner releases this view's native focus.

`clear_element_focus(window, cx)` clears logical focus and blurs native focus only
if this SceneView currently owns it; it does not blur an unrelated widget.
`focused_element()` exposes the live logical owner.

Deferred listener requests set one synchronization flag. On the next render the
view synchronizes GPUI once to the final logical owner, then clears the flag. It
does not request focus on every render. If a queued request is declined by GPUI,
logical focus clears and interaction_error records the failure. A later genuine
FocusLost clears pending synchronization, and a delayed focus-out notification is
ignored if the view already regained native focus. OS window activation is not
requested. This is not Tab navigation, IME, accessibility or an OS-wide focus manager.

The native state_styles example has Focus card / Clear focus controls alongside
hover/press/disabled, transitions and Undo/Redo. Compilation and headless tests do
not claim manual native-window validation.

## History and source boundary

Focus, focus-style transitions and listener registrations are runtime state. They
do not serialize into scene JSON or add Undo steps. Cancelling an active pointer
gesture can restore its baseline; independently queued edit commands have their
usual edit history. API calls themselves never select the new focused element.

The pinned official [state/index.ts](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/state/src/index.ts)
implements ui.focus(value) by clearing the previous focus style, assigning the
interaction's single focusData owner and applying the new focus style after mount.
The [Interaction lookup](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/interaction/interaction/src/Interaction.ts)
uses exact identity for isFocus. That upstream entry point does not define a
cancelable Blur/Focus notification protocol. Our non-cancelable routed notifications,
strict attached-handle validation, deferred requests and bounded redirects are
explicit Rust integration contracts. We do not claim to reproduce upstream's
waitLeafer deferral, parent-button inheritance, full focus manager or browser events.
