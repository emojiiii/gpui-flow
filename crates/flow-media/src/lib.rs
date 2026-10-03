//! Local-file audio/video playback using host-installed FFmpeg tools.
//!
//! No shell or network sources are accepted. Decode workers and the audio output
//! process are owned by a player and killed/reaped on pause, seek, stop and drop.
//! Video uses a bounded raw-BGRA queue; audio uses FFplay's native SDL output.
//! Both start from the same seek position and wall clock. This is a preview
//! backend, not sample-accurate lip synchronization or a replacement for an NLE.
mod backend;
mod decoder;
mod engine;
mod image_loader;
mod loading;
mod player;
mod pooled_backend;
mod probe;
mod process;
mod types;

pub use backend::*;
pub use image_loader::*;
pub use loading::*;
pub use player::*;
pub use pooled_backend::*;
pub use types::*;
