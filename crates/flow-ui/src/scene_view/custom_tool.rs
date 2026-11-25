//! Host widgets are mounted once per tool activation, then explicitly released.
use super::*;
use gpui_flow_engine::{ElementHandle, PropertyKey};
use gpui_flow_interaction::scene::{
    SceneInteractionError, SceneToolController, SceneToolKind, ToolCloseReason, ToolSessionToken,
};
use std::{collections::BTreeMap, rc::Rc};

/// Immutable callback context. Keep the original token in asynchronous callbacks;
/// use a `WeakEntity<SceneView>` to avoid retaining a released view.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolWidgetContext {
    pub tool_id: String,
    pub target: Option<ElementHandle>,
    pub token: Option<ToolSessionToken>,
    pub object: Option<gpui_flow_scene::DisplayObject>,
    pub selection: Vec<DisplayId>,
    pub revision: u64,
}
/// A host may return any GPUI element, including its own retained Entity view.
/// Lifecycle callbacks never grant authority to bypass normal editor policy.
/// `destroy` runs once for every mounted widget, after `unload`, including release.
pub trait SceneToolWidget: 'static {
    fn load(&mut self, _context: &ToolWidgetContext, _cx: &mut Context<SceneView>) {}
    fn update(&mut self, _context: &ToolWidgetContext, _cx: &mut Context<SceneView>) {}
    fn render(&mut self, window: &mut Window, cx: &mut Context<SceneView>) -> AnyElement;
    fn unload(&mut self, _reason: ToolCloseReason, _cx: &mut App) {}
    fn destroy(&mut self, _cx: &mut App) {}
}
type Factory =
    Rc<dyn Fn(ToolWidgetContext, &mut Window, &mut Context<SceneView>) -> Box<dyn SceneToolWidget>>;
#[derive(Default)]
pub(super) struct ToolWidgets {
    factories: BTreeMap<String, Factory>,
    mounted: Option<Mount>,
}
struct Mount {
    context: ToolWidgetContext,
    widget: Box<dyn SceneToolWidget>,
    focus: FocusHandle,
    _focus_out: Subscription,
}
impl SceneView {
    pub fn register_inner_tool<W: SceneToolWidget>(
        &mut self,
        id: impl Into<String>,
        shape_tag: impl Into<String>,
        factory: impl Fn(ToolWidgetContext, &mut Window, &mut Context<Self>) -> W + 'static,
        cx: &mut Context<Self>,
    ) -> Result<(), SceneInteractionError> {
        let id = id.into();
        self.tools
            .get_or_insert_with(SceneToolController::new)
            .register_inner(id.clone(), shape_tag)?;
        self.tool_widgets.factories.insert(
            id,
            Rc::new(move |context, window, cx| Box::new(factory(context, window, cx))),
        );
        self.emit_tool_changes(self.editor.engine.revision(), cx);
        Ok(())
    }
    /// Outer widgets accompany the selected outer identity. Its input plugin is
    /// still supplied by the host; registering UI does not replace scene input.
    pub fn register_outer_tool<W: SceneToolWidget>(
        &mut self,
        id: impl Into<String>,
        factory: impl Fn(ToolWidgetContext, &mut Window, &mut Context<Self>) -> W + 'static,
        cx: &mut Context<Self>,
    ) -> Result<(), SceneInteractionError> {
        let id = id.into();
        self.tools
            .get_or_insert_with(SceneToolController::new)
            .register_outer(id.clone())?;
        self.tool_widgets.factories.insert(
            id,
            Rc::new(move |context, window, cx| Box::new(factory(context, window, cx))),
        );
        self.emit_tool_changes(self.editor.engine.revision(), cx);
        Ok(())
    }
    pub fn custom_tool_active(&self) -> bool {
        self.tools
            .as_ref()
            .and_then(SceneToolController::active_kind)
            == Some(SceneToolKind::CustomInner)
    }
    pub fn inner_tool_active(&self) -> bool {
        self.text_editor_active() || self.custom_tool_active()
    }
    pub fn open_inner_tool(
        &mut self,
        id: &str,
        target: ElementHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<ToolSessionToken, SceneInteractionError> {
        if self.is_layer_managed() {
            return Err(SceneInteractionError::InvalidInput(
                "Detach this layer before opening a native inner tool".into(),
            ));
        }
        let tools = self
            .tools
            .as_ref()
            .ok_or_else(|| SceneInteractionError::InvalidInput("tool is not registered".into()))?;
        tools.validate_inner_target(&self.editor, &self.interactions, id, target)?;
        if !self.tool_widgets.factories.contains_key(id) {
            return Err(SceneInteractionError::InvalidInput(
                "tool has no host widget".into(),
            ));
        }
        if self.text_editor_active() {
            self.flush_text_for_tool_switch(window, cx)?;
        }
        self.close_image_crop_editor(cx);
        self.focus_lost(cx);
        let before = self.editor.engine.revision();
        let token = self
            .tools
            .as_mut()
            .expect("registered controller")
            .open_inner(&mut self.editor, &mut self.interactions, id, target)?;
        self.sync_tool_widget(window, cx);
        if let Some(mount) = &self.tool_widgets.mounted {
            window.focus(&mount.focus, cx);
        }
        self.emit_tool_changes(before, cx);
        Ok(token)
    }
    pub fn set_inner_tool_property(
        &mut self,
        token: ToolSessionToken,
        key: PropertyKey,
        value: serde_json::Value,
        cx: &mut Context<Self>,
    ) -> Result<(), SceneInteractionError> {
        let before = self.editor.engine.revision();
        let result = self
            .tools
            .as_mut()
            .ok_or_else(|| {
                SceneInteractionError::InvalidInput("tool controller was released".into())
            })?
            .set_inner_property(&mut self.editor, &mut self.interactions, token, key, value);
        self.emit_tool_changes(before, cx);
        result
    }
    pub fn finish_inner_tool(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.close_custom_tool(false, ToolCloseReason::Done, window, cx)
    }
    pub fn cancel_inner_tool(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.close_custom_tool(true, ToolCloseReason::Cancelled, window, cx)
    }
    pub(super) fn close_custom_tool(
        &mut self,
        cancel: bool,
        reason: ToolCloseReason,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.custom_tool_active() {
            return false;
        }
        let before = self.editor.engine.revision();
        let tools = self.tools.as_mut().expect("active controller");
        let token = tools.active_token().expect("active token");
        let result = if cancel {
            tools.cancel(&mut self.editor, &mut self.interactions, token)
        } else {
            tools.finish_with_reason(&mut self.editor, &mut self.interactions, token, reason)
        };
        let closed = match result {
            Ok(value) => value,
            Err(error) => {
                self.interaction_error = Some(error.to_string());
                false
            }
        };
        if self
            .tools
            .as_ref()
            .and_then(SceneToolController::active_token)
            != Some(token)
        {
            let focused = self
                .tool_widgets
                .mounted
                .as_ref()
                .is_some_and(|m| m.focus.contains_focused(window, cx));
            self.release_tool_widget(
                if closed {
                    reason
                } else {
                    ToolCloseReason::Invalidated
                },
                cx,
            );
            if focused {
                window.focus(&self.focus, cx);
            }
        }
        self.emit_tool_changes(before, cx);
        closed
    }
    pub fn select_outer_tool(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<bool, SceneInteractionError> {
        if self.is_layer_managed() {
            return Err(SceneInteractionError::InvalidInput(
                "Detach this layer before selecting a native tool widget".into(),
            ));
        }
        let tools = self
            .tools
            .as_ref()
            .ok_or_else(|| SceneInteractionError::InvalidInput("tool is not registered".into()))?;
        if !tools
            .registry()
            .get(id)
            .is_some_and(|entry| entry.kind == SceneToolKind::Outer)
        {
            return Err(SceneInteractionError::InvalidInput(
                "unknown outer tool".into(),
            ));
        }
        if self.text_editor_active() {
            self.flush_text_for_tool_switch(window, cx)?;
        }
        let before = self.editor.engine.revision();
        let changed = self
            .tools
            .as_mut()
            .expect("registered controller")
            .select_outer(&mut self.editor, &mut self.interactions, id)?;
        if changed {
            self.release_tool_widget(ToolCloseReason::OuterToolSwitch, cx);
        }
        self.sync_tool_widget(window, cx);
        self.emit_tool_changes(before, cx);
        Ok(changed)
    }
    pub fn unregister_editor_tool(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<bool, SceneInteractionError> {
        if self.text_input.is_some()
            && self
                .tools
                .as_ref()
                .and_then(SceneToolController::active_tool_id)
                == Some(id)
        {
            self.flush_text_for_tool_switch(window, cx)?;
        }
        let before = self.editor.engine.revision();
        let changed = match &mut self.tools {
            Some(tools) => tools.unregister(&mut self.editor, &mut self.interactions, id)?,
            None => false,
        };
        if changed {
            if self
                .tool_widgets
                .mounted
                .as_ref()
                .is_some_and(|m| m.context.tool_id == id)
            {
                self.release_tool_widget(ToolCloseReason::Unregistered, cx);
            }
            self.tool_widgets.factories.remove(id);
        }
        self.emit_tool_changes(before, cx);
        Ok(changed)
    }
    pub fn set_editor_tools_enabled(
        &mut self,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), SceneInteractionError> {
        if !enabled && self.text_editor_active() {
            self.flush_text_for_tool_switch(window, cx)?;
        }
        let before = self.editor.engine.revision();
        if let Some(tools) = &mut self.tools {
            tools.set_enabled(&mut self.editor, &mut self.interactions, enabled)?;
        }
        if !enabled {
            self.release_tool_widget(ToolCloseReason::Disabled, cx);
        }
        self.emit_tool_changes(before, cx);
        Ok(())
    }
    pub(super) fn release_tool_widget(&mut self, reason: ToolCloseReason, cx: &mut App) {
        if let Some(mut mount) = self.tool_widgets.mounted.take() {
            mount.widget.unload(reason, cx);
            mount.widget.destroy(cx);
        }
    }
    pub(super) fn has_tool_widget(&self) -> bool {
        self.tool_widgets.mounted.is_some()
    }
    pub(super) fn dispose_tool_widgets(&mut self, cx: &mut App) {
        self.release_tool_widget(ToolCloseReason::Disposed, cx);
        self.tool_widgets.factories.clear();
    }
    pub(super) fn sync_tool_widget(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let desired = self
            .tools
            .as_ref()
            .and_then(|tools| {
                if !tools.is_enabled() {
                    return None;
                }
                if tools.active_kind() == Some(SceneToolKind::CustomInner) {
                    Some((
                        tools.active_tool_id()?.to_owned(),
                        tools.active_target(),
                        tools.active_token(),
                    ))
                } else if tools.active_token().is_none()
                    && tools.outer_tool_active(tools.outer_tool())
                {
                    Some((tools.outer_tool().to_owned(), None, None))
                } else {
                    None
                }
            })
            .filter(|(id, _, _)| self.tool_widgets.factories.contains_key(id));
        let Some((id, target, token)) = desired else {
            let reason = if self.tools.as_ref().is_some_and(|tools| !tools.is_enabled()) {
                ToolCloseReason::Disabled
            } else if self
                .tools
                .as_ref()
                .and_then(SceneToolController::active_token)
                .is_some()
            {
                ToolCloseReason::InnerOpened
            } else {
                ToolCloseReason::Invalidated
            };
            self.release_tool_widget(reason, cx);
            return;
        };
        let same = self.tool_widgets.mounted.as_ref().is_some_and(|m| {
            m.context.tool_id == id && m.context.target == target && m.context.token == token
        });
        let context = ToolWidgetContext {
            tool_id: id.clone(),
            target,
            token,
            object: target.and_then(|t| self.editor.engine.object(t).ok().cloned()),
            selection: self.editor.selection.ids.iter().cloned().collect(),
            revision: self.editor.engine.revision(),
        };
        if !same {
            let reason = if self
                .tool_widgets
                .mounted
                .as_ref()
                .is_some_and(|m| m.context.token.is_none())
            {
                if token.is_some() {
                    ToolCloseReason::InnerOpened
                } else {
                    ToolCloseReason::OuterToolSwitch
                }
            } else {
                ToolCloseReason::TargetSwitch
            };
            self.release_tool_widget(reason, cx);
            let factory = self
                .tool_widgets
                .factories
                .get(&id)
                .expect("known factory")
                .clone();
            let mut widget = factory(context.clone(), window, cx);
            widget.load(&context, cx);
            let focus = cx.focus_handle();
            let focus_copy = focus.clone();
            let focus_out = cx.on_focus_out(&focus, window, move |this, _, window, cx| {
                if token.is_some()
                    && this
                        .tools
                        .as_ref()
                        .and_then(SceneToolController::active_token)
                        == token
                    && !focus_copy.contains_focused(window, cx)
                {
                    this.close_custom_tool(false, ToolCloseReason::Blur, window, cx);
                }
            });
            self.tool_widgets.mounted = Some(Mount {
                context,
                widget,
                focus,
                _focus_out: focus_out,
            });
        } else if let Some(mount) = &mut self.tool_widgets.mounted
            && mount.context != context
        {
            mount.widget.update(&context, cx);
            mount.context = context;
        }
    }
    pub(super) fn custom_tool_panel(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let mount = self.tool_widgets.mounted.as_mut()?;
        let token = mount.context.token;
        let content = mount.widget.render(window, cx);
        let theme = self.theme;
        let mut panel = div()
            .id("scene-host-tool")
            .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
            .on_mouse_move(|_, _, cx| cx.stop_propagation())
            .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_scroll(|_, _, cx| cx.stop_propagation())
            .track_focus(&mount.focus)
            .absolute()
            .top_3()
            .right_3()
            .w(px(340.))
            .max_w_full()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .rounded_md()
            .bg(rgb(theme.panel))
            .border_1()
            .border_color(rgb(theme.border))
            .text_color(rgb(theme.foreground))
            .child(content);
        if token.is_some() {
            panel = panel
                .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    if event.keystroke.key == "escape" {
                        this.cancel_inner_tool(window, cx);
                        cx.stop_propagation();
                    }
                }))
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            crate::ControlButton::new("host-tool-done", "Done")
                                .theme(theme)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.finish_inner_tool(window, cx);
                                })),
                        )
                        .child(
                            crate::ControlButton::new("host-tool-cancel", "Cancel")
                                .theme(theme)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.cancel_inner_tool(window, cx);
                                })),
                        ),
                );
        }
        Some(panel.into_any_element())
    }
}
