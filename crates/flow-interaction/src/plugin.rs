use crate::*;
use gpui_flow_core::{ConnectionDrag, Editor};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginLifecycle {
    Installed,
    Enabled,
    Disabled,
    Cancelled,
    Uninstalled,
}
/// Host-declared rendering support used by transform tools, separate from Edit permission.
/// Default preserves native-only Text constraints. This runtime value is not document data.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TransformCapabilities {
    pub affine_text: bool,
}
/// The editor cannot be mutated through a plugin context. All changes must be
/// expressed as commands and pass host policy after every middleware hook.
pub struct PluginContext<'a, D = Editor> {
    pub editor: &'a D,
    pub policy: AccessPolicy,
    pub transforms: TransformCapabilities,
    pub connection: &'a ConnectionDrag,
    pub capture_owner: Option<&'a str>,
}
impl<D> PluginContext<'_, D> {
    pub fn allows(&self, capability: Capability) -> bool {
        self.policy.allows(capability)
    }
}
#[derive(Debug)]
pub struct PluginOutput<C = CanvasCommand> {
    pub commands: Vec<C>,
    pub effects: Vec<CanvasEffect>,
    pub stop_propagation: bool,
    pub prevent_default: bool,
}
impl<C> Default for PluginOutput<C> {
    fn default() -> Self {
        Self {
            commands: vec![],
            effects: vec![],
            stop_propagation: false,
            prevent_default: false,
        }
    }
}
impl<C> PluginOutput<C> {
    pub fn command(&mut self, command: C) {
        self.commands.push(command);
    }
    pub fn effect(&mut self, effect: CanvasEffect) {
        self.effects.push(effect);
    }
    pub fn stop_propagation(&mut self) {
        self.stop_propagation = true;
    }
    pub fn prevent_default(&mut self) {
        self.prevent_default = true;
    }
}
/// Ordered reusable behavior. Lower priority runs first. Default-action plugins
/// are skipped after prevent_default; custom observers can still receive events.
/// Command middleware runs in the same order, then the host validates/gates.
pub trait CanvasPlugin<D = Editor, C = CanvasCommand> {
    fn id(&self) -> &str;
    fn priority(&self) -> i32 {
        0
    }
    fn is_default_action(&self) -> bool {
        false
    }
    /// Optional retained-scene geometry overlay, shared by picking and input.
    fn geometry_tool_mode(&self) -> Option<crate::scene::GeometryToolMode> {
        None
    }
    /// Optional sizing-box policy for the scene resize plugin. The host uses
    /// the same result for painted handles, picking and input. None hides an
    /// unsupported target; graph plugins retain their existing behavior.
    fn resize_bounds(
        &self,
        _editor: &D,
        _id: &str,
        paint_bounds: gpui_flow_core::Rect,
    ) -> Option<gpui_flow_core::Rect> {
        Some(paint_bounds)
    }
    fn on_lifecycle(&mut self, _event: PluginLifecycle) {}
    fn on_event(
        &mut self,
        _event: &CanvasEvent,
        _context: &PluginContext<'_, D>,
        _output: &mut PluginOutput<C>,
    ) {
    }
    /// Clock-only synchronization before input, without advancing navigation.
    fn on_time(&mut self, _now: std::time::Duration) {}
    /// Explicit monotonic application time. Default hooks keep existing plugins
    /// inert; no timer or OS clock is created by the plugin host.
    fn on_frame(
        &mut self,
        _now: std::time::Duration,
        _context: &PluginContext<'_, D>,
        _output: &mut PluginOutput<C>,
    ) {
    }
    fn needs_frame(&self) -> bool {
        false
    }
    fn filter_command(
        &mut self,
        _command: &mut C,
        _context: &PluginContext<'_, D>,
        _effects: &mut Vec<CanvasEffect>,
    ) -> Result<(), String> {
        Ok(())
    }
}
