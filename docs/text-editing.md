# Retained Text content editing and tool sessions

Status: bounded implementation with Linux local regression and native-window
validation. Exact coverage and remaining platform/IME limits are below. This is
not a full Leafer editor, WYSIWYG, or cross-platform input acceptance claim.

## Scope and reuse

This is an opt-in, target-aware editor-tool slice, not full Leafer tool or Text
compatibility. The existing scene selection/transform tool remains the default
outer tool. The per-controller registry resolves a supported inner tool by target
shape and owns at most one inner session. Registrations and lifecycle events are
platform-neutral; arbitrary custom inner widgets and path/line editing tools
are not provided by this panel. The separate [bounded outer-handle slice](resize-handles.md)
adds eight single-selection handles and remains suspended during Text editing.

`SceneView::set_text_editing_enabled(true, window, cx)` enables native Text
double-click activation. `open_text_editor(handle, window, cx)` explicitly opens a
live instance and opts in if needed. Repeated opening of the same session keeps
the input's caret, selection and local history. A native activation uses GPUI's
`click_count == 2`; modified clicks, resize handles and Space-pan do not activate
it. A prevented routed PointerDown also prevents native activation.

The input is a clearly labelled **Text content** panel. It reuses the pinned
`gpui-kit` revision `0067bafcf3160cb871eff2521e98a51e526da175` component
`Textarea` and `TextareaState`. Kit owns the text buffer, shaping, caret/selection,
clipboard, input-local undo and native text/marked-text handling. No replacement
text or clipboard framework, dependency or double-click timer is added.

The panel uses GPUI focus traversal for Tab/Shift-Tab instead of inserting an
indent. Kit keyboard cut/copy/paste remain available. Its right-click menu is
explicitly disabled with the public `TextareaState::context_menu(false)` API:
the pinned Linux fallback menu takes focus outside the panel and has no public
ownership/dismissal hook suitable for commit-on-blur. No Kit internals are changed.

The panel does not match Text geometry, font, transforms or wrapping. It is not a
WYSIWYG overlay. The actual retained Text stays visible on the canvas, rendered
through the existing text pipeline. Installed text-measurement/layout providers
remeasure as part of the same ordinary engine change.

## Close, ownership and conflicts

- Done, focus leaving the content panel, window hiding/deactivation, disabling and valid target switch
  keep accepted content and seal one document undo step
- Escape and Cancel roll back the owned session baseline. This is an intentional
  difference from Leafer's Escape/close behavior, which keeps content
- An unchanged session makes no history entry and keeps the old redo branch
- Kit-local undo/redo applies while typing; after close, scene undo/redo applies
  to the completed session. SceneView's scene-keyboard entry points yield while
  an inner session owns input. Ordinary key propagation remains available for
  GPUI's native text delivery; consuming every bubbling KeyDown would block
  typing. Host-wide shortcuts remain the embedding application's responsibility
- Canvas clicks finish first and then resume ordinary outer interaction. Inner
  editing suspends outer selection/resize decoration and pointer defaults
- Callbacks carry an opaque session token. The controller also checks the engine,
  live handle generation, attachment epoch and document epoch. Late callbacks
  cannot edit a same-ID replacement or a newly opened session
- Ordinary editor edits and direct engine changes revoke ownership. Completed
  session edits and the subsequent external changes remain separate history
  steps, even if the external edit changed unrelated geometry rather than content.
  Multiple direct-engine writes before reconciliation form one external net-change
  step. Stale cancel never restores a whole old document over external changes
- Explicit document replacement keeps the existing load contract: clear history
  and invalidate old sessions. Detach/reinsert, deletion, locks and policy changes
  invalidate the old target. No invalid session is revived automatically
- Tab/Shift-Tab between the input and Done/Cancel stays within the session;
  leaving the whole tracked panel commits. Closing from an internal button
  restores canvas focus; focus already moved to another host control is retained
- Repeated blur/Done and old-token closes are harmless. Releasing SceneView
  disposes the tool controller and seals already accepted content; no callback
  retains ownership of the released view

Kit may keep preedit local without a Change notification during IME composition.
A programmatic switch to another target is rejected while a marked range exists;
finish the composition first. Done/blur explicitly unmarks the Kit state and keeps its latest
visible value, including preedit. Escape/Cancel unmarks and rolls back the whole
session. An allowed target switch flushes the old input before replacing it and emits a
document-change event for the flush. This policy does not promise that the
platform chose a conversion candidate. Local `unmark_text` is not an OS-level IME
reset: late native commits are routed by the platform's current input handler,
not by the old session token. Programmatic close/reopen during native composition,
candidate windows and focus interruption require platform-specific QA and are
not claimed safe across all IMEs. Hosts should finish composition before such
programmatic focus changes.

## Mutation boundary

`PropertyKey::TextContent` accepts only a string on a Text element. It changes
only `Text.content`, preserving font, style, bounds and unrelated object fields
before normal derived layout runs.
Text editing never replays a cached whole `Shape`. It uses the engine's ordinary
validation and derivation, the SceneEditor history system with token-owned
session methods, and
ScenePluginHost Edit capability, command filters and ancestor-lock policy.

An opaque owned edit token prevents a different controller from cancelling or
committing a newer session. `begin_edit()` seals a previous owned session before
starting an independent generic gesture; `begin_playback()` rejects an active
owned session. Generic `commit_edit()` and `cancel_edit()` deliberately cannot
finish token-owned sessions; use their token-aware counterparts. Invalid mutations remain atomic. The input can report
a rejected update without claiming the document accepted it. If a host command
filter normalizes content, the native buffer/caret/local undo stay intact; a panel
notice explains that the canvas shows the accepted normalized result, and reopen
starts from that canonical result. The last accepted input value prevents Done
or blur from reapplying a non-idempotent normalization.

Low-level engine and editor APIs remain trusted host APIs; AccessPolicy applies
to the interaction command path, not to arbitrary host writes. This is not a
security sandbox or a collaborative merge system.

## Source references

The contract was compared with the pinned official Leafer plugin revision
`3eb445bf34debb638766615282224b77108bf610`:

- [Editor tool selection and inner-editor switching](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/editor/src/Editor.ts)
- [InnerEditor lifecycle](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/editor/src/tool/InnerEditor.ts)
- [TextEditor content and keyboard ownership](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/text-editor/src/TextEditor.ts)

This independent Rust implementation does not import those sources or change
licenses. Browser DOM placement, HTMLText/rich text, arbitrary transformed inline
editing, simulated multiple targets and complete tool factory parity remain out
of scope.

## Local validation (2026-10-01)

- 912 headless tests passed, including doctests; 8 existing ignored tests remained
  ignored. This slice adds 4 engine property/resource, 7 owned-history, 16 tool
  lifecycle/policy/conflict and 2 text-layout regressions
- 98 GPUI library tests passed, including the added native click-count and
  modifier/resize/Space activation filter regression
- Workspace/all-targets strict Clippy, workspace/docs formatting, docs tests and
  strict Clippy, all 12 executable documentation examples, and the Linux native
  `text_editing` example build/link passed with locked offline dependencies
- Real Linux window checks covered explicit host opening, native double-click,
  repeated opening/switching, ordinary multiline input, Space/R/Delete isolation,
  Ctrl+A/C/V with English/Chinese/emoji, local Ctrl+Z/Ctrl+Y, one completed scene
  Undo/Redo, read-only rejection, Escape rollback, keyboard Done and Cancel,
  Tab/Shift-Tab inside the panel, blur to a host control, reopening, and closing
  the window during an active edited session. Native runs exited 0
- A lifecycle trace confirmed a single Cancelled event for keyboard Cancel and
  ordered target-switch/blur closes. The right-click menu remained disabled
- Initial native checks caught blocked raw character delivery, incompatible
  inherited input colors, Tab-action priority and a stale panel after draw-time
  blur. They were corrected; the final post-frame repaint check showed the panel
  disappearing without any later pointer movement. The notification uses one
  weak next-frame callback, not a timer or a polling loop

The executable example is `cargo run --locked -p flow-studio --bin text_editing`.
Optional `-- --trace` logs lifecycle metadata and change notifications, not text
contents. The headless API example is
`cargo run --locked --manifest-path docs/examples/Cargo.toml --bin text_editing`.

Not verified here: real native IME/candidate interactions (including programmatic
composition interruption), arbitrary host normalization policies in a native
window, full accessibility/DPI matrices, Windows/macOS, and edited-text pan/zoom/
transform combinations. Source-level safeguards and model fixtures do not prove
those platform behaviors. GitHub Actions and deployment were not run.

## Broader acceptance checklist (not all passed)

For future platform validation, extend the recorded checks with:

1. Explicit host open and native double-click; prevented and modified activation
2. Multiline Unicode and plain-text paste; typing Space/R/Delete and Ctrl/Cmd-A/Z
   changes only the input; Done → one scene Undo/Redo
3. Escape rollback, blank/no-op session and old redo preservation
4. Repeat Done/blur/open, switch title/body, focus another host control, deactivate
   the window and close it while editing
5. External unrelated/content edits, lock/read-only change, delete/recreate same
   ID and document load while input is active; stale callbacks cannot overwrite
6. Native IME composition followed by Done, Escape, blur, switch and deactivation;
   then reopen and continue typing, including candidate-popup keyboard behavior
7. Pan/zoom while closed, measured reflow, changed frame size and transformed Text
   remain visibly canvas behavior; no WYSIWYG fidelity claim for panel text

Cross-platform input, clipboard, accessibility and native IME validation are still
required even after headless tests and linking pass.
