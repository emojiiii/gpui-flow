//! Independently installed retained-scene behavior using CanvasPlugin<SceneEditor, SceneCommand>.
mod constraints;
mod drag;
mod geometry;
mod inertia;
mod keyboard;
mod line;
mod pan;
mod resize;
mod rotate;
mod selection;
mod snapping;
mod zoom;
use super::ScenePluginHost;
use crate::InteractionError;
pub use constraints::*;
pub use drag::SceneDragPlugin;
pub use geometry::SceneGeometryToolsPlugin;
pub use inertia::*;
pub use keyboard::SceneKeyboardPlugin;
pub use line::SceneLineEndpointPlugin;
pub use pan::ScenePanPlugin;
pub use resize::{SceneResizePlugin, TextResizeMode};
pub use rotate::SceneRotatePlugin;
pub use selection::SceneSelectionPlugin;
pub use snapping::{SceneEdgeSnappingPlugin, SceneSnappingPlugin};
pub use zoom::SceneZoomPlugin;
pub fn install_defaults(host: &mut ScenePluginHost) -> Result<(), InteractionError> {
    host.install(SceneSelectionPlugin::default())?;
    host.install(ScenePanPlugin::default())?;
    host.install(SceneResizePlugin::default())?;
    host.install(SceneRotatePlugin::default())?;
    host.install(SceneLineEndpointPlugin::default())?;
    host.install(SceneDragPlugin::default())?;
    host.install(SceneZoomPlugin::default())?;
    host.install(SceneKeyboardPlugin)?;
    Ok(())
}
