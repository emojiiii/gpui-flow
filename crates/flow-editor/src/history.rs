use crate::SceneSelection;
use gpui_flow_core::{Point, Viewport};
use gpui_flow_engine::{
    ChangeOrigin, ChangeSet, ElementHandle, EngineError, PropertyKey, SceneEngine,
};
use gpui_flow_scene::{Affine, Scene, SceneError};
#[derive(Clone, PartialEq)]
struct Snapshot {
    scene: Scene,
    selection: SceneSelection,
}
/// Opaque ownership of one editor playback baseline. Never serialize tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaybackToken {
    engine: u64,
    sequence: u64,
}
/// Opaque ownership of a live editor session. Never serialize tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditSessionToken {
    engine: u64,
    sequence: u64,
}
struct OwnedEdit {
    token: EditSessionToken,
    document_epoch: u64,
    baseline: Snapshot,
    last_owned: Snapshot,
}
struct Playback {
    token: PlaybackToken,
    document_epoch: u64,
    baseline: Snapshot,
    last_frame: Snapshot,
}
/// Atomic scene editing with bounded snapshot history and explicit gesture lifecycle.
pub struct SceneEditor {
    pub engine: SceneEngine,
    pub selection: SceneSelection,
    pub viewport: Viewport,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    pending: Option<Snapshot>,
    edit_session: Option<OwnedEdit>,
    edit_sequence: u64,
    playback: Option<Playback>,
    playback_sequence: u64,
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
            edit_session: None,
            edit_sequence: 0,
            playback: None,
            playback_sequence: 0,
            history_limit: 100,
        }
    }
    pub fn scene(&self) -> &Scene {
        self.engine.scene()
    }
    /// Replace a document while retaining registry configuration, never old instance identity.
    pub fn load_scene(&mut self, scene: Scene) -> Result<(), SceneError> {
        scene.validate_view(Viewport::default().zoom)?;
        self.engine.set_stroke_view_scale(Viewport::default().zoom);
        self.engine.reset_scene(scene)?;
        self.observed_revision = self.engine.revision();
        self.selection.clear();
        self.viewport = Viewport::default();
        self.undo.clear();
        self.redo.clear();
        self.pending = None;
        self.edit_session = None;
        self.playback = None;
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
        self.engine.set_stroke_view_scale(self.viewport.zoom);
        if self.engine.replace_scene(s.scene).is_err() {
            return false;
        }
        self.observed_revision = self.engine.revision();
        self.selection = s.selection;
        self.selection.retain_existing(self.engine.scene());
        true
    }
    fn push_undo(&mut self, before: Snapshot) {
        self.redo.clear();
        if self.history_limit > 0 {
            self.undo.push(before);
            let excess = self.undo.len().saturating_sub(self.history_limit);
            self.undo.drain(..excess);
        }
    }
    fn record(&mut self, before: Snapshot) {
        if before.scene != *self.scene() {
            self.push_undo(before);
        }
    }
    fn finish_active_playback(&mut self) -> bool {
        let Some(playback) = self.playback.take() else {
            return false;
        };
        self.record(playback.baseline);
        true
    }
    fn finish_playback_before_edit(&mut self, before: &Snapshot) {
        if let Some(playback) = self.playback.take()
            && playback.baseline.scene != before.scene
        {
            self.push_undo(playback.baseline);
        }
    }
    /// Reserve a playback baseline without changing Undo/Redo. A user gesture
    /// or another playback must finish first; ownership cannot be stolen.
    pub fn begin_playback(&mut self) -> Result<PlaybackToken, EngineError> {
        self.reconcile_external();
        if self.pending.is_some() || self.edit_session.is_some() || self.playback.is_some() {
            return Err(EngineError::Property(
                "an edit or playback is already active".into(),
            ));
        }
        let sequence = self
            .playback_sequence
            .checked_add(1)
            .ok_or(EngineError::Exhausted)?;
        let token = PlaybackToken {
            engine: self.engine.engine_id(),
            sequence,
        };
        let baseline = self.snapshot();
        self.playback = Some(Playback {
            token,
            document_epoch: self.engine.document_epoch(),
            last_frame: baseline.clone(),
            baseline,
        });
        self.playback_sequence = sequence;
        Ok(token)
    }
    /// Also reconciles direct host writes. Such a write ends the session before
    /// a stale controller can overwrite it.
    pub fn is_playback_active(&mut self, token: PlaybackToken) -> bool {
        self.reconcile_external();
        self.playback
            .as_ref()
            .is_some_and(|playback| playback.token == token)
    }
    /// Animation-only property batch; records the latest frame but no per-frame
    /// Undo entry. Invalid batches leave the baseline, clock and history intact.
    pub fn apply_playback_properties(
        &mut self,
        token: PlaybackToken,
        properties: impl IntoIterator<Item = (ElementHandle, PropertyKey, serde_json::Value)>,
    ) -> Result<ChangeSet, EngineError> {
        if !self.is_playback_active(token) {
            return Err(EngineError::Property(
                "playback ended or token belongs to another editor".into(),
            ));
        }
        let changes = self
            .engine
            .set_properties_batch(ChangeOrigin::Animation, properties)?;
        self.observed_revision = self.engine.revision();
        self.selection.retain_existing(self.engine.scene());
        let last_frame = self.snapshot();
        self.playback.as_mut().expect("active playback").last_frame = last_frame;
        Ok(changes)
    }
    /// Token-owned atomic frame with post-layout validation and no per-frame
    /// history entry. Failure preserves both the playback baseline and last frame.
    pub fn apply_playback_transaction_validated(
        &mut self,
        token: PlaybackToken,
        edit: impl FnOnce(&mut Scene) -> Result<(), SceneError>,
        validate: impl FnOnce(&Scene) -> Result<(), SceneError>,
    ) -> Result<ChangeSet, EngineError> {
        if !self.is_playback_active(token) {
            return Err(EngineError::Property(
                "playback ended or token belongs to another editor".into(),
            ));
        }
        let changes = self.engine.transaction_validated_with_origin(
            ChangeOrigin::Animation,
            edit,
            validate,
        )?;
        self.observed_revision = self.engine.revision();
        self.selection.retain_existing(self.engine.scene());
        let last_frame = self.snapshot();
        self.playback.as_mut().expect("active playback").last_frame = last_frame;
        Ok(changes)
    }
    /// Token-owned path-placement frame, retaining the engine's source/anchor
    /// post-layout validation. Does not end playback or create a per-frame Undo.
    pub fn apply_playback_path_placement(
        &mut self,
        token: PlaybackToken,
        target: ElementHandle,
        path: ElementHandle,
        options: gpui_flow_engine::PathPlacement,
    ) -> Result<ChangeSet, EngineError> {
        if !self.is_playback_active(token) {
            return Err(EngineError::Property(
                "playback ended or token belongs to another editor".into(),
            ));
        }
        let changes = self.engine.place_on_path_with_origin(
            target,
            path,
            options,
            ChangeOrigin::Animation,
        )?;
        self.observed_revision = self.engine.revision();
        self.selection.retain_existing(self.engine.scene());
        let last_frame = self.snapshot();
        self.playback.as_mut().expect("active playback").last_frame = last_frame;
        Ok(changes)
    }
    /// Keep the final pose as exactly one undoable edit, preserving prior edits.
    pub fn finish_playback(&mut self, token: PlaybackToken) -> bool {
        if !self.is_playback_active(token) {
            return false;
        }
        self.finish_active_playback()
    }
    /// Restore the playback baseline without consuming prior Undo/Redo. External
    /// edits end ownership first, so cancellation never overwrites them.
    pub fn cancel_playback(&mut self, token: PlaybackToken) -> bool {
        if !self.is_playback_active(token) {
            return false;
        }
        let mut before = self
            .playback
            .as_ref()
            .expect("active playback")
            .baseline
            .clone();
        // Selection is host interaction state, not an animated property.
        before.selection = self.selection.clone();
        if !self.restore(before) {
            return false;
        }
        self.playback = None;
        true
    }
    /// Reserve one undo baseline for a long-running, token-owned edit. Gestures
    /// and playback cannot share it. Public editor edits revoke this ownership
    /// only after a successful commit, preserving each as a separate undo step.
    pub fn begin_edit_session(&mut self) -> Result<EditSessionToken, EngineError> {
        self.reconcile_external();
        if self.pending.is_some() || self.edit_session.is_some() || self.playback.is_some() {
            return Err(EngineError::Property(
                "an edit or playback is already active".into(),
            ));
        }
        let sequence = self
            .edit_sequence
            .checked_add(1)
            .ok_or(EngineError::Exhausted)?;
        let token = EditSessionToken {
            engine: self.engine.engine_id(),
            sequence,
        };
        let baseline = self.snapshot();
        self.edit_session = Some(OwnedEdit {
            token,
            document_epoch: self.engine.document_epoch(),
            last_owned: baseline.clone(),
            baseline,
        });
        self.edit_sequence = sequence;
        Ok(token)
    }
    pub fn is_edit_session_active(&mut self, token: EditSessionToken) -> bool {
        self.reconcile_external();
        self.edit_session
            .as_ref()
            .is_some_and(|edit| edit.token == token)
    }
    /// The only mutation route that can extend an owned edit. A stale token
    /// cannot mutate the scene or another edit's history baseline.
    pub fn set_edit_session_property(
        &mut self,
        token: EditSessionToken,
        handle: ElementHandle,
        key: PropertyKey,
        value: serde_json::Value,
    ) -> Result<ChangeSet, EngineError> {
        self.set_edit_session_properties(token, handle, [(key, value)])
    }
    /// Atomic multi-property edit within the same owned history baseline.
    pub fn set_edit_session_properties(
        &mut self,
        token: EditSessionToken,
        handle: ElementHandle,
        properties: impl IntoIterator<Item = (PropertyKey, serde_json::Value)>,
    ) -> Result<ChangeSet, EngineError> {
        if !self.is_edit_session_active(token) {
            return Err(EngineError::Property(
                "edit session ended or belongs to another editor".into(),
            ));
        }
        let changes = self.engine.set_properties(handle, properties)?;
        self.observed_revision = self.engine.revision();
        self.selection.retain_existing(self.engine.scene());
        let last_owned = self.snapshot();
        self.edit_session
            .as_mut()
            .expect("active edit session")
            .last_owned = last_owned;
        Ok(changes)
    }
    fn finish_active_edit_session(&mut self) -> bool {
        let Some(edit) = self.edit_session.take() else {
            return false;
        };
        self.record(edit.baseline);
        true
    }
    fn finish_edit_session_before_edit(&mut self, before: &Snapshot) {
        if let Some(edit) = self.edit_session.take()
            && edit.baseline.scene != before.scene
        {
            self.push_undo(edit.baseline);
        }
    }
    /// Keep live changes as one undo step. Also used when a target or permission
    /// is revoked: discarding a session never writes its stale baseline back.
    pub fn finish_edit_session(&mut self, token: EditSessionToken) -> bool {
        if !self.is_edit_session_active(token) {
            return false;
        }
        self.finish_active_edit_session()
    }
    /// Escape rollback is safe only while this token still owns all intervening
    /// document edits. Selection is interaction state and is not rolled back.
    pub fn cancel_edit_session(&mut self, token: EditSessionToken) -> bool {
        if !self.is_edit_session_active(token) {
            return false;
        }
        let mut baseline = self
            .edit_session
            .as_ref()
            .expect("active edit session")
            .baseline
            .clone();
        baseline.selection = self.selection.clone();
        if !self.restore(baseline) {
            return false;
        }
        self.edit_session = None;
        true
    }
    pub fn transaction(
        &mut self,
        edit: impl FnOnce(&mut Scene) -> Result<(), SceneError>,
    ) -> Result<(), SceneError> {
        self.transaction_validated(edit, |_| Ok(()))
    }
    /// Validate the final normalized/derived candidate without exposing a
    /// partially committed scene or adding another history step.
    pub fn transaction_validated(
        &mut self,
        edit: impl FnOnce(&mut Scene) -> Result<(), SceneError>,
        validate: impl FnOnce(&Scene) -> Result<(), SceneError>,
    ) -> Result<(), SceneError> {
        self.reconcile_external();
        let before = self.snapshot();
        self.engine.transaction_validated(edit, validate)?;
        // A failed input/derived-layout commit must not end playback or discard
        // its Redo branch. On success, record its last frame before the edit.
        self.finish_playback_before_edit(&before);
        self.finish_edit_session_before_edit(&before);
        self.observed_revision = self.engine.revision();
        self.selection.retain_existing(self.engine.scene());
        if self.pending.is_none() {
            self.record(before);
        }
        Ok(())
    }
    /// One-shot path placement shares normal history, grouped edits and failure
    /// rollback. This does not install a follower binding or start playback.
    pub fn place_on_path(
        &mut self,
        target: ElementHandle,
        path: ElementHandle,
        options: gpui_flow_engine::PathPlacement,
    ) -> Result<(), SceneError> {
        self.reconcile_external();
        let before = self.snapshot();
        self.engine.place_on_path(target, path, options)?;
        self.finish_playback_before_edit(&before);
        self.finish_edit_session_before_edit(&before);
        self.observed_revision = self.engine.revision();
        self.selection.retain_existing(self.engine.scene());
        if self.pending.is_none() {
            self.record(before);
        }
        Ok(())
    }
    /// Replace local Path/Pen data as one undoable edit, or join the current
    /// generic edit transaction. Parse SVG separately with `Path::from_svg`.
    pub fn set_path_data(
        &mut self,
        handle: ElementHandle,
        path: gpui_flow_scene::display::Path,
    ) -> Result<(), SceneError> {
        path.to_svg().map_err(EngineError::Property)?;
        let value = serde_json::to_value(path).map_err(|e| EngineError::Property(e.to_string()))?;
        self.set_property(handle, PropertyKey::PathData, value)
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
        self.finish_playback_before_edit(&before);
        self.finish_edit_session_before_edit(&before);
        self.observed_revision = self.engine.revision();
        self.selection.retain_existing(self.engine.scene());
        if self.pending.is_none() {
            self.record(before);
        }
        Ok(())
    }
    /// Direct engine writes outside an owned session/playback remain a history
    /// boundary. During ownership, preserve the last owned snapshot and external
    /// edit separately; cancellation can never overwrite the external write.
    pub fn reconcile_external(&mut self) -> bool {
        self.engine.set_stroke_view_scale(self.viewport.zoom);
        // Engine replacement can reuse a revision number; identity is stronger
        // than revision equality. A document reset always drops old history.
        if self.edit_session.as_ref().is_some_and(|edit| {
            edit.token.engine != self.engine.engine_id()
                || edit.document_epoch != self.engine.document_epoch()
        }) {
            self.edit_session = None;
            self.pending = None;
            self.playback = None;
            self.undo.clear();
            self.redo.clear();
            self.selection.retain_existing(self.engine.scene());
            self.observed_revision = self.engine.revision();
            return true;
        }
        if self.observed_revision == self.engine.revision() {
            return false;
        }
        self.pending = None;
        if let Some(edit) = self.edit_session.take() {
            if edit.baseline.scene != edit.last_owned.scene {
                self.push_undo(edit.baseline);
            }
            if edit.last_owned.scene != *self.scene() {
                self.push_undo(edit.last_owned);
            }
        } else if let Some(playback) = self.playback.take() {
            if self.engine.document_epoch() != playback.document_epoch {
                // An explicit new-document load remains a history boundary.
                self.undo.clear();
                self.redo.clear();
            } else {
                // Preserve the last owned animation frame and the subsequent
                // external edit as distinct undo steps, never restore over it.
                if playback.baseline.scene != playback.last_frame.scene {
                    self.push_undo(playback.baseline);
                }
                if playback.last_frame.scene != *self.scene() {
                    self.push_undo(playback.last_frame);
                }
            }
        } else {
            self.undo.clear();
            self.redo.clear();
        }
        self.selection.retain_existing(self.engine.scene());
        self.observed_revision = self.engine.revision();
        true
    }
    /// Starts one undoable gesture. Returns false if a gesture already exists.
    /// An owned session or playback is first sealed as its own undo step.
    pub fn begin_edit(&mut self) -> bool {
        self.reconcile_external();
        self.finish_active_playback();
        self.finish_active_edit_session();
        if self.pending.is_some() {
            return false;
        }
        self.pending = Some(self.snapshot());
        true
    }
    /// Any user edit is active, including an owned edit session.
    pub fn editing(&self) -> bool {
        self.pending.is_some() || self.edit_session.is_some()
    }
    /// An unowned begin_edit gesture is active. Owned sessions have their own
    /// token-only finish/cancel methods; generic commit/cancel cannot steal them.
    pub fn gesture_editing(&self) -> bool {
        self.pending.is_some()
    }
    pub fn commit_edit(&mut self) {
        self.reconcile_external();
        if let Some(before) = self.pending.take() {
            self.record(before);
        }
    }
    /// Drop an invalidated generic gesture without replaying its old document
    /// or recording it in a different engine/document. Owned sessions are not
    /// affected. Input adapters use this only after identity replacement.
    pub fn abandon_edit(&mut self) {
        self.reconcile_external();
        self.pending = None;
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
        self.finish_active_playback();
        self.finish_active_edit_session();
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
        self.finish_active_playback();
        self.finish_active_edit_session();
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
    /// Rotate around each selected object's local origin, or the sole selected
    /// object's explicit editor pivot. Hierarchy and multi-selection behavior
    /// are preserved; pointer rotation supplies its separate world-space delta.
    pub fn rotate_selection(&mut self, degrees: f32) -> Result<(), SceneError> {
        let ids = self.selection.roots(self.scene());
        let single = self.selection.ids.len() == 1;
        self.transaction(|scene| {
            for id in ids {
                if !Self::locked(scene, &id) {
                    scene.update_object(&id, |object| {
                        let rotation = Affine::rotation(degrees);
                        object.transform = object.transform
                            * if single {
                                object
                                    .pivot
                                    .map_or(rotation, |pivot| rotation.around(pivot))
                            } else {
                                rotation
                            };
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
