use crate::SceneSelection;
use gpui_flow_core::{Point, Viewport};
use gpui_flow_engine::SceneEngine;
use gpui_flow_scene::{Affine, Scene, SceneError};
#[derive(Clone, PartialEq)]
struct Snapshot {
    scene: Scene,
    selection: SceneSelection,
}
/// Atomic scene editing with bounded snapshot history and explicit gesture lifecycle.
pub struct SceneEditor {
    pub engine: SceneEngine,
    pub selection: SceneSelection,
    pub viewport: Viewport,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    pending: Option<Snapshot>,
    history_limit: usize,
    observed_revision: u64,
}
impl SceneEditor {
    pub fn new(scene: Scene) -> Result<Self, SceneError> {
        Ok(Self::from_engine(SceneEngine::new(scene)?))
    }
    pub fn from_engine(engine: SceneEngine) -> Self {
        let observed_revision = engine.revision();
        Self {
            engine,
            observed_revision,
            selection: SceneSelection::default(),
            viewport: Viewport::default(),
            undo: vec![],
            redo: vec![],
            pending: None,
            history_limit: 100,
        }
    }
    pub fn scene(&self) -> &Scene {
        self.engine.scene()
    }
    /// Replace a document while retaining registry configuration, never old instance identity.
    pub fn load_scene(&mut self, scene: Scene) -> Result<(), SceneError> {
        self.engine.reset_scene(scene)?;
        self.observed_revision = self.engine.revision();
        self.selection.clear();
        self.viewport = Viewport::default();
        self.undo.clear();
        self.redo.clear();
        self.pending = None;
        Ok(())
    }
    pub fn set_history_limit(&mut self, limit: usize) {
        self.history_limit = limit;
        let excess = self.undo.len().saturating_sub(limit);
        self.undo.drain(..excess);
        let excess = self.redo.len().saturating_sub(limit);
        self.redo.drain(..excess);
    }
    fn locked(scene: &Scene, id: &gpui_flow_scene::DisplayId) -> bool {
        let mut current = Some(id);
        while let Some(id) = current {
            let Some(node) = scene.get(id) else {
                return true;
            };
            if node.object.locked {
                return true;
            }
            current = node.parent.as_ref();
        }
        false
    }
    fn snapshot(&self) -> Snapshot {
        Snapshot {
            scene: self.scene().clone(),
            selection: self.selection.clone(),
        }
    }
    fn restore(&mut self, s: Snapshot) -> bool {
        if self.engine.replace_scene(s.scene).is_err() {
            return false;
        }
        self.observed_revision = self.engine.revision();
        self.selection = s.selection;
        self.selection.retain_existing(self.engine.scene());
        true
    }
    fn record(&mut self, before: Snapshot) {
        if before.scene != *self.scene() {
            self.redo.clear();
            if self.history_limit > 0 {
                self.undo.push(before);
                let excess = self.undo.len().saturating_sub(self.history_limit);
                self.undo.drain(..excess);
            }
        }
    }
    pub fn transaction(
        &mut self,
        edit: impl FnOnce(&mut Scene) -> Result<(), SceneError>,
    ) -> Result<(), SceneError> {
        self.reconcile_external();
        let before = self.snapshot();
        self.engine.transaction(edit)?;
        self.observed_revision = self.engine.revision();
        self.selection.retain_existing(self.engine.scene());
        if self.pending.is_none() {
            self.record(before);
        }
        Ok(())
    }
    /// Validated instance property mutation participates in the same undo history.
    pub fn set_property(
        &mut self,
        handle: gpui_flow_engine::ElementHandle,
        key: gpui_flow_engine::PropertyKey,
        value: serde_json::Value,
    ) -> Result<(), SceneError> {
        self.reconcile_external();
        let before = self.snapshot();
        self.engine.set_property(handle, key, value)?;
        self.observed_revision = self.engine.revision();
        self.selection.retain_existing(self.engine.scene());
        if self.pending.is_none() {
            self.record(before);
        }
        Ok(())
    }
    /// Direct host engine writes supersede editor snapshots. Discard stale history
    /// rather than resurrecting destroyed instances or overwriting external data.
    pub fn reconcile_external(&mut self) -> bool {
        if self.observed_revision == self.engine.revision() {
            return false;
        }
        self.pending = None;
        self.undo.clear();
        self.redo.clear();
        self.selection.retain_existing(self.engine.scene());
        self.observed_revision = self.engine.revision();
        true
    }
    /// Starts one undoable gesture. Returns false if a gesture already exists.
    pub fn begin_edit(&mut self) -> bool {
        self.reconcile_external();
        if self.pending.is_some() {
            return false;
        }
        self.pending = Some(self.snapshot());
        true
    }
    pub fn editing(&self) -> bool {
        self.pending.is_some()
    }
    pub fn commit_edit(&mut self) {
        self.reconcile_external();
        if let Some(before) = self.pending.take() {
            self.record(before);
        }
    }
    /// Returns false if no edit exists or a restore is rejected; rejected history remains intact.
    pub fn cancel_edit(&mut self) -> bool {
        self.reconcile_external();
        let Some(before) = self.pending.clone() else {
            return false;
        };
        if !self.restore(before) {
            return false;
        }
        self.pending = None;
        true
    }
    pub fn undo(&mut self) -> bool {
        self.reconcile_external();
        if self.editing() {
            return false;
        }
        let Some(previous) = self.undo.last().cloned() else {
            return false;
        };
        let current = self.snapshot();
        if !self.restore(previous) {
            return false;
        }
        self.undo.pop();
        self.redo.push(current);
        true
    }
    pub fn redo(&mut self) -> bool {
        self.reconcile_external();
        if self.editing() {
            return false;
        }
        let Some(next) = self.redo.last().cloned() else {
            return false;
        };
        let current = self.snapshot();
        if !self.restore(next) {
            return false;
        }
        self.redo.pop();
        self.undo.push(current);
        let excess = self.undo.len().saturating_sub(self.history_limit);
        self.undo.drain(..excess);
        true
    }
    /// Translate selected roots in world coordinates, respecting each parent's affine transform.
    pub fn translate_selection(&mut self, delta: Point) -> Result<(), SceneError> {
        let ids = self.selection.roots(self.scene());
        self.transaction(|scene| {
            for id in ids {
                let node = scene.get(&id).expect("selected node exists");
                if Self::locked(scene, &id) {
                    continue;
                }
                let parent = node.parent.clone();
                let parent_world = match parent {
                    Some(parent) => scene.world_transform(&parent)?,
                    None => Affine::IDENTITY,
                };
                let Some(inverse) = parent_world.inverse() else {
                    continue;
                };
                let delta = inverse.transform_vector(delta);
                scene.update_object(&id, |object| {
                    object.transform = Affine::translation(delta.x, delta.y) * object.transform
                })?;
            }
            Ok(())
        })
    }
    /// Rotate around each selected object's local origin; hierarchy is preserved.
    pub fn rotate_selection(&mut self, degrees: f32) -> Result<(), SceneError> {
        let ids = self.selection.roots(self.scene());
        self.transaction(|scene| {
            for id in ids {
                if !Self::locked(scene, &id) {
                    scene.update_object(&id, |object| {
                        object.transform = object.transform * Affine::rotation(degrees)
                    })?;
                }
            }
            Ok(())
        })
    }
    pub fn scale_selection(&mut self, factor: f32) -> Result<(), SceneError> {
        let ids = self.selection.roots(self.scene());
        self.transaction(|scene| {
            for id in ids {
                if !Self::locked(scene, &id) {
                    scene.update_object(&id, |object| {
                        object.transform = object.transform * Affine::scale(factor, factor)
                    })?;
                }
            }
            Ok(())
        })
    }
    pub fn delete_selection(&mut self) -> Result<(), SceneError> {
        let ids = self.selection.roots(self.scene());
        self.transaction(|scene| {
            for id in ids {
                if !Self::locked(scene, &id) {
                    scene.remove(&id)?;
                }
            }
            Ok(())
        })
    }
}
