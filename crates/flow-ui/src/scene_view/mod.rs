//! Retained-scene canvas, independent of node graphs, media runners and Studio chrome.
mod interaction;
mod plugins;
mod render;
use crate::{
    FlowTheme,
    display::{RenderIssue, RenderObject},
};
use gpui::{prelude::*, *};
use gpui_flow_core::{Point as WorldPoint, Size as WorldSize};
use gpui_flow_editor::SceneEditor;
use gpui_flow_scene::{DisplayId, EventKind, HitTestOptions, Scene, SceneError, SceneEvent};

pub enum SceneViewEvent {
    Changed,
    SelectionChanged,
    Pointer(Box<SceneEvent>),
}
impl EventEmitter<SceneViewEvent> for SceneView {}
/// Embed this view in any host shell. Register scoped element listeners with `on`; the engine owns their lifecycle.
/// Actual content lives in `editor.scene()`; no demo-specific UI or provider runs occur here.
pub struct SceneView {
    pub editor: SceneEditor,
    pub theme: FlowTheme,
    pub show_grid: bool,
    pub interactions: gpui_flow_interaction::scene::ScenePluginHost,
    pub diagnostics: Vec<RenderIssue>,
    pub gradient_diagnostics: Vec<crate::display::GradientRenderIssue>,
    pub renderers: crate::display::DisplayRegistry,
    /// Opt-in paint-only state presentation; never writes editor documents.
    pub state_styles: gpui_flow_interaction::scene::SceneStateStyles,
    pub state_style_error: Option<String>,
    state_clock: std::time::Instant,
    focus: FocusHandle,
    origin: WorldPoint,
    extent: WorldSize,
    subscriptions: Vec<Subscription>,
    interaction_window: Option<u64>,
    router: gpui_flow_interaction::scene::SceneEventRouter,
    pub interaction_error: Option<String>,
}
impl SceneView {
    pub fn new(scene: Scene, cx: &mut Context<Self>) -> Result<Self, SceneError> {
        Ok(Self::from_engine(
            gpui_flow_engine::SceneEngine::new(scene)?,
            cx,
        ))
    }
    pub fn from_engine(engine: gpui_flow_engine::SceneEngine, cx: &mut Context<Self>) -> Self {
        let state_styles = gpui_flow_interaction::scene::SceneStateStyles::new(&engine);
        Self {
            editor: SceneEditor::from_engine(engine),
            theme: FlowTheme::default(),
            show_grid: true,
            interactions: gpui_flow_interaction::scene::ScenePluginHost::with_defaults(),
            diagnostics: vec![],
            gradient_diagnostics: vec![],
            renderers: crate::display::DisplayRegistry::default(),
            state_styles,
            state_style_error: None,
            state_clock: std::time::Instant::now(),
            focus: cx.focus_handle(),
            origin: WorldPoint::ZERO,
            extent: WorldSize::new(1000., 700.),
            subscriptions: vec![],
            interaction_window: None,
            router: gpui_flow_interaction::scene::SceneEventRouter::new(),
            interaction_error: None,
        }
    }
    pub fn on(
        &mut self,
        id: impl Into<DisplayId>,
        kind: EventKind,
        options: gpui_flow_scene::ListenerOptions,
        handler: impl FnMut(&mut SceneEvent) + 'static,
    ) -> Result<gpui_flow_engine::ListenerId, gpui_flow_engine::EngineError> {
        let id = id.into();
        let handle = self
            .editor
            .engine
            .handle(&id)
            .ok_or(SceneError::MissingNode(id))?;
        self.editor.engine.on(handle, kind, options, handler)
    }
    /// Queue capability-checked commands from an element listener; commands run
    /// after propagation/default handling and retain their originating identity.
    pub fn action_sender(&self) -> gpui_flow_interaction::scene::SceneActionSender {
        self.router.action_sender()
    }
    pub fn off(&mut self, id: gpui_flow_engine::ListenerId) -> bool {
        self.editor.engine.off(id)
    }
    pub fn is_editable(&self) -> bool {
        self.interactions.policy().editable
    }
    pub fn fit(&mut self) {
        if let Err(error) = self.interactions.execute(
            &mut self.editor,
            gpui_flow_interaction::scene::SceneCommand::Fit {
                canvas: self.extent,
                padding: 50.,
            },
        ) {
            self.interaction_error = Some(error.to_string());
        }
    }
    pub fn set_editable(&mut self, editable: bool, cx: &mut Context<Self>) {
        self.cancel_input(cx);
        let report = self.interactions.set_editable(editable, &mut self.editor);
        self.apply_input_report(report, cx);
    }
    pub fn set_scene(
        &mut self,
        scene: Scene,
        cx: &mut Context<Self>,
    ) -> Result<(), gpui_flow_interaction::scene::SceneInteractionError> {
        self.cancel_input(cx);
        self.execute_command(
            gpui_flow_interaction::scene::SceneCommand::LoadScene(scene),
            cx,
        )?;
        Ok(())
    }
    fn local(&self, p: gpui::Point<Pixels>) -> WorldPoint {
        WorldPoint::new(p.x.into(), p.y.into()) - self.origin
    }
    fn hit(&self, p: WorldPoint) -> Option<DisplayId> {
        self.editor
            .engine
            .hit_test(
                p,
                HitTestOptions {
                    tolerance: 4. / self.editor.viewport.zoom,
                    include_locked: true,
                    ..Default::default()
                },
            )
            .ok()
            .flatten()
            .map(|hit| hit.id)
    }
    fn resize_target(&self) -> Option<DisplayId> {
        if !self.is_editable()
            || !self.interactions.is_enabled("resize")
            || self.editor.selection.ids.len() != 1
        {
            return None;
        }
        let id = self.editor.selection.ids.iter().next()?.clone();
        let mut current = Some(id.clone());
        while let Some(ancestor) = current {
            let node = self.editor.scene().get(&ancestor)?;
            if node.object.locked {
                return None;
            }
            current = node.parent.clone();
        }
        self.selection_corners(&id)?;
        Some(id)
    }
    fn selection_corners(&self, id: &DisplayId) -> Option<[WorldPoint; 4]> {
        let bounds = self.editor.engine.local_bounds(id).ok()??;
        let transform = self.editor.engine.world_transform(id).ok()?;
        Some(
            [
                bounds.origin,
                WorldPoint::new(bounds.max_x(), bounds.min_y()),
                WorldPoint::new(bounds.max_x(), bounds.max_y()),
                WorldPoint::new(bounds.min_x(), bounds.max_y()),
            ]
            .map(|p| {
                self.editor
                    .viewport
                    .world_to_screen(transform.transform_point(p))
            }),
        )
    }
}
impl Focusable for SceneView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
