use crate::SceneSelection;
use gpui_flow_core::{Point, Viewport};
use gpui_flow_scene::{Affine, Scene, SceneError};
#[derive(Clone, PartialEq)]
struct Snapshot {
    scene: Scene,
    selection: SceneSelection,
}
/// Atomic scene editing with bounded snapshot history and explicit gesture lifecycle.
pub struct SceneEditor {
    pub scene: Scene,
    pub selection: SceneSelection,
    pub viewport: Viewport,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    pending: Option<Snapshot>,
    history_limit: usize,
}
impl SceneEditor {
    pub fn new(scene: Scene) -> Result<Self, SceneError> {
        scene.validate()?;
        Ok(Self {
            scene,
            selection: SceneSelection::default(),
            viewport: Viewport::default(),
            undo: vec![],
            redo: vec![],
            pending: None,
            history_limit: 100,
        })
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
            scene: self.scene.clone(),
            selection: self.selection.clone(),
        }
    }
    fn restore(&mut self, s: Snapshot) {
        self.scene = s.scene;
        self.selection = s.selection;
    }
    fn record(&mut self, before: Snapshot) {
        if before.scene != self.scene {
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
        let before = self.snapshot();
        let mut candidate = self.scene.clone();
        edit(&mut candidate)?;
        candidate.validate()?;
        self.scene = candidate;
        self.selection.retain_existing(&self.scene);
        if self.pending.is_none() {
            self.record(before);
        }
        Ok(())
    }
    /// Starts one undoable gesture. Returns false if a gesture already exists.
    pub fn begin_edit(&mut self) -> bool {
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
        if let Some(before) = self.pending.take() {
            self.record(before);
        }
    }
    pub fn cancel_edit(&mut self) {
        if let Some(before) = self.pending.take() {
            self.restore(before);
        }
    }
    pub fn undo(&mut self) -> bool {
        if self.editing() {
            return false;
        }
        if let Some(previous) = self.undo.pop() {
            let current = self.snapshot();
            self.restore(previous);
            self.redo.push(current);
            true
        } else {
            false
        }
    }
    pub fn redo(&mut self) -> bool {
        if self.editing() {
            return false;
        }
        if let Some(next) = self.redo.pop() {
            let current = self.snapshot();
            self.restore(next);
            self.undo.push(current);
            let excess = self.undo.len().saturating_sub(self.history_limit);
            self.undo.drain(..excess);
            true
        } else {
            false
        }
    }
    /// Translate selected roots in world coordinates, respecting each parent's affine transform.
    pub fn translate_selection(&mut self, delta: Point) -> Result<(), SceneError> {
        let ids = self.selection.roots(&self.scene);
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
        let ids = self.selection.roots(&self.scene);
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
        let ids = self.selection.roots(&self.scene);
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
        let ids = self.selection.roots(&self.scene);
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
