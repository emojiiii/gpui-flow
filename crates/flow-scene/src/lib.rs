//! A retained, toolkit-independent vector scene graph.
//!
//! Display primitives own local geometry, [`DisplayObject`] adds shared style
//! and identity, and [`Scene`] owns the ordered parent/child tree. Transforms,
//! clipping, bounds, picking, serialization, and event propagation share that
//! same tree. Image decoding and font shaping belong to rendering adapters.
//!
//! ```
//! use gpui_flow_scene::{display, Affine, DisplayObject, HitTestOptions, Point, Scene};
//!
//! let mut scene = Scene::new();
//! scene.insert(DisplayObject::new("frame", display::Frame::new(300.0, 200.0)), None, None)?;
//! scene.insert(
//!     DisplayObject::new("circle", display::Ellipse::circle(30.0))
//!         .with_transform(Affine::translation(40.0, 50.0)),
//!     Some("frame".into()), None,
//! )?;
//! let hit = scene.hit_test(Point::new(70.0, 80.0), HitTestOptions::default())?.unwrap();
//! assert_eq!(hit.id.as_str(), "circle");
//! assert_eq!(Scene::from_json(&scene.to_json()?)?, scene);
//! # Ok::<(), gpui_flow_scene::SceneError>(())
//! ```

pub use gpui_flow_core::{Point, Rect as Bounds, Size};
mod affine;
pub mod display;
mod events;
mod geometry;
mod gradient;
pub mod leafer_json;
mod mask;
mod object;
mod paint_phase;
mod scene;
mod stroke;
mod stroke_scale;
pub use stroke_scale::*;
mod style;

pub use affine::*;
pub use display::Shape;
pub use events::*;
pub use geometry::{Contour, FlattenOptions};
pub use gradient::*;
pub use object::*;
pub use paint_phase::*;
pub use scene::*;
pub use stroke::*;
pub use style::*;
