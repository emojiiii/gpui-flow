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
mod player;
mod probe;
mod process;
mod types;

pub use backend::*;
pub use player::*;
pub use types::*;
