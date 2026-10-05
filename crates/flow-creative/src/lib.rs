//! A typed creative-board layer over Flow's existing graph/editor, not a second engine.
//! Registry callbacks are host code; documents contain only data and stable IDs.
mod model;
pub use model::*;
mod guard;
pub use guard::CreativeGuard;
#[cfg(feature = "desktop")]
mod ui;
#[cfg(feature = "desktop")]
pub use ui::*;

mod generation;
pub use generation::*;

mod project;
pub use project::*;

pub mod model_preview;
mod text_style;
pub use text_style::*;
pub mod mask;
mod mask_node;
mod references;
pub use mask_node::{MASK_MEDIA_TYPE, MaskNodeData, mask_source};
pub mod processing;
pub mod processing_node;
