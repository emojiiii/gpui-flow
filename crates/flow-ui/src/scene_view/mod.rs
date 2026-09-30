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
use gpui_flow_scene::{
    DisplayId, EventDispatcher, EventKind, HitTestOptions, Scene, SceneError, SceneEvent,
};

pub enum SceneViewEvent {
    Changed,
    SelectionChanged,
    Pointer(SceneEvent),
}
impl EventEmitter<SceneViewEvent> for SceneView {}
/// Embed this view in any host shell. Attach core event listeners to `dispatcher`.
/// Actual content lives in `editor.scene`; no demo-specific UI or provider runs occur here.
pub struct SceneView {
    pub editor: SceneEditor,
    pub theme: FlowTheme,
    pub dispatcher: EventDispatcher,
    pub show_grid: bool,
    pub interactions: gpui_flow_interaction::scene::ScenePluginHost,
    pub diagnostics: Vec<RenderIssue>,
    pub gradient_diagnostics: Vec<crate::display::GradientRenderIssue>,
    pub renderers: crate::display::DisplayRegistry,
    focus: FocusHandle,
    origin: WorldPoint,
    extent: WorldSize,
    subscriptions: Vec<Subscription>,
    interaction_window: Option<u64>,
    space_held: bool,
    last_pointer: WorldPoint,
    pub interaction_error: Option<String>,
    pressed: Option<(DisplayId, WorldPoint)>,
}
impl SceneView {
    pub fn new(scene: Scene, cx: &mut Context<Self>) -> Result<Self, SceneError> {
        Ok(Self {
            editor: SceneEditor::new(scene)?,
            theme: FlowTheme::default(),
            dispatcher: EventDispatcher::default(),
            show_grid: true,
            interactions: gpui_flow_interaction::scene::ScenePluginHost::with_defaults(),
            diagnostics: vec![],
            gradient_diagnostics: vec![],
            renderers: crate::display::DisplayRegistry::default(),
            focus: cx.focus_handle(),
            origin: WorldPoint::ZERO,
            extent: WorldSize::new(1000., 700.),
            subscriptions: vec![],
            interaction_window: None,
            space_held: false,
            last_pointer: WorldPoint::ZERO,
            interaction_error: None,
            pressed: None,
        })
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
        self.dispatcher = EventDispatcher::default();
        Ok(())
    }
    fn local(&self, p: gpui::Point<Pixels>) -> WorldPoint {
        WorldPoint::new(p.x.into(), p.y.into()) - self.origin
    }
    fn world(&self, p: gpui::Point<Pixels>) -> WorldPoint {
        self.editor.viewport.screen_to_world(self.local(p))
    }
    fn hit(&self, p: WorldPoint) -> Option<DisplayId> {
        self.editor
            .scene
            .hit_test(
                p,
                HitTestOptions {
                    tolerance: 4. / self.editor.viewport.zoom,
                    ..Default::default()
                },
            )
            .ok()
            .flatten()
            .map(|hit| hit.id)
    }
    fn dispatch(
        &mut self,
        kind: EventKind,
        target: DisplayId,
        p: WorldPoint,
        cx: &mut Context<Self>,
    ) -> bool {
        match self
            .dispatcher
            .dispatch(&self.editor.scene, SceneEvent::new(kind, target).at(p))
        {
            Ok(result) => {
                let prevented = result.event.default_prevented();
                cx.emit(SceneViewEvent::Pointer(result.event));
                prevented
            }
            Err(_) => false,
        }
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
            let node = self.editor.scene.get(&ancestor)?;
            if node.object.locked {
                return None;
            }
            current = node.parent.clone();
        }
        self.selection_corners(&id)?;
        Some(id)
    }
    fn selection_corners(&self, id: &DisplayId) -> Option<[WorldPoint; 4]> {
        let bounds = self.editor.scene.local_bounds(id).ok()??;
        let transform = self.editor.scene.world_transform(id).ok()?;
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
