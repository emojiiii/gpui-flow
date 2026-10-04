//! The retained-scene adapter uses the same event/lifecycle/capability plugin protocol.
use super::*;
use gpui_flow_interaction::scene::{SceneCommand, SceneDispatchReport, SceneInteractionError};
use gpui_flow_interaction::{AccessPolicy, CanvasPlugin};
impl SceneView {
    pub fn execute_command(
        &mut self,
        command: SceneCommand,
        cx: &mut Context<Self>,
    ) -> Result<(), SceneInteractionError> {
        match self.interactions.execute(&mut self.editor, command) {
            Ok(report) => {
                self.apply_input_report(report, cx);
                Ok(())
            }
            Err(error) => {
                self.interaction_error = Some(error.to_string());
                cx.notify();
                Err(error)
            }
        }
    }
    pub fn set_access_policy(&mut self, policy: AccessPolicy, cx: &mut Context<Self>) {
        self.cancel_input(cx);
        let report = self.interactions.set_policy(policy, &mut self.editor);
        self.apply_input_report(report, cx);
    }
    pub fn install_plugin(
        &mut self,
        plugin: impl CanvasPlugin<SceneEditor, SceneCommand> + 'static,
        cx: &mut Context<Self>,
    ) -> Result<(), SceneInteractionError> {
        self.cancel_input(cx);
        self.interactions.install(plugin)?;
        cx.notify();
        Ok(())
    }
    pub fn replace_plugin(
        &mut self,
        plugin: impl CanvasPlugin<SceneEditor, SceneCommand> + 'static,
        cx: &mut Context<Self>,
    ) -> Result<(), SceneInteractionError> {
        self.cancel_input(cx);
        let report = self.interactions.replace(plugin, &mut self.editor)?;
        self.apply_input_report(report, cx);
        Ok(())
    }
    pub fn uninstall_plugin(
        &mut self,
        id: &str,
        cx: &mut Context<Self>,
    ) -> Result<(), SceneInteractionError> {
        self.cancel_input(cx);
        let report = self.interactions.uninstall(id, &mut self.editor)?;
        self.apply_input_report(report, cx);
        Ok(())
    }
    pub fn set_plugin_enabled(
        &mut self,
        id: &str,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> Result<(), SceneInteractionError> {
        self.cancel_input(cx);
        let report = self
            .interactions
            .set_enabled(id, enabled, &mut self.editor)?;
        self.apply_input_report(report, cx);
        Ok(())
    }
    pub(super) fn apply_input_report(
        &mut self,
        report: SceneDispatchReport,
        cx: &mut Context<Self>,
    ) {
        for effect in &report.effects {
            match effect {
                gpui_flow_interaction::CanvasEffect::RequestFit => self.fit(),
                gpui_flow_interaction::CanvasEffect::ToggleGrid => self.show_grid = !self.show_grid,
                _ => {}
            }
        }
        if report.document_changed {
            cx.emit(SceneViewEvent::Changed);
        }
        if report.selection_changed {
            cx.emit(SceneViewEvent::SelectionChanged);
        }
        if let Some(error) = report.errors.last() {
            self.interaction_error = Some(error.to_string());
        }
        cx.notify();
    }
    pub(super) fn subscribe_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = window.window_handle().window_id().as_u64();
        if self.interaction_window == Some(id) {
            return;
        }
        self.interaction_window = Some(id);
        self.subscriptions.clear();
        self.subscriptions.push(
            cx.on_focus(&self.focus.clone(), window, |this, window, cx| {
                this.semantic_enter(window, cx);
            }),
        );
        self.subscriptions.push(cx.on_focus_out(
            &self.focus.clone(),
            window,
            |this, _, window, cx| {
                // A host can return native focus before an older focus-out
                // notification is delivered; do not clear that new owner.
                if !this.focus.is_focused(window) {
                    this.focus_lost(cx);
                }
            },
        ));
        self.subscriptions
            .push(cx.observe_window_activation(window, |this, window, cx| {
                if !window.is_window_active() {
                    this.finish_text_editor(window, cx);
                    this.finish_inner_tool(window, cx);
                    if let Some(resources) = &mut this.resources {
                        resources.freeze_images(this.state_clock.elapsed());
                    }
                    this.focus_lost(cx);
                }
                cx.notify();
            }));
        self.subscriptions
            .push(cx.observe_window_visibility(window, |this, _, window, cx| {
                if !window.is_visible() {
                    this.finish_text_editor(window, cx);
                    this.finish_inner_tool(window, cx);
                    if let Some(resources) = &mut this.resources {
                        resources.freeze_images(this.state_clock.elapsed());
                    }
                }
                cx.notify();
            }));
    }
}
