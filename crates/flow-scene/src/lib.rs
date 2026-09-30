//! A retained, toolkit-independent vector scene graph.
//!
//! Display primitives own local geometry, [`DisplayObject`] adds shared style
//! and identity, and [`Scene`] owns the ordered parent/child tree. Transforms,
//! clipping, bounds, picking, serialization, and event propagation share that
//! same tree. Image decoding and font shaping belong to rendering adapters.

pub use gpui_flow_core::{Point, Rect as Bounds, Size};
mod affine;
pub mod display;
mod events;
mod geometry;
mod object;
mod scene;
mod style;

pub use affine::*;
pub use display::Shape;
pub use events::*;
pub use geometry::{Contour, FlattenOptions};
pub use object::*;
pub use scene::*;
pub use style::*;
