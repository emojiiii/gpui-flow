//! Opt-in, read-only snapshots of the native adapter's actual router operations.
use super::*;
use gpui_flow_interaction::{CanvasEvent, DispatchReport, scene::RoutedReport};

/// The exact normalized input accepted by the adapter, or a host focus operation.
/// IDs are document IDs, so another independent engine can replay the operation.
#[derive(Debug, Clone, PartialEq)]
pub enum SceneInputOperation {
    Canvas(CanvasEvent),
    /// A real native animation callback, separately replayable from input time.
    NavigationFrame {
        now: std::time::Duration,
    },
    /// Wheel target picked from the current rendered geometry/source alpha.
    /// Explicit None is background and must not fall back to canonical picking.
    PickedWheel {
        position: WorldPoint,
        delta: WorldPoint,
        modifiers: gpui_flow_interaction::Modifiers,
        target: Option<DisplayId>,
    },
    Focus(DisplayId),
    ClearFocus,
    /// GPUI declined native focus, so no logical input operation was performed.
    FocusDeclined(DisplayId),
}

/// One completed operation, emitted only when tracing is explicitly enabled.
/// This stores neither callbacks nor an internal history. Consumers own retention
/// and should bound it. Events retain their cancellation and related-target data.
#[derive(Debug, Clone)]
pub struct SceneInputTrace {
    pub operation: SceneInputOperation,
    /// Last accepted explicit navigation clock; input sync never advances motion.
    pub navigation_time: std::time::Duration,
    pub events: Vec<SceneEvent>,
    pub default: DispatchReport,
    pub errors: Vec<String>,
    pub focus_requested: bool,
    pub focused: Option<DisplayId>,
    pub space_held: bool,
    pub capture_owner: Option<String>,
    pub pressed: bool,
    pub editing: bool,
    pub selection: Vec<DisplayId>,
    pub viewport: gpui_flow_core::Viewport,
}
impl SceneInputTrace {
    /// Take an equivalent snapshot from an independent pure router for replay
    /// tests. Runtime handles are deliberately not compared across engines.
    pub fn from_report(
        operation: SceneInputOperation,
        report: &RoutedReport,
        editor: &SceneEditor,
        host: &gpui_flow_interaction::scene::ScenePluginHost,
        router: &gpui_flow_interaction::scene::SceneEventRouter,
    ) -> Self {
        Self {
            operation,
            navigation_time: host.navigation_time(),
            events: report.events.clone(),
            default: report.default.clone(),
            errors: report.errors.clone(),
            focus_requested: report.focus_requested,
            focused: router
                .focused(&editor.engine)
                .and_then(|handle| editor.engine.instance(handle).ok())
                .map(|instance| instance.id),
            space_held: router.space_held(),
            capture_owner: host.capture_owner().map(str::to_owned),
            pressed: router.has_pressed(&editor.engine),
            editing: editor.editing(),
            selection: editor.selection.ids.iter().cloned().collect(),
            viewport: editor.viewport,
        }
    }
}
impl SceneView {
    /// Enable per-operation input snapshots for diagnostics. Disabled by default;
    /// this does not alter routing, native focus, propagation or default actions.
    pub fn set_input_tracing(&mut self, enabled: bool) {
        self.input_tracing = enabled;
    }

    pub(super) fn emit_input_trace(
        &self,
        operation: SceneInputOperation,
        report: &RoutedReport,
        cx: &mut Context<Self>,
    ) {
        if self.input_tracing {
            cx.emit(SceneViewEvent::InputTrace(Box::new(
                SceneInputTrace::from_report(
                    operation,
                    report,
                    &self.editor,
                    &self.interactions,
                    &self.router,
                ),
            )));
        }
    }
}
