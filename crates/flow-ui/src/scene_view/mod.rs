//! Retained-scene canvas, independent of node graphs, media runners and Studio chrome.
mod canvas_commands;
mod composite;
pub(crate) mod export;
mod flow_layout;
pub use flow_layout::FlowFrameEvent;
mod input_trace;
mod interaction;
mod layer_host;
pub use input_trace::*;
mod custom_resource;
mod custom_tool;
mod geometry_tools;
mod image_crop;
pub(crate) mod image_hit;
mod navigation;
mod plugins;
mod presentation;
mod render;
mod text_edit;
use crate::{
    FlowTheme,
    display::{RenderIssue, RenderObject},
};
pub use custom_tool::{SceneToolWidget, ToolWidgetContext};
use gpui::{prelude::*, *};
use gpui_flow_core::{Point as WorldPoint, Size as WorldSize};
use gpui_flow_editor::SceneEditor;
use gpui_flow_scene::{DisplayId, EventKind, HitTestOptions, Scene, SceneError, SceneEvent};

pub enum SceneViewEvent {
    Changed,
    /// Result of one opt-in real-frame layout batch.
    FlowFrame(FlowFrameEvent),
    SelectionChanged,
    Pointer(Box<SceneEvent>),
    /// Opt-in completed input snapshot; no history is retained by the view.
    InputTrace(Box<SceneInputTrace>),
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
    presentations: presentation::Presentations,
    composite_geometry: Option<gpui_flow_composite::RasterViewport>,
    text_rasterizer: Option<std::sync::Arc<gpui_flow_text::GpuiTextRasterizer>>,
    custom_rasterizers: crate::display::composite_custom::CustomRasterizers,
    resources: Option<crate::resources::SceneResourceAdapter>,
    image_alpha_hits: image_hit::ImageHitPolicies,
    frame_layout: flow_layout::FrameLayoutState,
    navigation_frames: navigation::NavigationFrames,
    native_pointer: crate::native_pointer::NativePointerState,
    image_crop: Option<image_crop::ImageCropMode>,
    pub resource_error: Option<String>,
    pub renderers: crate::display::DisplayRegistry,
    /// Opt-in paint-only state presentation; never writes editor documents.
    pub state_styles: gpui_flow_interaction::scene::SceneStateStyles,
    pub state_style_error: Option<String>,
    state_clock: std::time::Instant,
    layer_host: layer_host::LayerHostState,
    focus: FocusHandle,
    pending_native_focus: bool,
    input_tracing: bool,
    origin: WorldPoint,
    extent: WorldSize,
    subscriptions: Vec<Subscription>,
    interaction_window: Option<u64>,
    router: gpui_flow_interaction::scene::SceneEventRouter,
    pub interaction_error: Option<String>,
    tools: Option<gpui_flow_interaction::scene::SceneToolController>,
    text_input: Option<text_edit::TextInputSession>,
    tool_widgets: custom_tool::ToolWidgets,
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
            this.dispose_tool_widgets(cx);
            if let Some(tools) = &mut this.tools {
                let _ = tools.dispose(&mut this.editor, &mut this.interactions);
            }
            this.text_input = None;
            this.navigation_frames.cancel();
            this.compositing.dispose(cx);
            this.presentations = Default::default();
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
            presentations: Default::default(),
            composite_geometry: None,
            text_rasterizer: None,
            custom_rasterizers: Default::default(),
            resources,
            image_alpha_hits: Default::default(),
            frame_layout: Default::default(),
            navigation_frames: Default::default(),
            native_pointer: Default::default(),
            image_crop: None,
            resource_error,
            renderers: crate::display::DisplayRegistry::default(),
            state_styles,
            state_style_error: None,
            state_clock: std::time::Instant::now(),
            layer_host: Default::default(),
            focus: cx.focus_handle(),
            pending_native_focus: false,
            input_tracing: false,
            origin: WorldPoint::ZERO,
            extent: WorldSize::new(1000., 700.),
            subscriptions: vec![],
            interaction_window: None,
            router: gpui_flow_interaction::scene::SceneEventRouter::new(),
            interaction_error: None,
            tools: None,
            text_input: None,
            tool_widgets: Default::default(),
        }
    }
    /// Mutate owned Canvas pixels and request a repaint; configuration continues
    /// to use ordinary editor transactions and Undo. Pixel bytes are not history.
    pub fn edit_canvas(
        &mut self,
        handle: gpui_flow_engine::ElementHandle,
        edit: impl FnOnce(
            &mut gpui_flow_composite::CanvasSurface,
        ) -> Result<(), gpui_flow_composite::CompositeError>,
        cx: &mut Context<Self>,
    ) -> Result<(), crate::resources::CanvasEditError> {
        self.edit_canvas_guarded(handle, edit, || Ok(()), cx)
    }
    fn edit_canvas_guarded(
        &mut self,
        handle: gpui_flow_engine::ElementHandle,
        edit: impl FnOnce(
            &mut gpui_flow_composite::CanvasSurface,
        ) -> Result<(), gpui_flow_composite::CompositeError>,
        validate: impl Fn() -> Result<(), gpui_flow_composite::CompositeError>,
        cx: &mut Context<Self>,
    ) -> Result<(), crate::resources::CanvasEditError> {
        let resources =
            self.resources
                .as_mut()
                .ok_or(crate::resources::CanvasEditError::Resource(
                    gpui_flow_engine::ResourceError::Disposed,
                ))?;
        for event in
            resources.edit_canvas_guarded(&mut self.editor.engine, handle, edit, validate)?
        {
            cx.emit(SceneViewEvent::Resource(event));
        }
        cx.notify();
        Ok(())
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
    pub fn set_owned_image_loader(
        &mut self,
        loader: Option<crate::resources::SceneOwnedImageLoader>,
        cx: &mut Context<Self>,
    ) -> Result<(), gpui_flow_engine::ResourceError> {
        self.resources
            .as_mut()
            .ok_or(gpui_flow_engine::ResourceError::Disposed)?
            .set_owned_image_loader(&self.editor.engine, loader)?;
        cx.notify();
        Ok(())
    }
    pub fn reload_owned_images(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Result<(), gpui_flow_engine::ResourceError> {
        self.resources
            .as_mut()
            .ok_or(gpui_flow_engine::ResourceError::Disposed)?
            .reload_owned_images(&self.editor.engine)?;
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
        let report = self.interactions.set_transform_capabilities(
            gpui_flow_interaction::TransformCapabilities {
                affine_text: bridge.is_some(),
            },
            &mut self.editor,
        );
        self.apply_input_report(report, cx);
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
        let options = HitTestOptions {
            tolerance: 4. / self.editor.viewport.zoom,
            include_locked: true,
            ..Default::default()
        };
        let geometry = self.presentation_hit_geometry()?;
        let hit = self.image_alpha_hits.hit_test_presented(
            &self.editor.engine,
            geometry,
            self.resources.as_ref(),
            p,
            options,
            self.presentation_hit_key(),
        );
        hit.ok().flatten().map(|hit| hit.id)
    }
    fn line_handles(
        &self,
    ) -> Option<(
        DisplayId,
        [gpui_flow_interaction::scene::SceneLineHandle; 2],
    )> {
        if self.geometry_tool().is_some()
            || self.inner_tool_active()
            || self.image_crop_editor_active()
            || !self.is_editable()
            || !self.interactions.is_enabled("line-endpoints")
            || self.tools.as_ref().is_some_and(|tools| {
                !tools.outer_tool_active(gpui_flow_interaction::scene::DEFAULT_OUTER_TOOL)
            })
        {
            return None;
        }
        presentation::line_handles_for_presentation(&self.editor, self.presentation_hit_geometry())
    }
    fn resize_handles(
        &self,
    ) -> Option<(
        Option<DisplayId>,
        [gpui_flow_interaction::scene::SceneResizeHandle; 8],
    )> {
        if self.geometry_tool().is_some()
            || self.inner_tool_active()
            || self.image_crop_editor_active()
            || self.line_handles().is_some()
            || !self.is_editable()
            || !self.interactions.is_enabled("resize")
            || self.tools.as_ref().is_some_and(|tools| {
                !tools.outer_tool_active(gpui_flow_interaction::scene::DEFAULT_OUTER_TOOL)
            })
        {
            return None;
        }
        let presented = self.presentation_hit_geometry()?;
        if self.editor.selection.ids.len() > 1 {
            let geometry =
                gpui_flow_interaction::scene::SceneSelectionResizeGeometry::for_selection_with_geometry(
                    &self.editor, self.interactions.transform_capabilities(), presented,
                )?;
            return Some((None, geometry.screen_handles(&self.editor)?));
        }
        let id = self.editor.selection.ids.iter().next()?.clone();
        let handles = self
            .interactions
            .resize_geometry(&self.editor, &id, presented)?
            .screen_handles(&self.editor)?;
        Some((Some(id), handles))
    }
    fn rotation_handle(&self) -> Option<gpui_flow_interaction::scene::SceneRotationHandle> {
        if self
            .geometry_tool()
            .is_some_and(|mode| mode != gpui_flow_interaction::scene::GeometryToolMode::Pivot)
            || self.inner_tool_active()
            || self.image_crop_editor_active()
            || self.line_handles().is_some()
            || !self.is_editable()
            || !self.interactions.is_enabled("rotate")
            || self.tools.as_ref().is_some_and(|tools| {
                !tools.outer_tool_active(gpui_flow_interaction::scene::DEFAULT_OUTER_TOOL)
            })
        {
            return None;
        }
        gpui_flow_interaction::scene::SceneRotationGeometry::for_selection_with_geometry(
            &self.editor,
            self.interactions.transform_capabilities(),
            self.presentation_hit_geometry()?,
        )?
        .screen_handle(&self.editor)
    }
    fn selection_corners(&self, id: &DisplayId) -> Option<[WorldPoint; 4]> {
        let engine = self.presentation_hit_geometry()?;
        let bounds = engine.local_bounds(id).ok()??;
        let transform = engine.world_transform(id).ok()?;
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
