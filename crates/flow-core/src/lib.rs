//! UI-independent building blocks for node-based editors.
//!
//! [`Graph`] is a serializable graph with typed ports and connection validation.
//! [`Editor`] adds selection, clipboard operations, and atomic undoable changes.
//! [`Viewport`] uses canvas-local screen coordinates, so host toolkits only need
//! to subtract the canvas origin before passing pointer coordinates here.
//!
//! All document mutations performed through `Editor` are validated. Public model
//! fields intentionally permit custom renderers and importers; call
//! [`Graph::validate`] after constructing or modifying a graph directly.

mod editor;
mod geometry;
mod model;

pub use editor::*;
pub use geometry::*;
pub use model::*;
