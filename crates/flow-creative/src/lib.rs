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
