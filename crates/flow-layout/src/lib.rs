//! Optional, deterministic visual flow layout. Applying a complete layout uses
//! one ordinary scene transaction; rendering and hit testing consume that scene.
#![doc = include_str!("../README.md")]

mod arrange;
mod reactive;
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
    NonConvergent { passes: usize },
}
impl std::fmt::Display for FlowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInput { id, message } => write!(f, "flow layout {id}: {message}"),
            Self::Scene(e) => e.fmt(f),
            Self::Engine(e) => e.fmt(f),
            Self::Serialization(e) => write!(f, "flow document: {e}"),
            Self::NonConvergent { passes } => write!(
                f,
                "flow layout did not converge within {passes} measure/arrange passes"
            ),
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

/// Serializable optional layout input. Explicit passes work independently; opt-in
/// install_reactive binds engine precommit derivation. This value alone installs
/// no observers, global plugins, or frame callbacks.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FlowLayout {
    pub containers: BTreeMap<DisplayId, FlowStyle>,
    pub items: BTreeMap<DisplayId, FlowItem>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FlowReport {
    /// Full measure/arrange passes; automatic sizes require a stable repeat.
    pub passes: usize,
    pub containers: usize,
    pub items: usize,
    /// Sorted IDs with changed transforms or shape geometry, never duplicates.
    pub changed: Vec<DisplayId>,
}

pub const MAX_FLOW_LAYOUT_PASSES: usize = 32;

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
                return Err(invalid(id, "flow container requires a Box or Frame"));
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
        // Size ownership is explicit: intrinsic content measurement must not
        // feed back through grow/stretch or another parent's allocation.
        for (id, style) in &self.containers {
            let node = scene.get(id).expect("validated container");
            let auto_main = if style.direction == FlowDirection::Row {
                style.width_sizing
            } else {
                style.height_sizing
            }
            .is_content();
            let auto_cross = if style.direction == FlowDirection::Row {
                style.height_sizing
            } else {
                style.width_sizing
            }
            .is_content();
            for child in &node.children {
                let item = self.items.get(child).cloned().unwrap_or_default();
                if !item.in_flow {
                    continue;
                }
                if auto_main && item.grow > 0.0 {
                    return Err(invalid(
                        child,
                        "grow cannot depend on a content-sized main axis",
                    ));
                }
                if auto_cross && item.align_self.unwrap_or(style.align_items) == AlignItems::Stretch
                {
                    return Err(invalid(
                        child,
                        "stretch cannot depend on a content-sized cross axis; choose a non-stretch alignment",
                    ));
                }
            }
            if let Some(parent_style) = node
                .parent
                .as_ref()
                .and_then(|parent| self.containers.get(parent))
            {
                let item = self.items.get(id).cloned().unwrap_or_default();
                if item.in_flow {
                    let main_allocated = item.grow > 0.0 || item.basis.is_some();
                    let cross_allocated =
                        item.align_self.unwrap_or(parent_style.align_items) == AlignItems::Stretch;
                    let (width_allocated, height_allocated) =
                        if parent_style.direction == FlowDirection::Row {
                            (main_allocated, cross_allocated)
                        } else {
                            (cross_allocated, main_allocated)
                        };
                    let width_allocated =
                        width_allocated || item.min_width.is_some() || item.max_width.is_some();
                    let height_allocated =
                        height_allocated || item.min_height.is_some() || item.max_height.is_some();
                    if (style.width_sizing.is_content() && width_allocated)
                        || (style.height_sizing.is_content() && height_allocated)
                    {
                        return Err(invalid(
                            id,
                            "content-sized dimension is also allocated by its parent; put limits on the container content sizing instead",
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    /// Compute on a private copy. No source mutation occurs, including on error.
    pub fn arranged(&self, scene: &Scene) -> Result<(Scene, FlowReport), FlowError> {
        self.arranged_with_limit(scene, MAX_FLOW_LAYOUT_PASSES)
    }
    fn arranged_with_limit(
        &self,
        scene: &Scene,
        limit: usize,
    ) -> Result<(Scene, FlowReport), FlowError> {
        self.validate(scene)?;
        let mut next = scene.clone();
        let mut order = Vec::new();
        let mut stack: Vec<_> = scene.roots().iter().rev().cloned().collect();
        while let Some(id) = stack.pop() {
            stack.extend(
                scene
                    .get(&id)
                    .expect("validated tree")
                    .children
                    .iter()
                    .rev()
                    .cloned(),
            );
            if self.containers.contains_key(&id) {
                order.push(id);
            }
        }
        let automatic = self
            .containers
            .values()
            .any(|style| style.width_sizing.is_content() || style.height_sizing.is_content());
        let mut report = FlowReport {
            containers: order.len(),
            ..Default::default()
        };
        if !order.is_empty() {
            let mut converged = false;
            for pass in 0..if automatic { limit } else { 1 } {
                let before = next.clone();
                if automatic {
                    // Measure nested content before a parent consumes it.
                    for id in order.iter().rev() {
                        arrange::container(&mut next, id, &self.containers[id], &self.items)?;
                    }
                }
                report.items = 0;
                // Parent allocations are final before wrapping/arranging children.
                for id in &order {
                    report.items +=
                        arrange::container(&mut next, id, &self.containers[id], &self.items)?;
                }
                report.passes = pass + 1;
                if !automatic || next == before {
                    converged = true;
                    break;
                }
            }
            if !converged {
                return Err(FlowError::NonConvergent { passes: limit });
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
    /// Apply an input edit and all dependent size/position updates as one editor
    /// transaction. This is an explicit host entry point, not a global observer.
    pub fn transaction_editor(
        &self,
        editor: &mut SceneEditor,
        edit: impl FnOnce(&mut Scene) -> Result<(), SceneError>,
    ) -> Result<FlowReport, FlowError> {
        let mut candidate = editor.scene().clone();
        edit(&mut candidate)?;
        let candidate = editor.engine.prepare_scene(candidate)?;
        let (next, report) = self.arranged(&candidate)?;
        editor.transaction(|scene| {
            *scene = next;
            Ok(())
        })?;
        Ok(report)
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

#[cfg(test)]
mod convergence_tests {
    use super::*;
    use gpui_flow_scene::{DisplayObject, display};
    #[test]
    fn bounded_measurement_failure_does_not_mutate_the_input() {
        let mut scene = Scene::new();
        scene
            .insert(
                DisplayObject::new("box", display::Box::new(400., 300.)),
                None,
                None,
            )
            .unwrap();
        scene
            .insert(
                DisplayObject::new("child", display::Rect::new(30., 20.)),
                Some("box".into()),
                None,
            )
            .unwrap();
        let original = scene.clone();
        let mut layout = FlowLayout::default();
        layout.containers.insert(
            "box".into(),
            FlowStyle {
                width_sizing: AxisSizing::content(),
                height_sizing: AxisSizing::content(),
                ..Default::default()
            },
        );
        assert_eq!(
            layout.arranged_with_limit(&scene, 1).unwrap_err(),
            FlowError::NonConvergent { passes: 1 }
        );
        assert_eq!(scene, original);
        assert!(layout.arranged(&scene).unwrap().1.passes > 1);
    }
}
