//! A content editor, deliberately not a transformed WYSIWYG text overlay.
//! Kit owns text storage, native input/IME, selection, clipboard and local undo.
use super::*;
use gpui_flow_interaction::scene::{
    SceneInteractionError, SceneToolController, TextSessionToken, ToolCloseReason,
};
use gpui_kit::component::{
    ActiveTheme as _,
    input::{IndentInline, InputEvent, OutdentInline, Textarea, TextareaState},
};

pub(super) struct TextInputSession {
    token: TextSessionToken,
    state: Entity<TextareaState>,
    panel_focus: FocusHandle,
    last_submitted: SharedString,
    normalized: bool,
    _subscription: Subscription,
    _focus_subscription: Subscription,
}

impl SceneView {
    /// Opt in to plain retained Text editing. Native double-click opens a
    /// content panel; all existing canvas behavior is unchanged while disabled.
    /// Disabling commits the latest visible text, including IME preedit.
    pub fn set_text_editing_enabled(
        &mut self,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if enabled {
            self.tools
                .get_or_insert_with(SceneToolController::with_text);
        } else {
            self.close_text_editor(false, ToolCloseReason::Disabled, window, cx);
            if self.text_input.is_none()
                && let Some(mut tools) = self.tools.take()
            {
                let _ = tools.dispose(&mut self.editor, &mut self.interactions);
                for event in tools.drain_events() {
                    cx.emit(SceneViewEvent::EditorTool(event));
                }
            }
        }
        cx.notify();
    }
    pub fn text_editing_enabled(&self) -> bool {
        self.tools.is_some()
    }
    pub fn text_editor_active(&self) -> bool {
        self.text_input.is_some()
    }
    pub fn text_editor_target(&self) -> Option<gpui_flow_engine::ElementHandle> {
        self.tools.as_ref()?.active_target()
    }
    /// Open exactly this live Text instance, enabling the optional tool if
    /// needed. An unsupported or denied target leaves the current editor open.
    pub fn open_text_editor(
        &mut self,
        target: gpui_flow_engine::ElementHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<TextSessionToken, SceneInteractionError> {
        self.reconcile_text_editor(window, cx);
        // Same-session activation never resets text selection or local history.
        if self.text_editor_target() == Some(target)
            && let Some(input) = &self.text_input
        {
            input.state.update(cx, |state, cx| state.focus(window, cx));
            return Ok(input.token);
        }
        // The controller validates the new target before sealing the old one.
        // End outer capture/Space first; the text widget gets native focus only
        // after the owned edit has been established.
        if !self.text_editor_active() {
            self.focus_lost(cx);
        }
        let before = self.editor.engine.revision();
        let tools = self
            .tools
            .get_or_insert_with(SceneToolController::with_text);
        tools.validate_text_target(&self.editor, &self.interactions, target)?;
        if let Some(previous) = &self.text_input {
            if previous.state.update(cx, |state, cx| {
                state.marked_text_range(window, cx).is_some()
            }) {
                return Err(SceneInteractionError::InvalidInput(
                    "Finish the current IME composition before switching text targets".into(),
                ));
            }
            let value = previous.state.read(cx).value();
            if value != previous.last_submitted
                && let Err(error) = tools.set_text(
                    &mut self.editor,
                    &mut self.interactions,
                    previous.token,
                    value.as_ref(),
                )
            {
                self.emit_text_tool_changes(before, cx);
                self.reconcile_text_editor(window, cx);
                return Err(error);
            }
        }
        let token = match tools.open_text(&mut self.editor, &mut self.interactions, target) {
            Ok(token) => token,
            Err(error) => {
                self.emit_text_tool_changes(before, cx);
                self.reconcile_text_editor(window, cx);
                return Err(error);
            }
        };
        let value: SharedString = tools.active_text().unwrap_or_default().to_owned().into();
        if let Some(previous) = self.text_input.take() {
            previous
                .state
                .update(cx, |state, cx| state.unmark_text(window, cx));
        }
        let state = cx.new(|cx| {
            TextareaState::new(window, cx)
                // Kit's Linux menu lives outside the panel's focus subtree.
                // Keep keyboard clipboard actions; do not close under a live menu.
                .context_menu(false)
                .default_value(value.clone())
        });
        let subscription = cx.subscribe_in(
            &state,
            window,
            move |this, state, event: &InputEvent, window, cx| {
                if this.text_input.as_ref().map(|input| input.token) != Some(token) {
                    return;
                }
                match event {
                    InputEvent::Change => {
                        let value = state.read(cx).value();
                        let before = this.editor.engine.revision();
                        if let Some(tools) = &mut this.tools {
                            match tools.set_text(
                                &mut this.editor,
                                &mut this.interactions,
                                token,
                                value.as_ref(),
                            ) {
                                Ok(()) => {
                                    this.interaction_error = None;
                                    // Preserve Kit's caret and local undo. A normalized
                                    // engine value is shown on the canvas and on reopen;
                                    // the accepted input watermark prevents close from
                                    // replaying non-idempotent host normalization.
                                    if let Some(input) = &mut this.text_input {
                                        input.normalized = tools
                                            .active_text()
                                            .is_some_and(|canonical| canonical != value.as_ref());
                                        input.last_submitted = value;
                                    }
                                }
                                Err(error) => this.interaction_error = Some(error.to_string()),
                            }
                        }
                        this.emit_text_tool_changes(before, cx);
                        this.reconcile_text_editor(window, cx);
                    }
                    // Ignore a queued blur after native focus already came
                    // back, or after another token took ownership.
                    InputEvent::Blur if !this.text_panel_contains_focus(window, cx) => {
                        this.close_text_editor(false, ToolCloseReason::Blur, window, cx);
                    }
                    _ => {}
                }
            },
        );
        let panel_focus = cx.focus_handle();
        let focus_subscription =
            cx.on_focus_out(&panel_focus, window, move |this, _, window, cx| {
                if this.text_input.as_ref().map(|input| input.token) == Some(token)
                    && !this.text_panel_contains_focus(window, cx)
                {
                    this.close_text_editor(false, ToolCloseReason::Blur, window, cx);
                }
            });
        self.text_input = Some(TextInputSession {
            token,
            state: state.clone(),
            panel_focus,
            last_submitted: value,
            normalized: false,
            _subscription: subscription,
            _focus_subscription: focus_subscription,
        });
        self.pending_native_focus = false;
        state.update(cx, |state, cx| state.focus(window, cx));
        self.emit_text_tool_changes(before, cx);
        Self::repaint_text_editor(window, cx);
        Ok(token)
    }
    /// Commit one document undo step. Kit-local undo is discarded on close.
    /// Repeated calls are harmless. Returns false for no session or a rejection.
    pub fn finish_text_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.close_text_editor(false, ToolCloseReason::Done, window, cx)
    }
    /// Roll back this session only while its ownership remains current.
    /// Unlike Leafer's close-on-Escape behavior, this intentionally cancels.
    pub fn cancel_text_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.close_text_editor(true, ToolCloseReason::Cancelled, window, cx)
    }
    fn text_panel_contains_focus(&self, window: &Window, cx: &App) -> bool {
        self.text_input.as_ref().is_some_and(|input| {
            input.state.read(cx).focus_handle(cx).is_focused(window)
                || input.panel_focus.contains_focused(window, cx)
        })
    }
    fn repaint_text_editor(window: &mut Window, cx: &mut Context<Self>) {
        // GPUI refresh is intentionally a no-op while drawing, and focus-out
        // observers can run in that phase. One weak post-frame notification
        // avoids leaving a closed panel painted until the next pointer event.
        window.refresh();
        let view = cx.weak_entity();
        window.on_next_frame(move |window, cx| {
            let _ = view.update(cx, |_, cx| cx.notify());
            window.refresh();
        });
    }
    fn close_text_editor(
        &mut self,
        cancel: bool,
        reason: ToolCloseReason,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.reconcile_text_editor(window, cx);
        let Some(input) = self.text_input.take() else {
            return false;
        };
        let focused = input.state.read(cx).focus_handle(cx).is_focused(window)
            || input.panel_focus.contains_focused(window, cx);
        // Explicit policy for closing mid-composition: commit retains latest
        // visible preedit; cancel restores the session baseline. Do not ask a
        // dropped platform callback to complete a replacement session.
        let value = input.state.update(cx, |state, cx| {
            state.unmark_text(window, cx);
            state.value().to_string()
        });
        let before = self.editor.engine.revision();
        let result = if let Some(tools) = &mut self.tools {
            if cancel {
                tools.cancel(&mut self.editor, &mut self.interactions, input.token)
            } else {
                let flush = if value == input.last_submitted.as_ref() {
                    Ok(())
                } else {
                    tools.set_text(&mut self.editor, &mut self.interactions, input.token, value)
                };
                flush.and_then(|()| {
                    tools.finish_with_reason(
                        &mut self.editor,
                        &mut self.interactions,
                        input.token,
                        reason,
                    )
                })
            }
        } else {
            Ok(false)
        };
        let closed = match result {
            Ok(closed) => {
                self.interaction_error = None;
                closed
            }
            Err(error) => {
                self.interaction_error = Some(error.to_string());
                if self
                    .tools
                    .as_ref()
                    .and_then(SceneToolController::active_token)
                    == Some(input.token)
                {
                    self.text_input = Some(input);
                }
                false
            }
        };
        if focused && self.text_input.is_none() {
            window.focus(&self.focus, cx);
        }
        self.emit_text_tool_changes(before, cx);
        Self::repaint_text_editor(window, cx);
        closed
    }
    pub(super) fn reconcile_text_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let before = self.editor.engine.revision();
        if let Some(tools) = &mut self.tools {
            tools.reconcile(&mut self.editor, &mut self.interactions);
        }
        let active = self
            .tools
            .as_ref()
            .and_then(SceneToolController::active_token);
        if self
            .text_input
            .as_ref()
            .is_some_and(|input| Some(input.token) != active)
            && let Some(input) = self.text_input.take()
        {
            let focused = input.state.read(cx).focus_handle(cx).is_focused(window)
                || input.panel_focus.contains_focused(window, cx);
            input
                .state
                .update(cx, |state, cx| state.unmark_text(window, cx));
            if focused {
                window.focus(&self.focus, cx);
            }
        }
        // Avoid requesting another frame from every render if nothing changed.
        if before != self.editor.engine.revision() {
            cx.emit(SceneViewEvent::Changed);
        }
        if let Some(tools) = &mut self.tools {
            for event in tools.drain_events() {
                cx.emit(SceneViewEvent::EditorTool(event));
            }
        }
    }
    fn emit_text_tool_changes(&mut self, before: u64, cx: &mut Context<Self>) {
        if before != self.editor.engine.revision() {
            cx.emit(SceneViewEvent::Changed);
        }
        if let Some(tools) = &mut self.tools {
            for event in tools.drain_events() {
                cx.emit(SceneViewEvent::EditorTool(event));
            }
        }
        cx.notify();
    }
    pub(super) fn text_editor_panel(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let input = self.text_input.as_ref()?;
        let target = self
            .text_editor_target()
            .and_then(|h| self.editor.engine.object(h).ok())?;
        let theme = self.theme;
        Some(
            div()
                .id("scene-text-content-editor")
                .track_focus(&input.panel_focus)
                .absolute()
                .top_3()
                .right_3()
                .w(px(340.))
                .max_w_full()
                .flex()
                .flex_col()
                .gap_3()
                .p_3()
                .rounded_md()
                .bg(rgb(theme.panel))
                .border_1()
                .border_color(rgb(theme.border))
                .text_color(rgb(theme.foreground))
                .child(
                    div()
                        .text_sm()
                        .child(format!("Text content · {}", target.id)),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(theme.muted))
                        .child(if input.normalized {
                            "Host normalized the saved text; canvas shows the result"
                        } else {
                            "Content editing; canvas layout stays visible"
                        }),
                )
                .child(
                    Textarea::new(&input.state)
                        .h(px(180.))
                        .w_full()
                        // Use the same semantic palette for both sides of the
                        // contrast pair; the host canvas may have a different theme.
                        .bg(cx.theme().background)
                        .text_color(cx.theme().foreground)
                        .aria_label("Scene text content"),
                )
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            crate::ControlButton::new("text-done", "Done")
                                .theme(theme)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.finish_text_editor(window, cx);
                                })),
                        )
                        .child(
                            crate::ControlButton::new("text-cancel", "Cancel")
                                .theme(theme)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.cancel_text_editor(window, cx);
                                })),
                        ),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(theme.muted))
                        .child("Escape cancels · Tab moves focus · Use clipboard shortcuts"),
                )
                .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
                .on_mouse_move(|_, _, cx| cx.stop_propagation())
                .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                // Kit dispatches its Tab key binding as an action before raw
                // keys. Capture the public actions, not just KeyDown, so the
                // content panel consistently uses native focus traversal.
                .capture_action(|_: &IndentInline, window, cx| {
                    window.focus_next(cx);
                    cx.stop_propagation();
                })
                .capture_action(|_: &OutdentInline, window, cx| {
                    window.focus_prev(cx);
                    cx.stop_propagation();
                })
                .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    if event.keystroke.key.as_str() == "escape" {
                        this.cancel_text_editor(window, cx);
                        cx.stop_propagation();
                    } else if event.keystroke.key.as_str() == "tab"
                        && !event.keystroke.modifiers.control
                        && !event.keystroke.modifiers.alt
                        && !event.keystroke.modifiers.platform
                    {
                        // This is a form-like content panel. Reuse GPUI focus
                        // traversal instead of Kit's default Tab indentation.
                        if event.keystroke.modifiers.shift {
                            window.focus_prev(cx);
                        } else {
                            window.focus_next(cx);
                        }
                        cx.stop_propagation();
                    }
                }))
                // Never consume ordinary key bubbling here: GPUI delivers
                // raw text after propagation. SceneView's keyboard entry points
                // explicitly yield while an inner session owns input.
                .into_any_element(),
        )
    }
}
