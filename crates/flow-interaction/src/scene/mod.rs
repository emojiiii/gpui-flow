//! Retained-scene adapter for the same plugin/event/policy contract used by node graphs.
mod command;
mod host;
pub mod plugins;
pub use command::SceneCommand;
use gpui_flow_core::Rect;
use gpui_flow_editor::SceneEditor;
use gpui_flow_scene::{DisplayId, Scene, SceneError};
pub use host::ScenePluginHost;
/// Check inherited locking and invertible parent transforms before editing.
pub fn object_editable(scene: &Scene, id: &DisplayId) -> bool {
    let Ok(path) = scene.ancestor_path(id) else {
        return false;
    };
    if path
        .iter()
        .any(|id| scene.get(id).is_none_or(|node| node.object.locked))
    {
        return false;
    }
    let Some(node) = scene.get(id) else {
        return false;
    };
    node.parent.as_ref().is_none_or(|parent| {
        scene
            .world_transform(parent)
            .ok()
            .and_then(|matrix| matrix.inverse())
            .is_some()
    })
}
pub fn selection_bounds(editor: &SceneEditor) -> Result<Option<Rect>, SceneError> {
    let presented = editor.scene().resolved_for_view(editor.viewport.zoom)?;
    let mut result = None;
    for id in editor.selection.roots(editor.scene()) {
        if let Some(bounds) = presented.world_bounds(&id)? {
            result = Some(result.map_or(bounds, |old: Rect| old.union(bounds)));
        }
    }
    Ok(result)
}
pub type SceneDispatchReport = crate::DispatchReport;
pub type SceneInteractionError = crate::InteractionError;

mod router;
pub use router::{
    PointerGestureProfile, RoutedReport, SceneEventRouter, SceneInteractionState, SemanticDefault,
    SemanticDispatch,
};

mod state_style;
pub use state_style::*;

mod action_queue;
pub use action_queue::{ActionQueueError, SceneActionSender, SceneFocusRequest};

mod state_transition;
pub use state_transition::{StateTransition, StateTransitions};

mod tools;
pub use tools::*;

mod resize_handles;
pub use resize_handles::*;
mod rotation_handles;
pub use rotation_handles::*;

mod line_endpoints;
pub use line_endpoints::*;
mod geometry_tools;
pub use geometry_tools::*;

mod image_crop;
pub use image_crop::*;

/// Fit evaluated at each candidate's own logical zoom. Every tested candidate
/// may become the result only after geometry admission. Invalid probes are
/// unavailable, not silently clamped. At most 64 resolutions retain the largest
/// observed fitting candidate (not a guaranteed global optimum), including
/// a numeric policy's strict-threshold jump, and never changes authored widths.
pub fn fit_scene_viewport(
    scene: &Scene,
    mut viewport: gpui_flow_core::Viewport,
    canvas: gpui_flow_core::Size,
    padding: f32,
) -> Result<gpui_flow_core::Viewport, SceneError> {
    let invalid = || {
        SceneError::InvalidGeometry(
            "scale-fixed fit cannot fit within the admitted viewport limits".into(),
        )
    };
    if scene.resolved_stroke_camera().is_some() || scene.has_transfer_context() {
        return Err(SceneError::InvalidGeometry(
            "fit requires the authored scene".into(),
        ));
    }
    if !scene.has_scale_fixed_strokes() {
        if let Some(bounds) = scene.visible_bounds()?
            && !viewport.fit(bounds, canvas, padding)
        {
            return Err(invalid());
        }
        return Ok(viewport);
    }
    if !viewport.is_valid() || !canvas.is_positive() || !padding.is_finite() || padding < 0. {
        return Err(invalid());
    }
    let available =
        gpui_flow_core::Size::new(canvas.width - 2. * padding, canvas.height - 2. * padding);
    if !available.is_positive() {
        return Err(invalid());
    }
    let fits = |zoom: f32| -> Result<bool, SceneError> {
        Ok(scene.visible_bounds_for_view(zoom)?.is_none_or(|bounds| {
            bounds.size.width as f64 * zoom as f64 <= available.width as f64
                && bounds.size.height as f64 * zoom as f64 <= available.height as f64
        }))
    };
    // Precision/work admission need not hold at either viewport endpoint or
    // every intermediate width. Only successful probes may become the result.
    let mut seeds = vec![
        viewport.zoom,
        1f32.clamp(viewport.min_zoom, viewport.max_zoom),
    ];
    if let Ok(Some(bounds)) = scene.visible_bounds_for_view(viewport.zoom) {
        let mut guess = viewport;
        if guess.fit(bounds, canvas, padding) {
            seeds.push(guess.zoom);
        }
    }
    let log_min = (viewport.min_zoom as f64).ln();
    let log_max = (viewport.max_zoom as f64).ln();
    for index in 0..32 {
        seeds.push(if index == 0 {
            viewport.min_zoom
        } else if index == 31 {
            viewport.max_zoom
        } else {
            (log_min + (log_max - log_min) * index as f64 / 31.).exp() as f32
        });
    }
    let mut best: Option<f32> = None;
    for candidate in seeds {
        if fits(candidate).unwrap_or(false) && best.is_none_or(|old| candidate > old) {
            best = Some(candidate);
        }
    }
    let mut lo = best.ok_or_else(invalid)?;
    let mut hi = viewport.max_zoom;
    for _ in 0..24 {
        let mid = ((lo as f64 + hi as f64) * 0.5) as f32;
        if mid == lo || mid == hi {
            break;
        }
        if fits(mid).unwrap_or(false) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    viewport.zoom = lo;
    if let Some(bounds) = scene.visible_bounds_for_view(lo)? {
        viewport.pan = gpui_flow_core::Point::new(canvas.width * 0.5, canvas.height * 0.5)
            - bounds.center() * lo;
    }
    if !viewport.is_valid() {
        return Err(invalid());
    }
    Ok(viewport)
}
