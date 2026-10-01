//! Retained-scene canvas, independent of node graphs, media runners and Studio chrome.
mod composite;
mod interaction;
mod plugins;
mod render;
mod text_edit;
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
    Resource(gpui_flow_engine::ResourceEvent),
    ResourceView(gpui_flow_engine::ResourceViewEvent),
    EditorTool(gpui_flow_interaction::scene::SceneToolEvent),
}
impl EventEmitter<SceneViewEvent> for SceneView {}
/// Which pixels are owned by the CPU compositing pass.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SceneCompositing {
    /// Keep independent isolation domains and native siblings (the default).
    #[default]
    Domains,
    /// Include this view's opaque background, grid and complete presented
    /// document in one bounded domain. Does not capture host/native backdrops.
    OwnedCanvas,
}
/// Embed this view in any host shell. Register scoped element listeners with `on`; the engine owns their lifecycle.
/// Actual content lives in `editor.scene()`; no demo-specific UI or provider runs occur here.
pub struct SceneView {
    pub editor: SceneEditor,
    pub theme: FlowTheme,
    pub show_grid: bool,
    pub compositing_mode: SceneCompositing,
    pub interactions: gpui_flow_interaction::scene::ScenePluginHost,
    pub diagnostics: Vec<RenderIssue>,
    pub gradient_diagnostics: Vec<crate::display::GradientRenderIssue>,
    /// CPU-domain rejection, upload failures and explicit stale-frame notices.
    pub composite_diagnostics: Vec<gpui_flow_composite::CompositeIssue>,
    pub compositing_pending: bool,
    pub(crate) compositing: crate::display::composite::CompositeController,
    composite_geometry: Option<gpui_flow_composite::RasterViewport>,
    text_rasterizer: Option<std::sync::Arc<gpui_flow_text::GpuiTextRasterizer>>,
    resources: Option<crate::resources::SceneResourceAdapter>,
    pub resource_error: Option<String>,
    pub renderers: crate::display::DisplayRegistry,
    /// Opt-in paint-only state presentation; never writes editor documents.
    pub state_styles: gpui_flow_interaction::scene::SceneStateStyles,
    pub state_style_error: Option<String>,
    state_clock: std::time::Instant,
    focus: FocusHandle,
    pending_native_focus: bool,
    origin: WorldPoint,
    extent: WorldSize,
    subscriptions: Vec<Subscription>,
    interaction_window: Option<u64>,
    router: gpui_flow_interaction::scene::SceneEventRouter,
    pub interaction_error: Option<String>,
    tools: Option<gpui_flow_interaction::scene::SceneToolController>,
    text_input: Option<text_edit::TextInputSession>,
}
impl SceneView {
    pub fn new(scene: Scene, cx: &mut Context<Self>) -> Result<Self, SceneError> {
        Ok(Self::from_engine(
            gpui_flow_engine::SceneEngine::new(scene)?,
            cx,
        ))
    }
    pub fn from_engine(mut engine: gpui_flow_engine::SceneEngine, cx: &mut Context<Self>) -> Self {
        let state_styles = gpui_flow_interaction::scene::SceneStateStyles::new(&engine);
        let (resources, resource_error) =
            match crate::resources::SceneResourceAdapter::new(&mut engine) {
                Ok(resources) => (Some(resources), None),
                Err(error) => (None, Some(error.to_string())),
            };
        cx.on_release(|this: &mut Self, cx| {
            if let Some(tools) = &mut this.tools {
                let _ = tools.dispose(&mut this.editor, &mut this.interactions);
            }
            this.text_input = None;
            this.compositing.dispose(cx);
            if let Some(resources) = &mut this.resources {
                let _ = resources.dispose(&mut this.editor.engine);
            }
        })
        .detach();
        Self {
            editor: SceneEditor::from_engine(engine),
            theme: FlowTheme::default(),
            show_grid: true,
            compositing_mode: SceneCompositing::Domains,
            interactions: gpui_flow_interaction::scene::ScenePluginHost::with_defaults(),
            diagnostics: vec![],
            gradient_diagnostics: vec![],
            composite_diagnostics: vec![],
            compositing_pending: false,
            compositing: Default::default(),
            composite_geometry: None,
            text_rasterizer: None,
            resources,
            resource_error,
            renderers: crate::display::DisplayRegistry::default(),
            state_styles,
            state_style_error: None,
            state_clock: std::time::Instant::now(),
            focus: cx.focus_handle(),
            pending_native_focus: false,
            origin: WorldPoint::ZERO,
            extent: WorldSize::new(1000., 700.),
            subscriptions: vec![],
            interaction_window: None,
            router: gpui_flow_interaction::scene::SceneEventRouter::new(),
            interaction_error: None,
            tools: None,
            text_input: None,
        }
    }
    pub fn resources(&self) -> Option<&crate::resources::SceneResourceAdapter> {
        self.resources.as_ref()
    }
    /// Explicit host authorization for image sources. No scene import may set it.
    pub fn set_image_resolver(
        &mut self,
        resolver: crate::resources::SceneImageResolver,
        cx: &mut Context<Self>,
    ) -> Result<(), gpui_flow_engine::ResourceError> {
        self.resources
            .as_mut()
            .ok_or(gpui_flow_engine::ResourceError::Disposed)?
            .set_image_resolver(&self.editor.engine, resolver)?;
        cx.notify();
        Ok(())
    }
    pub fn set_image_cache(
        &mut self,
        cache: Option<gpui::AnyImageCache>,
        cx: &mut Context<Self>,
    ) -> Result<(), gpui_flow_engine::ResourceError> {
        self.resources
            .as_mut()
            .ok_or(gpui_flow_engine::ResourceError::Disposed)?
            .set_image_cache(&self.editor.engine, cache)?;
        cx.notify();
        Ok(())
    }
    /// Pause/resume this loaded image instance on both native and composited
    /// routes. Returns false for a stale handle or a not-yet-loaded generation.
    /// Runtime state is not serialized or linked to the editor animation timeline.
    pub fn set_image_playing(
        &mut self,
        handle: gpui_flow_engine::ElementHandle,
        playing: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let changed = self.resources.as_mut().is_some_and(|r| {
            r.set_image_playing(
                &self.editor.engine,
                handle,
                self.state_clock.elapsed(),
                playing,
            )
        });
        if changed {
            cx.notify();
        }
        changed
    }
    /// Seek to elapsed time within an infinitely looping decoded animation.
    /// The new playback generation rejects old CPU jobs and displayed snapshots.
    pub fn seek_image(
        &mut self,
        handle: gpui_flow_engine::ElementHandle,
        time: std::time::Duration,
        cx: &mut Context<Self>,
    ) -> bool {
        let changed = self
            .resources
            .as_mut()
            .is_some_and(|r| r.seek_image(&self.editor.engine, handle, time));
        if changed {
            cx.notify();
        }
        changed
    }
    /// Opt into CPU text using the bridge captured while constructing this App.
    /// A bridge from another App is rejected before any FontId can cross backends.
    /// Passing None removes the capability and invalidates all text-domain jobs.
    pub fn set_text_rasterizer(
        &mut self,
        bridge: Option<std::sync::Arc<gpui_flow_text::GpuiTextRasterizer>>,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if bridge
            .as_ref()
            .is_some_and(|b| !b.matches_text_system(cx.text_system()))
        {
            return Err("text rasterizer belongs to a different GPUI Application".into());
        }
        self.text_rasterizer = bridge;
        cx.notify();
        Ok(())
    }
    /// Call after the host changes its installed fonts/fallback resources.
    pub fn invalidate_text_resources(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Result<(), gpui_flow_engine::ResourceError> {
        if let Some(bridge) = &self.text_rasterizer {
            bridge.invalidate();
        }
        self.resources
            .as_mut()
            .ok_or(gpui_flow_engine::ResourceError::Disposed)?
            .invalidate_text(&self.editor.engine)?;
        cx.notify();
        Ok(())
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
        if self.text_editor_active()
            || !self.is_editable()
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
