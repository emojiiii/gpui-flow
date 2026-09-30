//! GPUI adapter for a backend-independent media controller.
//! Frames are uploaded on the UI thread; retired atlas images are explicitly
//! removed. Dropping the view shuts down its owned playback controller.
mod view;
pub use view::*;
