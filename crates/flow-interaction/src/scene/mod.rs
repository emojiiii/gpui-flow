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
    let mut result = None;
    for id in editor.selection.roots(editor.scene()) {
        if let Some(bounds) = editor.scene().world_bounds(&id)? {
            result = Some(result.map_or(bounds, |old: Rect| old.union(bounds)));
        }
    }
    Ok(result)
}
pub type SceneDispatchReport = crate::DispatchReport;
pub type SceneInteractionError = crate::InteractionError;

mod router;
pub use router::{RoutedReport, SceneEventRouter, SceneInteractionState};

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
