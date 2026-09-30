//! Optional, deterministic visual flow layout. Applying a complete layout uses
//! one ordinary scene transaction; rendering and hit testing consume that scene.
#![doc = include_str!("../README.md")]

mod arrange;
mod style;
pub use style::*;

use gpui_flow_editor::SceneEditor;
use gpui_flow_engine::{ChangeOrigin, ChangeSet, EngineError, SceneEngine};
use gpui_flow_scene::{DisplayId, Scene, SceneError, Shape};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub enum FlowError {
    InvalidInput { id: DisplayId, message: String },
    Scene(SceneError),
    Engine(EngineError),
    Serialization(String),
}
impl std::fmt::Display for FlowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInput { id, message } => write!(f, "flow layout {id}: {message}"),
            Self::Scene(e) => e.fmt(f),
            Self::Engine(e) => e.fmt(f),
            Self::Serialization(e) => write!(f, "flow document: {e}"),
        }
    }
}
impl std::error::Error for FlowError {}
impl From<SceneError> for FlowError {
    fn from(value: SceneError) -> Self {
        Self::Scene(value)
    }
}
impl From<EngineError> for FlowError {
    fn from(value: EngineError) -> Self {
        Self::Engine(value)
    }
}
pub(crate) fn invalid(id: &DisplayId, message: impl Into<String>) -> FlowError {
    FlowError::InvalidInput {
        id: id.clone(),
        message: message.into(),
    }
}

/// Serializable optional layout input. An explicit pass is required after input
/// changes; this value installs no observers, global plugins, or frame callbacks.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FlowLayout {
    pub containers: BTreeMap<DisplayId, FlowStyle>,
    pub items: BTreeMap<DisplayId, FlowItem>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FlowReport {
    pub containers: usize,
    pub items: usize,
    /// Sorted IDs with changed transforms or shape geometry, never duplicates.
    pub changed: Vec<DisplayId>,
}

impl FlowLayout {
    /// Validate all configuration, including excluded items. IDs must exist;
    /// item overrides must be direct children of a configured container.
    pub fn validate(&self, scene: &Scene) -> Result<(), FlowError> {
        scene.validate()?;
        for (id, style) in &self.containers {
            let node = scene
                .get(id)
                .ok_or_else(|| invalid(id, "container does not exist"))?;
            if !matches!(node.object.shape, Shape::Box(_) | Shape::Frame(_)) {
                return Err(invalid(
                    id,
                    "flow container requires a fixed-size Box or Frame",
                ));
            }
            style::validate_style(style).map_err(|e| invalid(id, e))?;
        }
        for (id, item) in &self.items {
            let node = scene
                .get(id)
                .ok_or_else(|| invalid(id, "item does not exist"))?;
            if !node
                .parent
                .as_ref()
                .is_some_and(|p| self.containers.contains_key(p))
            {
                return Err(invalid(
                    id,
                    "item must belong to a configured flow container",
                ));
            }
            style::validate_item(item).map_err(|e| invalid(id, e))?;
        }
        Ok(())
    }

    /// Compute on a private copy. No source mutation occurs, including on error.
    pub fn arranged(&self, scene: &Scene) -> Result<(Scene, FlowReport), FlowError> {
        self.validate(scene)?;
        let mut next = scene.clone();
        let mut report = FlowReport::default();
        // Parent-first scene order, not map/ID order: a growing nested container
        // receives its final size before its own children are arranged.
        let mut stack: Vec<_> = scene.roots().iter().rev().cloned().collect();
        while let Some(id) = stack.pop() {
            let node = next.get(&id).expect("validated tree");
            stack.extend(node.children.iter().rev().cloned());
            if let Some(style) = self.containers.get(&id) {
                report.items += arrange::container(&mut next, &id, style, &self.items)?;
                report.containers += 1;
            }
        }
        next.validate()?;
        report.changed = next
            .nodes()
            .filter(|node| scene.get(&node.object.id).expect("same topology").object != node.object)
            .map(|node| node.object.id.clone())
            .collect();
        Ok((next, report))
    }

    /// Atomically replace only the document state, retaining engine identities,
    /// subscriptions, dirty propagation, and one Layout-origin change batch.
    pub fn apply_to_engine(
        &self,
        engine: &mut SceneEngine,
    ) -> Result<(FlowReport, ChangeSet), FlowError> {
        let (next, report) = self.arranged(engine.scene())?;
        let changes = engine.transaction_with_origin(ChangeOrigin::Layout, |scene| {
            *scene = next;
            Ok(())
        })?;
        Ok((report, changes))
    }

    /// One undoable edit for all containers and children (or joins an active
    /// editor gesture). This trusted host API does not enforce editing locks.
    pub fn apply_to_editor(&self, editor: &mut SceneEditor) -> Result<FlowReport, FlowError> {
        let (next, report) = self.arranged(editor.scene())?;
        editor.transaction(|scene| {
            *scene = next;
            Ok(())
        })?;
        Ok(report)
    }

    /// Atomic convenience for an inert scene, without history or runtime state.
    pub fn apply_to_scene(&self, scene: &mut Scene) -> Result<FlowReport, FlowError> {
        let (next, report) = self.arranged(scene)?;
        *scene = next;
        Ok(report)
    }
}

/// Persistence wrapper: saves explicit flow input beside the scene without
/// reserving or overwriting application-owned `Scene.data` / object data.
/// Bare `Scene` JSON saves the arranged geometry only, not layout configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlowDocument {
    pub scene: Scene,
    #[serde(default)]
    pub layout: FlowLayout,
}
impl FlowDocument {
    pub fn to_json(&self) -> Result<String, FlowError> {
        self.layout.validate(&self.scene)?;
        serde_json::to_string_pretty(self).map_err(|e| FlowError::Serialization(e.to_string()))
    }
    pub fn from_json(json: &str) -> Result<Self, FlowError> {
        let document: Self =
            serde_json::from_str(json).map_err(|e| FlowError::Serialization(e.to_string()))?;
        document.layout.validate(&document.scene)?;
        Ok(document)
    }
}
