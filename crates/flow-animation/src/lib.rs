//! Deterministic opt-in animation using live instance handles and atomic mutations.
#![doc = include_str!("../README.md")]
mod tween;
pub use tween::*;

use gpui_flow_editor::{PlaybackToken, SceneEditor};
use gpui_flow_engine::{
    ChangeOrigin, ChangeSet, ElementHandle, EngineError, InstanceState, PropertyKey, SceneEngine,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

type PropertyUpdates = Vec<(ElementHandle, PropertyKey, Value)>;

pub const MAX_ANIMATIONS: usize = 256;
pub const MAX_TWEENS_PER_ANIMATION: usize = 32;

#[derive(Debug, Clone, PartialEq)]
pub enum AnimationError {
    Engine(EngineError),
    WrongEngine,
    UnknownAnimation,
    InvalidState,
    EditorSessionEnded,
    InvalidInput(String),
    Conflict,
    Capacity,
}
impl From<EngineError> for AnimationError {
    fn from(error: EngineError) -> Self {
        Self::Engine(error)
    }
}
impl std::fmt::Display for AnimationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Engine(error) => error.fmt(f),
            Self::WrongEngine => write!(f, "animation controller belongs to another scene engine"),
            Self::UnknownAnimation => write!(f, "unknown or removed animation"),
            Self::InvalidState => write!(f, "animation state does not allow this operation"),
            Self::EditorSessionEnded => {
                write!(f, "editor playback ended or was interrupted by an edit")
            }
            Self::InvalidInput(reason) => write!(f, "invalid animation: {reason}"),
            Self::Conflict => write!(f, "an active animation already owns this target property"),
            Self::Capacity => write!(f, "animation capacity exhausted; remove completed records"),
        }
    }
}
impl std::error::Error for AnimationError {}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct AnimationId {
    engine: u64,
    controller: u64,
    sequence: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelReason {
    User,
    TargetUnavailable,
    Overridden,
    EditorInterrupted,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimationStatus {
    Playing,
    Paused,
    Completed,
    Cancelled(CancelReason),
}
impl AnimationStatus {
    fn active(self) -> bool {
        matches!(self, Self::Playing | Self::Paused)
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnimationSpec {
    pub duration_ms: u64,
    pub easing: Easing,
    pub tweens: Vec<Tween>,
}
impl AnimationSpec {
    pub fn new(duration_ms: u64, tweens: Vec<Tween>) -> Self {
        Self {
            duration_ms,
            easing: Easing::Linear,
            tweens,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct AnimationInfo {
    pub id: AnimationId,
    pub target: ElementHandle,
    pub status: AnimationStatus,
    pub elapsed: Duration,
    pub duration: Duration,
}
#[derive(Debug, Clone)]
struct Track {
    property: TweenProperty,
    from: TweenValue,
    to: TweenValue,
    last: TweenValue,
}
#[derive(Debug, Clone)]
struct Animation {
    info: AnimationInfo,
    easing: Easing,
    tracks: Vec<Track>,
}
#[derive(Debug, Clone)]
pub struct TickReport {
    pub changes: ChangeSet,
    pub advanced: Vec<AnimationId>,
    pub completed: Vec<AnimationId>,
    pub cancelled: Vec<AnimationId>,
}
/// Bounded runtime records, bound to exactly one engine. No callbacks are installed
/// in the engine. Every mutating/query operation with an engine synchronizes target
/// lifetime and external overrides. `tick` never gives an old ID a new target.
struct TickPlan {
    next: BTreeMap<AnimationId, Animation>,
    updates: PropertyUpdates,
    advanced: Vec<AnimationId>,
    completed: Vec<AnimationId>,
    cancelled: Vec<AnimationId>,
}
pub struct AnimationController {
    engine: u64,
    id: u64,
    next: u64,
    animations: BTreeMap<AnimationId, Animation>,
}
static NEXT_CONTROLLER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
impl AnimationController {
    pub fn new(engine: &SceneEngine) -> Self {
        let id = NEXT_CONTROLLER
            .fetch_update(
                std::sync::atomic::Ordering::Relaxed,
                std::sync::atomic::Ordering::Relaxed,
                |value| value.checked_add(1),
            )
            .expect("animation controller identity exhausted");
        Self {
            engine: engine.engine_id(),
            id,
            next: 0,
            animations: BTreeMap::new(),
        }
    }
    fn check_engine(&self, engine: &SceneEngine) -> Result<(), AnimationError> {
        if engine.engine_id() == self.engine {
            Ok(())
        } else {
            Err(AnimationError::WrongEngine)
        }
    }
    /// Synchronize instance destruction/detach/reset and host writes. Cancellation
    /// keeps current document values; it never restores or recreates an instance.
    pub fn sync(&mut self, engine: &SceneEngine) -> Result<Vec<AnimationId>, AnimationError> {
        self.check_engine(engine)?;
        let mut cancelled = Vec::new();
        for animation in self
            .animations
            .values_mut()
            .filter(|animation| !matches!(animation.info.status, AnimationStatus::Cancelled(_)))
        {
            let target = animation.info.target;
            let reason = if !engine
                .instance(target)
                .is_ok_and(|instance| instance.state == InstanceState::Attached)
            {
                Some(CancelReason::TargetUnavailable)
            } else if animation.info.status.active()
                && animation.tracks.iter().any(|track| {
                    tween::read(engine, target, &track.property)
                        .map_or(true, |value| value != track.last)
                })
            {
                Some(CancelReason::Overridden)
            } else {
                None
            };
            if let Some(reason) = reason {
                animation.info.status = AnimationStatus::Cancelled(reason);
                cancelled.push(animation.info.id);
            }
        }
        Ok(cancelled)
    }
    pub fn len(&self) -> usize {
        self.animations.len()
    }
    pub fn is_empty(&self) -> bool {
        self.animations.is_empty()
    }
    /// Whether the last synchronized state has a playing animation.
    pub fn is_running(&self) -> bool {
        self.animations
            .values()
            .any(|animation| animation.info.status == AnimationStatus::Playing)
    }
    pub fn info(
        &mut self,
        engine: &SceneEngine,
        id: AnimationId,
    ) -> Result<AnimationInfo, AnimationError> {
        self.sync(engine)?;
        self.animations
            .get(&id)
            .map(|animation| animation.info.clone())
            .ok_or(AnimationError::UnknownAnimation)
    }
    fn check_conflict(
        &self,
        target: ElementHandle,
        properties: &[TweenProperty],
        except: Option<AnimationId>,
    ) -> Result<(), AnimationError> {
        if self.animations.values().any(|animation| {
            Some(animation.info.id) != except
                && animation.info.target == target
                && animation.info.status.active()
                && animation
                    .tracks
                    .iter()
                    .any(|track| properties.contains(&track.property))
        }) {
            Err(AnimationError::Conflict)
        } else {
            Ok(())
        }
    }
    /// Capture current effective inputs, validate the endpoint, then reserve
    /// property ownership. Adding an animation does not mutate the document.
    pub fn animate(
        &mut self,
        engine: &SceneEngine,
        target: ElementHandle,
        spec: AnimationSpec,
    ) -> Result<AnimationId, AnimationError> {
        self.sync(engine)?;
        if self.animations.len() >= MAX_ANIMATIONS {
            return Err(AnimationError::Capacity);
        }
        if spec.duration_ms == 0
            || spec.tweens.is_empty()
            || spec.tweens.len() > MAX_TWEENS_PER_ANIMATION
        {
            return Err(AnimationError::InvalidInput(
                "positive duration and 1..=32 tweens required".into(),
            ));
        }
        if engine.instance(target)?.state != InstanceState::Attached {
            return Err(AnimationError::Engine(EngineError::DetachedHandle));
        }
        let mut seen = BTreeSet::new();
        let mut tracks = Vec::new();
        let mut endpoint = Vec::new();
        for tween in spec.tweens {
            if !seen.insert(tween.property.clone()) {
                return Err(AnimationError::InvalidInput("duplicate property".into()));
            }
            let from = tween::read(engine, target, &tween.property)?;
            tween::validate_pair(&tween.property, &from, &tween.to)?;
            let (key, value) = tween::update(engine, target, &tween.property, &tween.to)?;
            endpoint.push((target, key, value));
            tracks.push(Track {
                property: tween.property,
                from: from.clone(),
                to: tween.to,
                last: from,
            });
        }
        self.check_conflict(target, &seen.into_iter().collect::<Vec<_>>(), None)?;
        engine.validate_properties_batch_with_origin(ChangeOrigin::Animation, endpoint)?;
        let next = self.next.checked_add(1).ok_or(AnimationError::Capacity)?;
        let id = AnimationId {
            engine: self.engine,
            controller: self.id,
            sequence: next,
        };
        self.animations.insert(
            id,
            Animation {
                info: AnimationInfo {
                    id,
                    target,
                    status: AnimationStatus::Playing,
                    elapsed: Duration::ZERO,
                    duration: Duration::from_millis(spec.duration_ms),
                },
                easing: spec.easing,
                tracks,
            },
        );
        self.next = next;
        Ok(id)
    }
    pub fn pause(&mut self, engine: &SceneEngine, id: AnimationId) -> Result<(), AnimationError> {
        self.sync(engine)?;
        let animation = self
            .animations
            .get_mut(&id)
            .ok_or(AnimationError::UnknownAnimation)?;
        if !animation.info.status.active() {
            return Err(AnimationError::InvalidState);
        }
        animation.info.status = AnimationStatus::Paused;
        Ok(())
    }
    pub fn resume(&mut self, engine: &SceneEngine, id: AnimationId) -> Result<(), AnimationError> {
        self.sync(engine)?;
        let animation = self
            .animations
            .get_mut(&id)
            .ok_or(AnimationError::UnknownAnimation)?;
        if !animation.info.status.active() {
            return Err(AnimationError::InvalidState);
        }
        animation.info.status = AnimationStatus::Playing;
        Ok(())
    }
    /// Stop and retain current values. Use seek before cancel to choose an endpoint.
    pub fn cancel(&mut self, engine: &SceneEngine, id: AnimationId) -> Result<(), AnimationError> {
        self.sync(engine)?;
        let animation = self
            .animations
            .get_mut(&id)
            .ok_or(AnimationError::UnknownAnimation)?;
        if animation.info.status.active() {
            animation.info.status = AnimationStatus::Cancelled(CancelReason::User);
        }
        Ok(())
    }
    pub fn remove(&mut self, id: AnimationId) -> bool {
        self.animations.remove(&id).is_some()
    }
    /// Low-level engine playback. For an editor, use `tick_editor` with an
    /// owned playback token so existing Undo/Redo history remains intact.
    pub fn tick(
        &mut self,
        engine: &mut SceneEngine,
        delta: Duration,
    ) -> Result<TickReport, AnimationError> {
        let mut plan = self.plan_tick(engine, delta)?;
        let changes = engine
            .set_properties_batch(ChangeOrigin::Animation, std::mem::take(&mut plan.updates))?;
        Ok(self.commit_tick(engine, plan, changes))
    }
    fn plan_tick(
        &mut self,
        engine: &SceneEngine,
        delta: Duration,
    ) -> Result<TickPlan, AnimationError> {
        let cancelled = self.sync(engine)?;
        let mut next = self.animations.clone();
        let mut updates = Vec::new();
        let mut advanced = Vec::new();
        let mut completed = Vec::new();
        for animation in next
            .values_mut()
            .filter(|animation| animation.info.status == AnimationStatus::Playing)
        {
            let elapsed = animation
                .info
                .elapsed
                .saturating_add(delta)
                .min(animation.info.duration);
            if elapsed != animation.info.elapsed {
                advanced.push(animation.info.id);
            }
            sample(engine, animation, elapsed, &mut updates)?;
            if elapsed == animation.info.duration {
                animation.info.status = AnimationStatus::Completed;
                completed.push(animation.info.id);
            }
        }
        Ok(TickPlan {
            next,
            updates,
            advanced,
            completed,
            cancelled,
        })
    }
    fn commit_tick(
        &mut self,
        engine: &SceneEngine,
        mut plan: TickPlan,
        changes: ChangeSet,
    ) -> TickReport {
        for animation in plan
            .next
            .values_mut()
            .filter(|animation| !matches!(animation.info.status, AnimationStatus::Cancelled(_)))
        {
            if !refresh_values(engine, animation) {
                plan.cancelled.push(animation.info.id);
                plan.completed.retain(|id| *id != animation.info.id);
            }
        }
        self.animations = plan.next;
        TickReport {
            changes,
            advanced: plan.advanced,
            completed: plan.completed,
            cancelled: plan.cancelled,
        }
    }
    fn ensure_editor_session(
        &mut self,
        editor: &mut SceneEditor,
        token: PlaybackToken,
    ) -> Result<(), AnimationError> {
        self.check_engine(&editor.engine)?;
        if !editor.is_playback_active(token) {
            for animation in self
                .animations
                .values_mut()
                .filter(|animation| animation.info.status.active())
            {
                animation.info.status = AnimationStatus::Cancelled(CancelReason::EditorInterrupted);
            }
            return Err(AnimationError::EditorSessionEnded);
        }
        Ok(())
    }
    /// Editor-owned frames do not create Undo entries. Finish commits one whole
    /// playback step; cancel restores its baseline and the original Undo/Redo.
    pub fn tick_editor(
        &mut self,
        editor: &mut SceneEditor,
        token: PlaybackToken,
        delta: Duration,
    ) -> Result<TickReport, AnimationError> {
        self.ensure_editor_session(editor, token)?;
        let mut plan = self.plan_tick(&editor.engine, delta)?;
        let changes = editor.apply_playback_properties(token, std::mem::take(&mut plan.updates))?;
        Ok(self.commit_tick(&editor.engine, plan, changes))
    }
    /// Seek a low-level engine animation. Editor hosts should use seek_editor.
    pub fn seek(
        &mut self,
        engine: &mut SceneEngine,
        id: AnimationId,
        position: Duration,
    ) -> Result<ChangeSet, AnimationError> {
        let (mut animation, updates) = self.plan_seek(engine, id, position)?;
        let changes = engine.set_properties_batch(ChangeOrigin::Animation, updates)?;
        refresh_values(engine, &mut animation);
        self.animations.insert(id, animation);
        Ok(changes)
    }
    pub fn seek_editor(
        &mut self,
        editor: &mut SceneEditor,
        token: PlaybackToken,
        id: AnimationId,
        position: Duration,
    ) -> Result<ChangeSet, AnimationError> {
        self.ensure_editor_session(editor, token)?;
        let (mut animation, updates) = self.plan_seek(&editor.engine, id, position)?;
        let changes = editor.apply_playback_properties(token, updates)?;
        refresh_values(&editor.engine, &mut animation);
        self.animations.insert(id, animation);
        Ok(changes)
    }
    fn plan_seek(
        &mut self,
        engine: &SceneEngine,
        id: AnimationId,
        position: Duration,
    ) -> Result<(Animation, PropertyUpdates), AnimationError> {
        self.sync(engine)?;
        let mut animation = self
            .animations
            .get(&id)
            .ok_or(AnimationError::UnknownAnimation)?
            .clone();
        if matches!(animation.info.status, AnimationStatus::Cancelled(_)) {
            return Err(AnimationError::InvalidState);
        }
        self.check_conflict(
            animation.info.target,
            &animation
                .tracks
                .iter()
                .map(|track| track.property.clone())
                .collect::<Vec<_>>(),
            Some(id),
        )?;
        let position = position.min(animation.info.duration);
        let mut updates = Vec::new();
        sample(engine, &mut animation, position, &mut updates)?;
        animation.info.status = if position == animation.info.duration {
            AnimationStatus::Completed
        } else if animation.info.status == AnimationStatus::Completed {
            AnimationStatus::Paused
        } else {
            animation.info.status
        };
        Ok((animation, updates))
    }
    /// Stop at the current pose and make the entire playback one Undo step.
    pub fn finish_editor(
        &mut self,
        editor: &mut SceneEditor,
        token: PlaybackToken,
    ) -> Result<(), AnimationError> {
        self.ensure_editor_session(editor, token)?;
        if !editor.finish_playback(token) {
            return Err(AnimationError::EditorSessionEnded);
        }
        for animation in self
            .animations
            .values_mut()
            .filter(|animation| animation.info.status.active())
        {
            animation.info.status = AnimationStatus::Cancelled(CancelReason::User);
        }
        Ok(())
    }
    /// Restore the pre-playback document and keep earlier Undo/Redo entries.
    /// An external edit ends ownership, so stale cancellation cannot overwrite it.
    pub fn cancel_editor(
        &mut self,
        editor: &mut SceneEditor,
        token: PlaybackToken,
    ) -> Result<(), AnimationError> {
        self.ensure_editor_session(editor, token)?;
        if !editor.cancel_playback(token) {
            return Err(AnimationError::EditorSessionEnded);
        }
        for animation in self
            .animations
            .values_mut()
            .filter(|animation| !matches!(animation.info.status, AnimationStatus::Cancelled(_)))
        {
            animation.info.status = AnimationStatus::Cancelled(CancelReason::User);
        }
        Ok(())
    }
}
fn sample(
    engine: &SceneEngine,
    animation: &mut Animation,
    elapsed: Duration,
    updates: &mut PropertyUpdates,
) -> Result<(), AnimationError> {
    let t = animation
        .easing
        .sample(elapsed.as_secs_f64() / animation.info.duration.as_secs_f64());
    for track in &mut animation.tracks {
        let value = tween::interpolate(&track.from, &track.to, t)?;
        if value != tween::read(engine, animation.info.target, &track.property)? {
            let (key, data) =
                tween::update(engine, animation.info.target, &track.property, &value)?;
            updates.push((animation.info.target, key, data));
            track.last = value;
        }
    }
    animation.info.elapsed = elapsed;
    Ok(())
}

fn refresh_values(engine: &SceneEngine, animation: &mut Animation) -> bool {
    for track in &mut animation.tracks {
        match tween::read(engine, animation.info.target, &track.property) {
            Ok(value) => track.last = value,
            Err(_) => {
                animation.info.status = AnimationStatus::Cancelled(CancelReason::TargetUnavailable);
                return false;
            }
        }
    }
    true
}
