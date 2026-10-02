//! Explicit single-target motion binding over the engine's validated placement.
use crate::{
    AnimationError, AnimationTiming, Easing, PlaybackRate, TimingPhase, TimingSample, TweenValue,
    timing::Schedule,
};
use gpui_flow_editor::{PlaybackToken, SceneEditor};
use gpui_flow_engine::{
    ChangeOrigin, ChangeSet, ElementHandle, EngineError, InstanceState, PathPlacement, SceneEngine,
};
use gpui_flow_scene::{Affine, DisplayId, Point, Scene, SceneError, Shape};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Local arc-distance endpoints, not fractions or world-space distances.
/// Registration captures the original pose without writing the document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MotionPathSpec {
    pub duration_ms: u64,
    pub from_distance: f64,
    pub to_distance: f64,
    pub anchor: Point,
    /// World tangent offset in degrees; None preserves the world linear matrix.
    pub rotation: Option<f32>,
    pub easing: Easing,
    pub timing: AnimationTiming,
}
impl MotionPathSpec {
    pub fn new(duration_ms: u64, from_distance: f64, to_distance: f64) -> Self {
        Self {
            duration_ms,
            from_distance,
            to_distance,
            anchor: Point::ZERO,
            rotation: None,
            easing: Easing::Linear,
            timing: AnimationTiming::default(),
        }
    }
    fn placement(&self, distance: f64) -> PathPlacement {
        PathPlacement {
            distance,
            anchor: self.anchor,
            rotation: self.rotation,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionPathCancelReason {
    User,
    TargetUnavailable,
    SourceUnavailable,
    TargetOverridden,
    SourceChanged,
    TargetParentChanged,
    EditorInterrupted,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionPathStatus {
    Playing,
    Paused,
    Completed,
    Cancelled(MotionPathCancelReason),
}
impl MotionPathStatus {
    fn active(self) -> bool {
        matches!(self, Self::Playing | Self::Paused)
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct MotionPathInfo {
    pub target: ElementHandle,
    pub path: ElementHandle,
    pub status: MotionPathStatus,
    pub elapsed: Duration,
    pub duration: Duration,
    /// Last requested local distance, before endpoint clamping. None means the
    /// captured original pose (initial registration or initial delay).
    pub distance: Option<f64>,
}
#[derive(Debug, Clone)]
pub struct MotionPathTick {
    pub changes: ChangeSet,
    pub info: MotionPathInfo,
}
#[derive(Debug, Clone, PartialEq)]
struct Guard {
    handle: ElementHandle,
    epoch: u64,
    transform: Affine,
}
struct Frame {
    timing: TimingSample,
    remainder: u32,
    distance: Option<f64>,
}

/// One bounded binding, with no timer, callback, scene-stored dependency or
/// implicit global property registry. Every frame is one atomic placement.
/// Source edits cancel; create a new binding explicitly to follow revised data.
/// Different bindings/controllers must be host-orchestrated: an observed write
/// to this binding's target transform cancels it instead of being overwritten.
/// Editor and bare-engine bindings cannot switch mutation routes or tokens.
pub struct MotionPathAnimation {
    engine: u64,
    editor: Option<PlaybackToken>,
    spec: MotionPathSpec,
    schedule: Schedule,
    info: MotionPathInfo,
    remainder: u32,
    target_epoch: u64,
    target_id: DisplayId,
    path_id: DisplayId,
    source_shape: Shape,
    source_chain: Vec<Guard>,
    parent_chain: Vec<Guard>,
    source_world: Affine,
    parent_world: Affine,
    original: Affine,
    last: Affine,
}
impl MotionPathAnimation {
    pub fn new(
        engine: &SceneEngine,
        target: ElementHandle,
        path: ElementHandle,
        spec: MotionPathSpec,
    ) -> Result<Self, AnimationError> {
        if !spec.from_distance.is_finite()
            || !spec.to_distance.is_finite()
            || !spec.anchor.is_finite()
            || spec.rotation.is_some_and(|r| !r.is_finite())
        {
            return Err(invalid("motion-path inputs must be finite"));
        }
        let duration = Duration::from_millis(spec.duration_ms);
        let schedule = spec.timing.compile(duration)?;
        let target_instance = engine.instance(target)?;
        let path_instance = engine.instance(path)?;
        if target_instance.state != InstanceState::Attached
            || path_instance.state != InstanceState::Attached
        {
            return Err(EngineError::DetachedHandle.into());
        }
        let target_id = target_instance.id;
        let path_id = path_instance.id;
        let source_chain = chain(engine, Some(path_id.clone()))?;
        if source_chain.iter().any(|guard| guard.handle == target) {
            return Err(invalid(
                "target cannot be its path or an ancestor of its path",
            ));
        }
        let target_node = engine.scene().get(&target_id).expect("attached");
        let parent_chain = chain(engine, target_node.parent.clone())?;
        let parent_world = match &target_node.parent {
            Some(parent) => engine.world_transform(parent)?,
            None => Affine::IDENTITY,
        };
        if parent_world.inverse().is_none() {
            return Err(invalid("target parent transform is singular"));
        }
        let source_world = engine.world_transform(&path_id)?;
        let measure = engine.measure_path(path)?;
        for distance in [spec.from_distance, spec.to_distance] {
            measure
                .sample(distance)
                .map_err(invalid)?
                .ok_or_else(|| invalid("path has no measurable geometry"))?
                .transformed(source_world)
                .map_err(invalid)?;
        }
        let world = engine.world_transform(&target_id)?;
        if spec.rotation.is_some() && world.a == 0. && world.b == 0. {
            return Err(invalid("target world +x axis is collapsed"));
        }
        let original = target_node.object.transform;
        Ok(Self {
            engine: engine.engine_id(),
            editor: None,
            spec,
            schedule,
            info: MotionPathInfo {
                target,
                path,
                status: MotionPathStatus::Playing,
                elapsed: Duration::ZERO,
                duration,
                distance: None,
            },
            remainder: 0,
            target_epoch: target_instance.attachment_epoch,
            target_id,
            path_id,
            source_shape: engine.object(path)?.shape.clone(),
            source_chain,
            parent_chain,
            source_world,
            parent_world,
            original,
            last: original,
        })
    }
    /// Bind to this exact existing playback session. A replacement token never
    /// authorizes an old animation, even if no stale tick was attempted in between.
    pub fn new_editor(
        editor: &mut SceneEditor,
        token: PlaybackToken,
        target: ElementHandle,
        path: ElementHandle,
        spec: MotionPathSpec,
    ) -> Result<Self, AnimationError> {
        if !editor.is_playback_active(token) {
            return Err(AnimationError::EditorSessionEnded);
        }
        let mut animation = Self::new(&editor.engine, target, path, spec)?;
        animation.editor = Some(token);
        Ok(animation)
    }
    fn check_engine(&self, engine: &SceneEngine) -> Result<(), AnimationError> {
        if engine.engine_id() != self.engine {
            Err(AnimationError::WrongEngine)
        } else {
            Ok(())
        }
    }
    fn check_bare(&self, engine: &SceneEngine) -> Result<(), AnimationError> {
        self.check_engine(engine)?;
        if self.editor.is_some() {
            Err(AnimationError::InvalidState)
        } else {
            Ok(())
        }
    }
    /// Retire stale identities or edited dependencies without restoring anything.
    /// Both active and completed bindings are checked before a possible rewind.
    pub fn sync(&mut self, engine: &SceneEngine) -> Result<MotionPathStatus, AnimationError> {
        self.check_engine(engine)?;
        if matches!(self.info.status, MotionPathStatus::Cancelled(_)) {
            return Ok(self.info.status);
        }
        let reason = if !attached_epoch(engine, self.info.target, self.target_epoch) {
            Some(MotionPathCancelReason::TargetUnavailable)
        } else if !attached_epoch(engine, self.info.path, self.source_chain[0].epoch) {
            Some(MotionPathCancelReason::SourceUnavailable)
        } else if engine.object(self.info.target)?.transform != self.last {
            Some(MotionPathCancelReason::TargetOverridden)
        } else if engine.object(self.info.path)?.shape != self.source_shape
            || chain(engine, Some(self.path_id.clone()))? != self.source_chain
        {
            Some(MotionPathCancelReason::SourceChanged)
        } else if chain(
            engine,
            engine
                .scene()
                .get(&self.target_id)
                .expect("attached")
                .parent
                .clone(),
        )? != self.parent_chain
        {
            Some(MotionPathCancelReason::TargetParentChanged)
        } else {
            None
        };
        if let Some(reason) = reason {
            self.info.status = MotionPathStatus::Cancelled(reason);
        }
        Ok(self.info.status)
    }
    pub fn info(&mut self, engine: &SceneEngine) -> Result<MotionPathInfo, AnimationError> {
        self.sync(engine)?;
        Ok(self.info.clone())
    }
    pub fn timing_sample(&mut self, engine: &SceneEngine) -> Result<TimingSample, AnimationError> {
        self.sync(engine)?;
        Ok(self.schedule.sample(self.info.elapsed))
    }
    pub fn pause(&mut self, engine: &SceneEngine) -> Result<(), AnimationError> {
        self.sync(engine)?;
        if !self.info.status.active() {
            return Err(AnimationError::InvalidState);
        }
        self.info.status = MotionPathStatus::Paused;
        Ok(())
    }
    pub fn resume(&mut self, engine: &SceneEngine) -> Result<(), AnimationError> {
        self.sync(engine)?;
        if !self.info.status.active() {
            return Err(AnimationError::InvalidState);
        }
        self.info.status = MotionPathStatus::Playing;
        Ok(())
    }
    pub fn set_playback_rate(
        &mut self,
        engine: &SceneEngine,
        rate: PlaybackRate,
    ) -> Result<(), AnimationError> {
        self.sync(engine)?;
        if !self.info.status.active() {
            return Err(AnimationError::InvalidState);
        }
        self.schedule.timing.speed = rate;
        self.remainder = 0;
        Ok(())
    }
    /// Bare-engine cancellation keeps the current pose. For rollback with history
    /// preservation, create an editor binding and use cancel_editor instead.
    pub fn cancel(&mut self, engine: &SceneEngine) -> Result<(), AnimationError> {
        self.check_bare(engine)?;
        self.sync(engine)?;
        if !matches!(self.info.status, MotionPathStatus::Cancelled(_)) {
            self.info.status = MotionPathStatus::Cancelled(MotionPathCancelReason::User);
        }
        Ok(())
    }
    pub fn tick(
        &mut self,
        engine: &mut SceneEngine,
        delta: Duration,
    ) -> Result<MotionPathTick, AnimationError> {
        self.check_bare(engine)?;
        self.sync(engine)?;
        if self.info.status != MotionPathStatus::Playing {
            return Ok(self.report(empty_change(engine)));
        }
        let (elapsed, remainder) =
            self.schedule
                .advance(self.info.elapsed, self.remainder, delta)?;
        let frame = self.frame(elapsed, remainder)?;
        let changes = self.apply(engine, &frame)?;
        self.commit(engine, frame, false);
        Ok(self.report(changes))
    }
    /// Absolute logical time including delay and repeats; speed does not scale it.
    /// A completed rewind becomes paused; cancelled bindings cannot restart.
    pub fn seek(
        &mut self,
        engine: &mut SceneEngine,
        position: Duration,
    ) -> Result<ChangeSet, AnimationError> {
        self.check_bare(engine)?;
        self.sync(engine)?;
        self.ensure_seek()?;
        let frame = self.frame(position, 0)?;
        let changes = self.apply(engine, &frame)?;
        self.commit(engine, frame, true);
        Ok(changes)
    }
    fn ensure_seek(&self) -> Result<(), AnimationError> {
        if matches!(self.info.status, MotionPathStatus::Cancelled(_)) {
            Err(AnimationError::InvalidState)
        } else {
            Ok(())
        }
    }
    fn frame(&self, elapsed: Duration, remainder: u32) -> Result<Frame, AnimationError> {
        let timing = self.schedule.sample(elapsed);
        let distance = if timing.phase == TimingPhase::Delay {
            None
        } else {
            let progress = timing.cycle_position.as_secs_f64() / self.info.duration.as_secs_f64();
            let (from, to, progress) = if timing.reversed {
                (
                    self.spec.to_distance,
                    self.spec.from_distance,
                    1. - progress,
                )
            } else {
                (self.spec.from_distance, self.spec.to_distance, progress)
            };
            let TweenValue::Number(value) = TweenValue::Number(from).interpolate(
                &TweenValue::Number(to),
                progress,
                self.spec.easing,
            )?
            else {
                unreachable!("number interpolation")
            };
            Some(value)
        };
        Ok(Frame {
            timing,
            remainder,
            distance,
        })
    }
    fn apply(&self, engine: &mut SceneEngine, frame: &Frame) -> Result<ChangeSet, AnimationError> {
        match frame.distance {
            Some(distance) => Ok(engine.place_on_path_with_origin(
                self.info.target,
                self.info.path,
                self.spec.placement(distance),
                ChangeOrigin::Animation,
            )?),
            None => Ok(engine.transaction_validated_with_origin(
                ChangeOrigin::Animation,
                |scene| scene.update_object(&self.target_id, |o| o.transform = self.original),
                |scene| self.validate_original(scene),
            )?),
        }
    }
    fn validate_original(&self, scene: &Scene) -> Result<(), SceneError> {
        let target = scene
            .get(&self.target_id)
            .ok_or_else(|| SceneError::MissingNode(self.target_id.clone()))?;
        let source = scene
            .get(&self.path_id)
            .ok_or_else(|| SceneError::MissingNode(self.path_id.clone()))?;
        let parent_world = match &target.parent {
            Some(id) => scene.world_transform(id)?,
            None => Affine::IDENTITY,
        };
        if target.object.transform != self.original
            || parent_world != self.parent_world
            || source.object.shape != self.source_shape
            || scene.world_transform(&self.path_id)? != self.source_world
        {
            return Err(SceneError::InvalidGeometry(
                "layout overrode motion-path delay pose or source".into(),
            ));
        }
        Ok(())
    }
    fn commit(&mut self, engine: &SceneEngine, frame: Frame, seek: bool) {
        self.info.elapsed = frame.timing.elapsed;
        self.info.distance = frame.distance;
        self.remainder = frame.remainder;
        // Keep handle identity authoritative even after a successful commit;
        // never resolve a disappeared target again through its document ID.
        let Ok(target) = engine.object(self.info.target) else {
            self.info.status =
                MotionPathStatus::Cancelled(MotionPathCancelReason::TargetUnavailable);
            return;
        };
        self.last = target.transform;
        self.info.status = if frame.timing.phase == TimingPhase::Completed {
            MotionPathStatus::Completed
        } else if seek && self.info.status == MotionPathStatus::Completed {
            MotionPathStatus::Paused
        } else {
            self.info.status
        };
    }
    fn report(&self, changes: ChangeSet) -> MotionPathTick {
        MotionPathTick {
            changes,
            info: self.info.clone(),
        }
    }
    fn ensure_editor(
        &mut self,
        editor: &mut SceneEditor,
        token: PlaybackToken,
    ) -> Result<(), AnimationError> {
        self.check_engine(&editor.engine)?;
        if self.editor != Some(token) {
            return Err(AnimationError::EditorSessionEnded);
        }
        if !editor.is_playback_active(token) {
            if !matches!(self.info.status, MotionPathStatus::Cancelled(_)) {
                self.info.status =
                    MotionPathStatus::Cancelled(MotionPathCancelReason::EditorInterrupted);
            }
            return Err(AnimationError::EditorSessionEnded);
        }
        Ok(())
    }
    fn apply_editor(
        &self,
        editor: &mut SceneEditor,
        token: PlaybackToken,
        frame: &Frame,
    ) -> Result<ChangeSet, AnimationError> {
        match frame.distance {
            Some(distance) => Ok(editor.apply_playback_path_placement(
                token,
                self.info.target,
                self.info.path,
                self.spec.placement(distance),
            )?),
            None => Ok(editor.apply_playback_transaction_validated(
                token,
                |scene| scene.update_object(&self.target_id, |o| o.transform = self.original),
                |scene| self.validate_original(scene),
            )?),
        }
    }
    pub fn tick_editor(
        &mut self,
        editor: &mut SceneEditor,
        token: PlaybackToken,
        delta: Duration,
    ) -> Result<MotionPathTick, AnimationError> {
        self.ensure_editor(editor, token)?;
        self.sync(&editor.engine)?;
        if self.info.status != MotionPathStatus::Playing {
            return Ok(self.report(empty_change(&editor.engine)));
        }
        let (elapsed, remainder) =
            self.schedule
                .advance(self.info.elapsed, self.remainder, delta)?;
        let frame = self.frame(elapsed, remainder)?;
        let changes = self.apply_editor(editor, token, &frame)?;
        self.commit(&editor.engine, frame, false);
        Ok(self.report(changes))
    }
    pub fn seek_editor(
        &mut self,
        editor: &mut SceneEditor,
        token: PlaybackToken,
        position: Duration,
    ) -> Result<ChangeSet, AnimationError> {
        self.ensure_editor(editor, token)?;
        self.sync(&editor.engine)?;
        self.ensure_seek()?;
        let frame = self.frame(position, 0)?;
        let changes = self.apply_editor(editor, token, &frame)?;
        self.commit(&editor.engine, frame, true);
        Ok(changes)
    }
    /// Seal the entire existing PlaybackToken session as at most one Undo entry.
    /// This ends the session for all animations sharing that token.
    pub fn finish_editor(
        &mut self,
        editor: &mut SceneEditor,
        token: PlaybackToken,
    ) -> Result<(), AnimationError> {
        self.ensure_editor(editor, token)?;
        if !editor.finish_playback(token) {
            return Err(AnimationError::EditorSessionEnded);
        }
        if self.info.status.active() {
            self.info.status = MotionPathStatus::Cancelled(MotionPathCancelReason::User);
        }
        Ok(())
    }
    /// Restore the session baseline and prior Undo/Redo branch. External edits
    /// revoke the original token first, so stale cancellation never restores it.
    pub fn cancel_editor(
        &mut self,
        editor: &mut SceneEditor,
        token: PlaybackToken,
    ) -> Result<(), AnimationError> {
        self.ensure_editor(editor, token)?;
        if !editor.cancel_playback(token) {
            return Err(AnimationError::EditorSessionEnded);
        }
        self.info.status = MotionPathStatus::Cancelled(MotionPathCancelReason::User);
        Ok(())
    }
}
fn invalid(reason: impl Into<String>) -> AnimationError {
    AnimationError::InvalidInput(reason.into())
}
fn attached_epoch(engine: &SceneEngine, handle: ElementHandle, epoch: u64) -> bool {
    engine
        .instance(handle)
        .is_ok_and(|i| i.state == InstanceState::Attached && i.attachment_epoch == epoch)
}
fn chain(engine: &SceneEngine, mut id: Option<DisplayId>) -> Result<Vec<Guard>, AnimationError> {
    let mut guards = Vec::new();
    while let Some(current) = id {
        let node = engine
            .scene()
            .get(&current)
            .ok_or_else(|| EngineError::Scene(SceneError::MissingNode(current.clone())))?;
        let handle = engine.handle(&current).ok_or(EngineError::DetachedHandle)?;
        let instance = engine.instance(handle)?;
        guards.push(Guard {
            handle,
            epoch: instance.attachment_epoch,
            transform: node.object.transform,
        });
        id = node.parent.clone();
    }
    Ok(guards)
}
fn empty_change(engine: &SceneEngine) -> ChangeSet {
    ChangeSet {
        before_revision: engine.revision(),
        revision: engine.revision(),
        origin: ChangeOrigin::Animation,
        ..ChangeSet::default()
    }
}
