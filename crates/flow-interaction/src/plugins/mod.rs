//! Independently installable behaviors. IDs are stable and replacement plugins
//! can use the same ID. No behavior is installed by PluginHost::default().
mod connection;
mod drag;
mod keyboard;
mod pan;
mod resize;
mod selection;
mod snapping;
mod zoom;
use crate::{InteractionError, PluginHost};
pub use connection::ConnectionPlugin;
pub use drag::DragPlugin;
pub use keyboard::KeyboardPlugin;
pub use pan::PanPlugin;
pub use resize::ResizePlugin;
pub use selection::SelectionPlugin;
pub use snapping::{SnapConfig, SnappingPlugin};
pub use zoom::ZoomPlugin;
pub fn install_defaults(host: &mut PluginHost) -> Result<(), InteractionError> {
    host.install(PanPlugin::default())?;
    host.install(SelectionPlugin::default())?;
    host.install(ConnectionPlugin::default())?;
    host.install(ResizePlugin::default())?;
    host.install(DragPlugin::default())?;
    host.install(ZoomPlugin::default())?;
    host.install(SnappingPlugin::default())?;
    host.install(KeyboardPlugin)?;
    Ok(())
}
