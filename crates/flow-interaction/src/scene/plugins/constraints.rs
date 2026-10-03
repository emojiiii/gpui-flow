use crate::scene::{SceneCommand, selection_bounds};
use crate::*;
use gpui_flow_core::{Point, Rect, Size, Viewport};
use gpui_flow_editor::SceneEditor;
use gpui_flow_scene::Affine;

/// Fixed containment rectangle. Parent-local mode requires all selected roots
/// to share a parent (root-level selections share the identity parent).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DragBounds {
    World(Rect),
    ParentLocal(Rect),
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DragAxis {
    #[default]
    Both,
    X,
    Y,
}
/// Runtime-only policy. The axis is measured in the bounds coordinate system,
/// or world coordinates when bounds are absent. Oversized content is centered
/// on an impossible-to-contain axis; it never jumps between opposite edges.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SceneDragConstraints {
    pub bounds: Option<DragBounds>,
    pub axis: DragAxis,
}
#[derive(Debug, Clone, Copy, Default)]
pub struct SceneDragConstraintsPlugin {
    pub constraints: SceneDragConstraints,
}
impl CanvasPlugin<SceneEditor, SceneCommand> for SceneDragConstraintsPlugin {
    fn id(&self) -> &str {
        "drag-constraints"
    }
    fn priority(&self) -> i32 {
        1000
    }
    fn filter_command(
        &mut self,
        command: &mut SceneCommand,
        cx: &PluginContext<'_, SceneEditor>,
        effects: &mut Vec<CanvasEffect>,
    ) -> Result<(), String> {
        let SceneCommand::MoveSelectionTo { anchor } = command else {
            return Ok(());
        };
        let Some(world) = selection_bounds(cx.editor).map_err(|e| e.to_string())? else {
            return Ok(());
        };
        let mut frame = Affine::IDENTITY;
        let mut content = world;
        if matches!(self.constraints.bounds, Some(DragBounds::ParentLocal(_))) {
            let roots = cx.editor.selection.roots(cx.editor.scene());
            let first = roots.first().ok_or("empty constrained selection")?;
            let parent = cx
                .editor
                .scene()
                .get(first)
                .ok_or("missing constrained selection")?
                .parent
                .clone();
            if roots
                .iter()
                .any(|id| cx.editor.scene().get(id).is_none_or(|n| n.parent != parent))
            {
                return Err("parent-local drag bounds require one shared parent".into());
            }
            frame = parent
                .as_ref()
                .map_or(Ok(Affine::IDENTITY), |id| {
                    cx.editor.scene().world_transform(id)
                })
                .map_err(|e| e.to_string())?;
            let mut bounds: Option<Rect> = None;
            for id in roots {
                if let Some(local) = cx
                    .editor
                    .scene()
                    .local_bounds_for_view(&id, cx.editor.viewport.zoom)
                    .map_err(|e| e.to_string())?
                {
                    let node = cx
                        .editor
                        .scene()
                        .get(&id)
                        .ok_or("missing constrained root")?;
                    let local = node.object.transform.transform_bounds(local);
                    bounds = Some(bounds.map_or(local, |old| old.union(local)));
                }
            }
            content = bounds.ok_or("constrained selection has no bounds")?;
        }
        let inverse = frame
            .inverse()
            .ok_or("drag constraint frame is not invertible")?;
        let mut delta = inverse.transform_vector(*anchor - world.origin);
        match self.constraints.axis {
            DragAxis::Both => {}
            DragAxis::X => delta.y = 0.0,
            DragAxis::Y => delta.x = 0.0,
        }
        if let Some(DragBounds::World(bounds) | DragBounds::ParentLocal(bounds)) =
            self.constraints.bounds
        {
            if !bounds.is_valid() || !content.is_valid() || !delta.is_finite() {
                return Err("drag constraint geometry must be finite and nonnegative".into());
            }
            delta = contained_delta(content, bounds, delta);
        }
        let constrained = world.origin + frame.transform_vector(delta);
        if !constrained.is_finite() {
            return Err("drag constraint overflow".into());
        }
        if constrained != *anchor {
            // A clamped snap is no longer a true alignment. Never show a stale guide.
            effects.retain(|effect| !matches!(effect, CanvasEffect::Guides(_)));
            effects.push(CanvasEffect::Guides(vec![]));
            *anchor = constrained;
        }
        Ok(())
    }
}
fn contained_delta(content: Rect, bounds: Rect, delta: Point) -> Point {
    fn axis(start: f32, extent: f32, low: f32, size: f32, delta: f32) -> f32 {
        let (start, extent, low, size, delta) = (
            start as f64,
            extent as f64,
            low as f64,
            size as f64,
            delta as f64,
        );
        (if extent > size {
            low + (size - extent) / 2.0 - start
        } else {
            delta.clamp(low - start, low + size - start - extent)
        }) as f32
    }
    Point::new(
        axis(
            content.min_x(),
            content.size.width,
            bounds.min_x(),
            bounds.size.width,
            delta.x,
        ),
        axis(
            content.min_y(),
            content.size.height,
            bounds.min_y(),
            bounds.size.height,
            delta.y,
        ),
    )
}

/// Constrain a viewport to a fixed world rectangle and a logical-pixel canvas.
/// If content is smaller than the canvas at a given zoom, center that axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewportBounds {
    pub world: Rect,
    pub canvas: Size,
}
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SceneViewportConstraints {
    pub min_zoom: Option<f32>,
    pub max_zoom: Option<f32>,
    pub bounds: Option<ViewportBounds>,
}
impl SceneViewportConstraints {
    pub fn constrain(self, mut viewport: Viewport) -> Result<Viewport, String> {
        if !viewport.is_valid() {
            return Err("invalid viewport".into());
        }
        let min = self
            .min_zoom
            .unwrap_or(viewport.min_zoom)
            .max(viewport.min_zoom);
        let max = self
            .max_zoom
            .unwrap_or(viewport.max_zoom)
            .min(viewport.max_zoom);
        if self.min_zoom.is_some_and(|x| !x.is_finite() || x <= 0.0)
            || self.max_zoom.is_some_and(|x| !x.is_finite() || x <= 0.0)
            || min > max
        {
            return Err("viewport zoom constraints must overlap existing limits".into());
        }
        let zoom = viewport.zoom.clamp(min, max);
        if zoom != viewport.zoom && !viewport.zoom_at(Point::ZERO, zoom) {
            return Err("invalid constrained zoom".into());
        }
        if let Some(bounds) = self.bounds {
            if !bounds.world.is_valid() || !bounds.canvas.is_positive() {
                return Err("invalid viewport world or canvas bounds".into());
            }
            let world = viewport.world_rect_to_screen(bounds.world);
            let canvas = Rect::new(Point::ZERO, bounds.canvas);
            if !world.is_valid() {
                return Err("viewport bounds overflow".into());
            }
            // Keep the canvas inside the world: exchange content/constraint and
            // invert the correction. This also centers undersized world content.
            let correction = contained_delta(canvas, world, Point::ZERO);
            viewport.pan -= correction;
        }
        if !viewport.is_valid() {
            return Err("viewport constraint overflow".into());
        }
        Ok(viewport)
    }
    fn zoom(self, mut viewport: Viewport, position: Point, zoom: f32) -> Result<Viewport, String> {
        // Clamp before zoom_at to keep the requested cursor anchor unchanged
        // unless an actual world boundary forces a pan correction.
        self.constrain(viewport)?;
        let min = self
            .min_zoom
            .unwrap_or(viewport.min_zoom)
            .max(viewport.min_zoom);
        let max = self
            .max_zoom
            .unwrap_or(viewport.max_zoom)
            .min(viewport.max_zoom);
        if !zoom.is_finite() || zoom <= 0.0 || !viewport.zoom_at(position, zoom.clamp(min, max)) {
            return Err("invalid constrained zoom input".into());
        }
        self.constrain(viewport)
    }
}
#[derive(Debug, Clone, Copy, Default)]
pub struct SceneViewportConstraintsPlugin {
    pub constraints: SceneViewportConstraints,
}
impl CanvasPlugin<SceneEditor, SceneCommand> for SceneViewportConstraintsPlugin {
    fn id(&self) -> &str {
        "viewport-constraints"
    }
    fn priority(&self) -> i32 {
        1000
    }
    fn filter_command(
        &mut self,
        command: &mut SceneCommand,
        cx: &PluginContext<'_, SceneEditor>,
        _: &mut Vec<CanvasEffect>,
    ) -> Result<(), String> {
        let mut viewport = cx.editor.viewport;
        match *command {
            SceneCommand::Pan { delta } => {
                if !viewport.pan_by(delta) {
                    return Err("invalid pan".into());
                }
            }
            SceneCommand::Zoom { position, zoom } => {
                viewport = self.constraints.zoom(viewport, position, zoom)?;
            }
            SceneCommand::Fit { canvas, padding } => {
                self.constraints.constrain(viewport)?;
                // Solve with the constraint's allowed range, not a later clamp
                // that could enlarge a fixed-width fit beyond the canvas.
                let original_limits = (viewport.min_zoom, viewport.max_zoom);
                viewport.min_zoom = self
                    .constraints
                    .min_zoom
                    .unwrap_or(viewport.min_zoom)
                    .max(viewport.min_zoom);
                viewport.max_zoom = self
                    .constraints
                    .max_zoom
                    .unwrap_or(viewport.max_zoom)
                    .min(viewport.max_zoom);
                viewport.zoom = viewport.zoom.clamp(viewport.min_zoom, viewport.max_zoom);
                viewport =
                    crate::scene::fit_scene_viewport(cx.editor.scene(), viewport, canvas, padding)
                        .map_err(|e| e.to_string())?;
                viewport.min_zoom = original_limits.0;
                viewport.max_zoom = original_limits.1;
            }
            SceneCommand::SetViewport {
                viewport: candidate,
            } => viewport = candidate,
            _ => return Ok(()),
        }
        *command = SceneCommand::SetViewport {
            viewport: self.constraints.constrain(viewport)?,
        };
        Ok(())
    }
}
