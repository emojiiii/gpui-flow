//! Composable input behavior for flow canvases. Plugins receive immutable
//! editor context and emit commands; the host validates every command after
//! middleware and capability checks. No toolkit or global event handler needed.
mod command;
mod event;
mod host;
mod plugin;
pub mod plugins;
pub use command::*;
pub use event::*;
pub use gpui_flow_core as core;
pub use host::*;
pub use plugin::*;

pub mod scene;
