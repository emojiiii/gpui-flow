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
pub mod text_appearance;
pub use mask_node::{MASK_MEDIA_TYPE, MaskNodeData, mask_source};
pub mod custom_host;
pub mod custom_nodes;
pub mod image_edit;
pub mod processing;
pub mod processing_node;

mod layer_guard;

pub mod arrangement;
pub mod asset_export;
pub mod asset_filter;
pub mod canvas_preferences;
#[cfg(feature = "desktop")]
pub mod frame_png;
#[cfg(feature = "desktop")]
pub mod frame_text;
pub mod frames;

pub mod image_adjust;
pub mod image_rotation;
pub mod object_frame;
pub mod object_placement;
pub mod pencil;
pub mod shapes;
pub mod text_placement;
pub mod vector_export;
