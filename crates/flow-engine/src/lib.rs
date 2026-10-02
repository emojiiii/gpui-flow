//! Live scene instances, atomic changes, element definitions, and derived layout.
//! The engine owns one attached [`gpui_flow_scene::Scene`]. Documents are inert
//! serializable values; handles, subscriptions, caches and disposers are runtime state.
#![doc = include_str!("../README.md")]

mod change;
mod derive;
mod engine;
mod instance;
mod layout;
mod motion;
mod query;
mod registry;
mod resource;
mod subtree;

pub use change::*;
pub use derive::{LayoutProvider, LayoutProviderId, MAX_LAYOUT_DERIVATION_PASSES};
pub use engine::*;
pub use instance::*;
pub use layout::*;
pub use motion::PathPlacement;
pub use registry::*;
pub use resource::*;

use gpui_flow_scene::SceneError;
#[derive(Debug, Clone, PartialEq)]
pub enum EngineError {
    Scene(SceneError),
    InvalidHandle,
    DetachedHandle,
    InvalidSubtree,
    Registry(String),
    Property(String),
    Layout(String),
    Exhausted,
}
impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Scene(e) => e.fmt(f),
            Self::InvalidHandle => write!(f, "stale, destroyed, or foreign element handle"),
            Self::DetachedHandle => write!(f, "element is detached"),
            Self::InvalidSubtree => write!(f, "stale or foreign detached subtree"),
            Self::Registry(e) => write!(f, "element registry: {e}"),
            Self::Property(e) => write!(f, "property: {e}"),
            Self::Layout(e) => write!(f, "layout: {e}"),
            Self::Exhausted => write!(f, "runtime identifier space exhausted"),
        }
    }
}
impl std::error::Error for EngineError {}
impl From<SceneError> for EngineError {
    fn from(e: SceneError) -> Self {
        Self::Scene(e)
    }
}
/// Compatibility conversion for scene editors retaining their existing error type.
impl From<EngineError> for SceneError {
    fn from(e: EngineError) -> Self {
        match e {
            EngineError::Scene(e) => e,
            e => Self::InvalidGeometry(e.to_string()),
        }
    }
}
