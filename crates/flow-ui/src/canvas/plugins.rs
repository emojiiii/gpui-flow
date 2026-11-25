//! Host lifecycle and the single policy-checked command entrypoint used by canvas chrome.
use super::*;
use gpui_flow_interaction::{
    AccessPolicy, CanvasCommand, CanvasPlugin, Capability, InteractionError,
};
impl FlowCanvas {
    pub fn is_editable(&self) -> bool {
        self.interactions.allows(Capability::Edit)
    }
    pub fn set_editable(&mut self, editable: bool, cx: &mut Context<Self>) {
        let report = self.interactions.set_editable(editable, &mut self.editor);
        self.apply_interaction_report(report, cx);
        cx.notify();
    }
    pub fn set_access_policy(&mut self, policy: AccessPolicy, cx: &mut Context<Self>) {
        let report = self.interactions.set_policy(policy, &mut self.editor);
        self.apply_interaction_report(report, cx);
        cx.notify();
    }
    pub fn install_plugin(
        &mut self,
        plugin: impl CanvasPlugin + 'static,
        cx: &mut Context<Self>,
    ) -> Result<(), InteractionError> {
        self.cancel_interaction(cx);
        self.interactions.install(plugin)?;
        cx.notify();
        Ok(())
    }
    pub fn replace_plugin(
        &mut self,
        plugin: impl CanvasPlugin + 'static,
        cx: &mut Context<Self>,
    ) -> Result<(), InteractionError> {
        let report = self.interactions.replace(plugin, &mut self.editor)?;
        self.apply_interaction_report(report, cx);
        cx.notify();
        Ok(())
    }
    pub fn uninstall_plugin(
        &mut self,
        id: &str,
        cx: &mut Context<Self>,
    ) -> Result<(), InteractionError> {
        let report = self.interactions.uninstall(id, &mut self.editor)?;
        self.apply_interaction_report(report, cx);
        cx.notify();
        Ok(())
    }
    pub fn set_plugin_enabled(
        &mut self,
        id: &str,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> Result<(), InteractionError> {
        let report = self
            .interactions
            .set_enabled(id, enabled, &mut self.editor)?;
        self.apply_interaction_report(report, cx);
        cx.notify();
        Ok(())
    }
    /// UI edits, custom toolbar actions and import operations share this policy gate.
    /// Direct mutation of the public editor is a trusted host escape hatch, not a sandbox.
    pub fn execute_command(
        &mut self,
        command: CanvasCommand,
        cx: &mut Context<Self>,
    ) -> Result<(), InteractionError> {
        self.execute_checked_command(command, |_| Ok(()), cx)
    }
    pub(super) fn execute_checked_command<T>(
        &mut self,
        command: CanvasCommand,
        inspect: impl FnOnce(&CanvasCommand) -> Result<T, InteractionError>,
        cx: &mut Context<Self>,
    ) -> Result<T, InteractionError> {
        match self
            .interactions
            .execute_checked(&mut self.editor, command, inspect)
        {
            Ok((result, report)) => {
                self.apply_interaction_report(report, cx);
                Ok(result)
            }
            Err(error) => {
                self.report_interaction_rejection(&error, cx);
                cx.notify();
                Err(error)
            }
        }
    }
    pub(super) fn subscribe_interaction_lifecycle(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = window.window_handle().window_id().as_u64();
        if self.interaction_window == Some(id) {
            return;
        }
        self.interaction_subscriptions.clear();
        self.interaction_window = Some(id);
        self.interaction_subscriptions.push(cx.on_focus_out(
            &self.focus.clone(),
            window,
            |this, _, _, cx| this.focus_lost(cx),
        ));
        self.interaction_subscriptions
            .push(cx.observe_window_activation(window, |this, window, cx| {
                if !window.is_window_active() {
                    this.focus_lost(cx);
                }
            }));
    }
}
