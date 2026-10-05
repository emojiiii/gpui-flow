# Host-defined editor tools

> 旧示例说明：本文涉及 Studio 的示例名称、源码路径、操作和验证结果均为历史内容，相关演示已移除；库 API 契约继续保留。当前工作台入口见[入门](getting-started.md)，迁移范围见[说明](studio-retirement.md)。

`SceneToolController` owns a registry, one active inner session, and an outer
tool identity. `SceneView` now mounts host-defined GPUI widgets through
`SceneToolWidget`; a widget may return any `AnyElement`, including a retained
`Entity` view. See the complete `custom_tools example`（历史源码已移除）.

## Registration and lifecycle

- `register_inner_tool(id, shape_tag, factory, cx)` binds a canonical shape tag
  to an inner widget. Duplicate IDs and ambiguous inner registrations reject
- `register_outer_tool(id, factory, cx)` associates a widget with an outer tool
  identity. The host still supplies its input plugin; registration grants no
  edit permission and does not replace ordinary canvas input
- `open_inner_tool` validates the target and policy before replacing the current
  session. Reopening the same target/tool preserves the mounted widget
- The factory and `load` run once per activation. `update` receives a changed
  `ToolWidgetContext` containing the target, object snapshot, selection, revision
  and opaque token. Rendering does not recreate the widget
- Switching, disabling, unregistering or disposing calls `unload(reason)`, then
  `destroy`, exactly once for that mounted widget. SceneView release drops its
  widget and factories. Hosts must release their own external Entity references

The standard panel provides Done/Cancel and Escape. Its pointer/scroll events
stay in the panel. Leaving an inner panel commits accepted edits; switching to
the built-in Text editor releases the custom widget. Text marked composition
must finish before a programmatic tool switch.

## Editing and ownership

Use the original `ToolWidgetContext.token` with `set_inner_tool_property` and a
`WeakEntity<SceneView>` in asynchronous callbacks. This goes through the ordinary
command, middleware, property validation and Edit-capability boundary. Tokens
cannot be reused after a session, document replacement or new engine. Detached,
deleted, locked and policy-revoked targets invalidate the session.

Accepted property changes remain visible during the session. Done seals one
document Undo step; Cancel restores the owned baseline. No-op sessions preserve
redo. External edits revoke ownership, so a stale Cancel cannot overwrite newer
changes. An accepted edit that locks its own target succeeds and then closes the
session. Lifecycle callbacks notify the host; they are not a buffered-write hook.

`set_editor_tools_enabled` controls this tool registry and its widgets.
`set_editable` independently controls editing capability. Host-wide shortcuts,
custom input routing and local widget buffers remain the host's responsibility.
Managed LayeredApp surfaces refuse native tool widget opening; detach a layer
before mounting one. This API does not add WYSIWYG, path-point tools or new IME
support.

## Verification

Run `cargo test --locked -p gpui-flow-interaction --test custom_tools --test text_tools`.
The six custom regressions cover registry/policy rejection, single-step history,
Cancel, stale tokens and external edits, lifecycle release, and accepted self-lock.
The existing Text session regressions remain part of the same command.

Run `cargo run --locked -p flow-studio --bin custom_tools` for the native example.
The Linux check exercised Rect edits/history/Cancel, an independently retained
Ellipse Entity, outer/Text switching, policy/disable/unregister, and view
release/recreation. A post-release read reported zero live widgets and Entities,
with ten loads, unloads and destroys. The window exited normally. Immediate
toolbar counts may precede deferred Entity release; use Record after a frame.
