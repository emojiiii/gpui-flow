//! Finite nested property timelines. A whole frame is sampled first and then
//! committed through exactly one engine/editor batch; child controllers never run.
use crate::{
    AnimationClip, AnimationError, AnimationStatus, AnimationTiming, CancelReason, IterationCount,
    PlaybackRate, PropertyUpdates, TimingPhase, TimingSample, Track, TweenProperty, TweenValue,
    sequence, timing::Schedule, tween,
};
use gpui_flow_editor::{PlaybackToken, SceneEditor};
use gpui_flow_engine::{ChangeOrigin, ChangeSet, ElementHandle, InstanceState, SceneEngine};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

pub const MAX_TIMELINES: usize = 64;
/// Includes the root group and every nested group/clip.
pub const MAX_TIMELINE_NODES: usize = 256;
/// Root is depth one.
pub const MAX_TIMELINE_DEPTH: usize = 8;
pub const MAX_TIMELINE_PROPERTIES: usize = 256;
/// Applies to every node's cycle, logical total and parent-clock duration.
pub const MAX_TIMELINE_DURATION: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimelineMode {
    Serial,
    Parallel,
}
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum EndingStyle {
    /// Keep the final sampled pose, including reverse/alternate direction.
    #[default]
    Hold,
    /// Restore this node's captured entry pose on completion.
    Restore,
}

/// Runtime-only target handles are deliberately not serialized.
#[derive(Debug, Clone)]
pub struct TimelineSpec {
    pub mode: TimelineMode,
    pub children: Vec<TimelineChild>,
    pub timing: AnimationTiming,
    pub ending_style: EndingStyle,
}
impl TimelineSpec {
    pub fn serial(children: Vec<TimelineChild>) -> Self {
        Self::new(TimelineMode::Serial, children)
    }
    pub fn parallel(children: Vec<TimelineChild>) -> Self {
        Self::new(TimelineMode::Parallel, children)
    }
    fn new(mode: TimelineMode, children: Vec<TimelineChild>) -> Self {
        Self {
            mode,
            children,
            timing: AnimationTiming::default(),
            ending_style: EndingStyle::Hold,
        }
    }
    pub fn with_timing(mut self, timing: AnimationTiming) -> Self {
        self.timing = timing;
        self
    }
    pub fn with_ending_style(mut self, style: EndingStyle) -> Self {
        self.ending_style = style;
        self
    }
}
#[derive(Debug, Clone)]
pub enum TimelineNode {
    Clip {
        target: ElementHandle,
        clip: AnimationClip,
        ending_style: EndingStyle,
    },
    Group(TimelineSpec),
}
#[derive(Debug, Clone)]
pub struct TimelineChild {
    /// Serial: gap after the previous child's end. Parallel: offset from group start.
    pub offset_ms: u64,
    pub node: TimelineNode,
}
impl TimelineChild {
    pub fn animate(target: ElementHandle, clip: impl Into<AnimationClip>) -> Self {
        Self {
            offset_ms: 0,
            node: TimelineNode::Clip {
                target,
                clip: clip.into(),
                ending_style: EndingStyle::Hold,
            },
        }
    }
    pub fn group(spec: TimelineSpec) -> Self {
        Self {
            offset_ms: 0,
            node: TimelineNode::Group(spec),
        }
    }
    pub fn with_offset(mut self, offset_ms: u64) -> Self {
        self.offset_ms = offset_ms;
        self
    }
    pub fn with_ending_style(mut self, style: EndingStyle) -> Self {
        match &mut self.node {
            TimelineNode::Clip { ending_style, .. } => *ending_style = style,
            TimelineNode::Group(spec) => spec.ending_style = style,
        }
        self
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TimelineId {
    engine: u64,
    controller: u64,
    sequence: u64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineInfo {
    pub id: TimelineId,
    pub status: AnimationStatus,
    /// Absolute logical parent position, including delays and repeats.
    pub elapsed: Duration,
    pub duration: Duration,
    /// Reverse the parent cursor without jumping the sampled pose.
    pub backward: bool,
}
#[derive(Debug, Clone)]
pub struct TimelineTickReport {
    pub changes: ChangeSet,
    pub advanced: Vec<TimelineId>,
    pub completed: Vec<TimelineId>,
    pub cancelled: Vec<TimelineId>,
}
type Property = (ElementHandle, TweenProperty);
type Pose = BTreeMap<Property, TweenValue>;
#[derive(Debug, Clone)]
struct PreparedNode {
    schedule: Schedule,
    parent_duration: Duration,
    initial: Pose,
    ending_style: EndingStyle,
    kind: PreparedKind,
}
#[derive(Debug, Clone)]
enum PreparedKind {
    Clip {
        target: ElementHandle,
        segments: Vec<sequence::Segment>,
        tracks: Vec<Track>,
    },
    Group(Vec<(Duration, PreparedNode)>),
}
#[derive(Debug, Clone)]
struct Timeline {
    info: TimelineInfo,
    root: PreparedNode,
    epochs: BTreeMap<ElementHandle, u64>,
    last: Pose,
    remainder: u32,
}
struct FramePlan {
    next: BTreeMap<TimelineId, Timeline>,
    updates: PropertyUpdates,
    advanced: Vec<TimelineId>,
    completed: Vec<TimelineId>,
    cancelled: Vec<TimelineId>,
}
/// Use one controller for property ownership checks between registered timelines.
/// Other controllers remain independent; their writes count as external edits.
pub struct TimelineController {
    engine: u64,
    id: u64,
    next: u64,
    timelines: BTreeMap<TimelineId, Timeline>,
}
impl TimelineController {
    pub fn new(engine: &SceneEngine) -> Self {
        let id = crate::NEXT_CONTROLLER
            .try_update(
                std::sync::atomic::Ordering::Relaxed,
                std::sync::atomic::Ordering::Relaxed,
                |value| value.checked_add(1),
            )
            .expect("animation controller identity exhausted");
        Self {
            engine: engine.engine_id(),
            id,
            next: 0,
            timelines: BTreeMap::new(),
        }
    }
    fn check_engine(&self, engine: &SceneEngine) -> Result<(), AnimationError> {
        if engine.engine_id() == self.engine {
            Ok(())
        } else {
            Err(AnimationError::WrongEngine)
        }
    }
    pub fn len(&self) -> usize {
        self.timelines.len()
    }
    pub fn is_empty(&self) -> bool {
        self.timelines.is_empty()
    }
    pub fn is_running(&self) -> bool {
        self.timelines
            .values()
            .any(|t| t.info.status == AnimationStatus::Playing)
    }
    /// One unavailable/overridden child cancels its entire parent. No partial
    /// sibling frame is emitted and cancellation never restores old document data.
    pub fn sync(&mut self, engine: &SceneEngine) -> Result<Vec<TimelineId>, AnimationError> {
        self.check_engine(engine)?;
        let mut cancelled = Vec::new();
        for timeline in self
            .timelines
            .values_mut()
            .filter(|t| !matches!(t.info.status, AnimationStatus::Cancelled(_)))
        {
            let reason = if timeline.epochs.iter().any(|(target, epoch)| {
                !engine.instance(*target).is_ok_and(|i| {
                    i.state == InstanceState::Attached && i.attachment_epoch == *epoch
                })
            }) {
                Some(CancelReason::TargetUnavailable)
            } else if timeline.info.status.active()
                && timeline.last.iter().any(|((target, property), last)| {
                    tween::read(engine, *target, property).map_or(true, |current| current != *last)
                })
            {
                Some(CancelReason::Overridden)
            } else {
                None
            };
            if let Some(reason) = reason {
                timeline.info.status = AnimationStatus::Cancelled(reason);
                cancelled.push(timeline.info.id);
            }
        }
        Ok(cancelled)
    }
    fn check_conflict(
        &self,
        properties: &Pose,
        except: Option<TimelineId>,
    ) -> Result<(), AnimationError> {
        if self.timelines.values().any(|t| {
            Some(t.info.id) != except
                && t.info.status.active()
                && t.last.keys().any(|key| properties.contains_key(key))
        }) {
            Err(AnimationError::Conflict)
        } else {
            Ok(())
        }
    }
    /// Register without writing. Serial children capture preceding ending poses;
    /// parallel siblings must have disjoint property unions, even across offsets.
    pub fn animate(
        &mut self,
        engine: &SceneEngine,
        spec: TimelineSpec,
    ) -> Result<TimelineId, AnimationError> {
        self.sync(engine)?;
        if self.timelines.len() >= MAX_TIMELINES {
            return Err(AnimationError::Capacity);
        }
        validate_budget(&spec)?;
        let root = prepare_group(engine, &spec, &Pose::new())?;
        self.check_conflict(&root.initial, None)?;
        let mut epochs = BTreeMap::new();
        for (target, _) in root.initial.keys() {
            let instance = engine.instance(*target)?;
            if instance.state != InstanceState::Attached {
                return Err(gpui_flow_engine::EngineError::DetachedHandle.into());
            }
            epochs.insert(*target, instance.attachment_epoch);
        }
        let sequence = self.next.checked_add(1).ok_or(AnimationError::Capacity)?;
        let id = TimelineId {
            engine: self.engine,
            controller: self.id,
            sequence,
        };
        self.timelines.insert(
            id,
            Timeline {
                info: TimelineInfo {
                    id,
                    status: AnimationStatus::Playing,
                    elapsed: Duration::ZERO,
                    duration: root.schedule.total.expect("finite timeline"),
                    backward: false,
                },
                last: root.initial.clone(),
                root,
                epochs,
                remainder: 0,
            },
        );
        self.next = sequence;
        Ok(id)
    }
    pub fn info(
        &mut self,
        engine: &SceneEngine,
        id: TimelineId,
    ) -> Result<TimelineInfo, AnimationError> {
        self.sync(engine)?;
        Ok(self
            .timelines
            .get(&id)
            .ok_or(AnimationError::UnknownAnimation)?
            .info
            .clone())
    }
    pub fn timing_sample(
        &mut self,
        engine: &SceneEngine,
        id: TimelineId,
    ) -> Result<TimingSample, AnimationError> {
        self.sync(engine)?;
        let t = self
            .timelines
            .get(&id)
            .ok_or(AnimationError::UnknownAnimation)?;
        Ok(t.root.schedule.sample(t.info.elapsed))
    }
    pub fn pause(&mut self, engine: &SceneEngine, id: TimelineId) -> Result<(), AnimationError> {
        self.active_mut(engine, id)?.info.status = AnimationStatus::Paused;
        Ok(())
    }
    pub fn resume(&mut self, engine: &SceneEngine, id: TimelineId) -> Result<(), AnimationError> {
        self.active_mut(engine, id)?.info.status = AnimationStatus::Playing;
        Ok(())
    }
    fn active_mut(
        &mut self,
        engine: &SceneEngine,
        id: TimelineId,
    ) -> Result<&mut Timeline, AnimationError> {
        self.sync(engine)?;
        let t = self
            .timelines
            .get_mut(&id)
            .ok_or(AnimationError::UnknownAnimation)?;
        if !t.info.status.active() {
            return Err(AnimationError::InvalidState);
        }
        Ok(t)
    }
    /// Toggle parent clock travel without moving its current pose. A completed
    /// timeline is reactivated paused, after rechecking its whole property union.
    pub fn reverse(&mut self, engine: &SceneEngine, id: TimelineId) -> Result<(), AnimationError> {
        self.sync(engine)?;
        let t = self
            .timelines
            .get(&id)
            .ok_or(AnimationError::UnknownAnimation)?;
        if matches!(t.info.status, AnimationStatus::Cancelled(_)) {
            return Err(AnimationError::InvalidState);
        }
        self.check_conflict(&t.last, Some(id))?;
        let t = self.timelines.get_mut(&id).expect("checked timeline");
        if t.info.status == AnimationStatus::Completed {
            // Completion released ownership. Reacquiring it must observe the
            // current document, independent of any intervening idle ticks.
            if !refresh(engine, t) {
                return Err(AnimationError::InvalidState);
            }
            t.info.status = AnimationStatus::Paused;
        }
        t.info.backward = !t.info.backward;
        t.remainder = 0;
        Ok(())
    }
    pub fn set_playback_rate(
        &mut self,
        engine: &SceneEngine,
        id: TimelineId,
        speed: PlaybackRate,
    ) -> Result<(), AnimationError> {
        let t = self.active_mut(engine, id)?;
        bounded_parent_duration(t.info.duration, speed)?;
        t.root.schedule.timing.speed = speed;
        t.remainder = 0;
        Ok(())
    }
    pub fn cancel(&mut self, engine: &SceneEngine, id: TimelineId) -> Result<(), AnimationError> {
        self.sync(engine)?;
        let t = self
            .timelines
            .get_mut(&id)
            .ok_or(AnimationError::UnknownAnimation)?;
        if t.info.status.active() {
            t.info.status = AnimationStatus::Cancelled(CancelReason::User);
        }
        Ok(())
    }
    pub fn remove(&mut self, id: TimelineId) -> bool {
        self.timelines.remove(&id).is_some()
    }
    pub fn tick(
        &mut self,
        engine: &mut SceneEngine,
        delta: Duration,
    ) -> Result<TimelineTickReport, AnimationError> {
        let mut plan = self.plan_tick(engine, delta)?;
        let changes = engine
            .set_properties_batch(ChangeOrigin::Animation, std::mem::take(&mut plan.updates))?;
        Ok(self.commit_tick(engine, plan, changes))
    }
    fn plan_tick(
        &mut self,
        engine: &SceneEngine,
        delta: Duration,
    ) -> Result<FramePlan, AnimationError> {
        let cancelled = self.sync(engine)?;
        let mut plan = FramePlan {
            next: self.timelines.clone(),
            updates: Vec::new(),
            advanced: Vec::new(),
            completed: Vec::new(),
            cancelled,
        };
        for t in plan
            .next
            .values_mut()
            .filter(|t| t.info.status == AnimationStatus::Playing)
        {
            let (elapsed, remainder) = if t.info.backward {
                let (step, remainder) =
                    t.root
                        .schedule
                        .advance(Duration::ZERO, t.remainder, delta)?;
                let elapsed = t.info.elapsed.saturating_sub(step);
                (elapsed, if elapsed.is_zero() { 0 } else { remainder })
            } else {
                t.root
                    .schedule
                    .advance(t.info.elapsed, t.remainder, delta)?
            };
            if elapsed != t.info.elapsed {
                plan.advanced.push(t.info.id);
            }
            let complete = if t.info.backward {
                elapsed.is_zero()
            } else {
                elapsed == t.info.duration
            };
            stage(engine, t, elapsed, complete, &mut plan.updates)?;
            t.remainder = remainder;
            if complete {
                t.info.status = AnimationStatus::Completed;
                plan.completed.push(t.info.id);
            }
        }
        Ok(plan)
    }
    fn commit_tick(
        &mut self,
        engine: &SceneEngine,
        mut plan: FramePlan,
        changes: ChangeSet,
    ) -> TimelineTickReport {
        for t in plan
            .next
            .values_mut()
            .filter(|t| !matches!(t.info.status, AnimationStatus::Cancelled(_)))
        {
            if !refresh(engine, t) {
                plan.cancelled.push(t.info.id);
                plan.completed.retain(|id| *id != t.info.id);
            }
        }
        self.timelines = plan.next;
        TimelineTickReport {
            changes,
            advanced: plan.advanced,
            completed: plan.completed,
            cancelled: plan.cancelled,
        }
    }
    pub fn seek(
        &mut self,
        engine: &mut SceneEngine,
        id: TimelineId,
        position: Duration,
    ) -> Result<ChangeSet, AnimationError> {
        let (mut t, updates) = self.plan_seek(engine, id, position)?;
        let changes = engine.set_properties_batch(ChangeOrigin::Animation, updates)?;
        refresh(engine, &mut t);
        self.timelines.insert(id, t);
        Ok(changes)
    }
    fn plan_seek(
        &mut self,
        engine: &SceneEngine,
        id: TimelineId,
        position: Duration,
    ) -> Result<(Timeline, PropertyUpdates), AnimationError> {
        self.sync(engine)?;
        let mut t = self
            .timelines
            .get(&id)
            .ok_or(AnimationError::UnknownAnimation)?
            .clone();
        if matches!(t.info.status, AnimationStatus::Cancelled(_)) {
            return Err(AnimationError::InvalidState);
        }
        self.check_conflict(&t.last, Some(id))?;
        let elapsed = position.min(t.info.duration);
        let complete = if t.info.backward {
            elapsed.is_zero()
        } else {
            elapsed == t.info.duration
        };
        let mut updates = Vec::new();
        stage(engine, &mut t, elapsed, complete, &mut updates)?;
        t.remainder = 0;
        t.info.status = if complete {
            AnimationStatus::Completed
        } else if t.info.status == AnimationStatus::Completed {
            AnimationStatus::Paused
        } else {
            t.info.status
        };
        Ok((t, updates))
    }
    fn ensure_editor(
        &mut self,
        editor: &mut SceneEditor,
        token: PlaybackToken,
    ) -> Result<(), AnimationError> {
        self.check_engine(&editor.engine)?;
        if !editor.is_playback_active(token) {
            for t in self
                .timelines
                .values_mut()
                .filter(|t| t.info.status.active())
            {
                t.info.status = AnimationStatus::Cancelled(CancelReason::EditorInterrupted);
            }
            return Err(AnimationError::EditorSessionEnded);
        }
        Ok(())
    }
    pub fn tick_editor(
        &mut self,
        editor: &mut SceneEditor,
        token: PlaybackToken,
        delta: Duration,
    ) -> Result<TimelineTickReport, AnimationError> {
        self.ensure_editor(editor, token)?;
        let mut plan = self.plan_tick(&editor.engine, delta)?;
        let changes = editor.apply_playback_properties(token, std::mem::take(&mut plan.updates))?;
        Ok(self.commit_tick(&editor.engine, plan, changes))
    }
    pub fn seek_editor(
        &mut self,
        editor: &mut SceneEditor,
        token: PlaybackToken,
        id: TimelineId,
        position: Duration,
    ) -> Result<ChangeSet, AnimationError> {
        self.ensure_editor(editor, token)?;
        let (mut t, updates) = self.plan_seek(&editor.engine, id, position)?;
        let changes = editor.apply_playback_properties(token, updates)?;
        refresh(&editor.engine, &mut t);
        self.timelines.insert(id, t);
        Ok(changes)
    }
    /// Commit the current whole-parent pose as one Undo step. This does not seek.
    pub fn finish_editor(
        &mut self,
        editor: &mut SceneEditor,
        token: PlaybackToken,
    ) -> Result<(), AnimationError> {
        self.ensure_editor(editor, token)?;
        if !editor.finish_playback(token) {
            return Err(AnimationError::EditorSessionEnded);
        }
        for t in self
            .timelines
            .values_mut()
            .filter(|t| t.info.status.active())
        {
            t.info.status = AnimationStatus::Cancelled(CancelReason::User);
        }
        Ok(())
    }
    pub fn cancel_editor(
        &mut self,
        editor: &mut SceneEditor,
        token: PlaybackToken,
    ) -> Result<(), AnimationError> {
        self.ensure_editor(editor, token)?;
        if !editor.cancel_playback(token) {
            return Err(AnimationError::EditorSessionEnded);
        }
        for t in self.timelines.values_mut() {
            t.info.status = AnimationStatus::Cancelled(CancelReason::User);
        }
        Ok(())
    }
}
fn invalid(reason: &str) -> AnimationError {
    AnimationError::InvalidInput(reason.into())
}
fn bounded(duration: Duration) -> Result<Duration, AnimationError> {
    if duration > MAX_TIMELINE_DURATION {
        Err(invalid("timeline exceeds 24-hour duration limit"))
    } else {
        Ok(duration)
    }
}
fn bounded_parent_duration(
    total: Duration,
    speed: PlaybackRate,
) -> Result<Duration, AnimationError> {
    let nanos = (total.as_nanos() * u128::from(speed.denominator()))
        .div_ceil(u128::from(speed.numerator()));
    if nanos > MAX_TIMELINE_DURATION.as_nanos() {
        return Err(invalid("timeline exceeds 24-hour parent-clock limit"));
    }
    Ok(Duration::new(
        (nanos / 1_000_000_000) as u64,
        (nanos % 1_000_000_000) as u32,
    ))
}
fn validate_budget(root: &TimelineSpec) -> Result<(), AnimationError> {
    let mut pending = vec![(root, 1)];
    let mut count = 1;
    while let Some((group, depth)) = pending.pop() {
        if group.children.is_empty() {
            return Err(invalid("timeline groups require children"));
        }
        if depth > MAX_TIMELINE_DEPTH {
            return Err(invalid("timeline nesting exceeds 8 levels"));
        }
        count += group.children.len();
        if count > MAX_TIMELINE_NODES {
            return Err(AnimationError::Capacity);
        }
        for child in &group.children {
            if depth + 1 > MAX_TIMELINE_DEPTH {
                return Err(invalid("timeline nesting exceeds 8 levels"));
            }
            if let TimelineNode::Group(group) = &child.node {
                pending.push((group, depth + 1));
            }
        }
    }
    Ok(())
}
fn compile_node(
    kind: PreparedKind,
    initial: Pose,
    timing: AnimationTiming,
    ending_style: EndingStyle,
    cycle: Duration,
) -> Result<PreparedNode, AnimationError> {
    if matches!(timing.iterations, IterationCount::Infinite) {
        return Err(invalid("timeline children and parents must be finite"));
    }
    let schedule = timing.compile(bounded(cycle)?)?;
    let total = bounded(schedule.total.expect("finite timeline"))?;
    let parent_duration = bounded_parent_duration(total, timing.speed)?;
    if initial.len() > MAX_TIMELINE_PROPERTIES {
        return Err(AnimationError::Capacity);
    }
    Ok(PreparedNode {
        schedule,
        parent_duration,
        initial,
        ending_style,
        kind,
    })
}
fn prepare_group(
    engine: &SceneEngine,
    spec: &TimelineSpec,
    inherited: &Pose,
) -> Result<PreparedNode, AnimationError> {
    let mut children = Vec::new();
    let mut initial = Pose::new();
    let mut running = inherited.clone();
    let mut duration = Duration::ZERO;
    let mut parallel_owned = BTreeSet::new();
    for child in &spec.children {
        let from = if spec.mode == TimelineMode::Serial {
            &running
        } else {
            inherited
        };
        let prepared = match &child.node {
            TimelineNode::Group(group) => prepare_group(engine, group, from)?,
            TimelineNode::Clip {
                target,
                clip,
                ending_style,
            } => {
                let initial = from
                    .iter()
                    .filter(|((h, _), _)| h == target)
                    .map(|((_, p), v)| (p.clone(), v.clone()))
                    .collect();
                let (segments, tracks, cycle) =
                    sequence::prepare_with_initial(engine, *target, &clip.sequence, &initial)?;
                let pose = tracks
                    .iter()
                    .map(|t| ((*target, t.property.clone()), t.values[0].clone()))
                    .collect();
                compile_node(
                    PreparedKind::Clip {
                        target: *target,
                        segments,
                        tracks,
                    },
                    pose,
                    clip.timing,
                    *ending_style,
                    cycle,
                )?
            }
        };
        for (key, value) in &prepared.initial {
            if spec.mode == TimelineMode::Parallel && !parallel_owned.insert(key.clone()) {
                return Err(AnimationError::Conflict);
            }
            initial.entry(key.clone()).or_insert_with(|| value.clone());
        }
        if initial.len() > MAX_TIMELINE_PROPERTIES {
            return Err(AnimationError::Capacity);
        }
        let start = if spec.mode == TimelineMode::Serial {
            duration
        } else {
            Duration::ZERO
        }
        .checked_add(Duration::from_millis(child.offset_ms))
        .ok_or_else(|| invalid("timeline offset overflow"))?;
        let end = bounded(
            start
                .checked_add(prepared.parent_duration)
                .ok_or_else(|| invalid("timeline child duration overflow"))?,
        )?;
        duration = duration.max(end);
        if spec.mode == TimelineMode::Serial {
            running.extend(prepared.sample(prepared.schedule.total.expect("finite"), false)?);
        }
        children.push((start, prepared));
    }
    compile_node(
        PreparedKind::Group(children),
        initial,
        spec.timing,
        spec.ending_style,
        duration,
    )
}
impl PreparedNode {
    fn sample(&self, position: Duration, inherited_reverse: bool) -> Result<Pose, AnimationError> {
        let mut timing = self.schedule.sample(position);
        if timing.phase == TimingPhase::Delay
            || (timing.phase == TimingPhase::Completed && self.ending_style == EndingStyle::Restore)
        {
            return Ok(self.initial.clone());
        }
        timing.reversed ^= inherited_reverse;
        match &self.kind {
            PreparedKind::Clip {
                target,
                segments,
                tracks,
            } => Ok(sequence::sample_tracks(segments, tracks, timing)?
                .into_iter()
                .map(|(p, v)| ((*target, p), v))
                .collect()),
            PreparedKind::Group(children) => {
                let mut pose = self.initial.clone();
                for (start, child) in children {
                    if timing.cycle_position >= *start {
                        let (local, _) = child.schedule.advance(
                            Duration::ZERO,
                            0,
                            timing.cycle_position - *start,
                        )?;
                        pose.extend(child.sample(local, timing.reversed)?);
                    }
                }
                Ok(pose)
            }
        }
    }
}
fn stage(
    engine: &SceneEngine,
    t: &mut Timeline,
    elapsed: Duration,
    complete: bool,
    updates: &mut PropertyUpdates,
) -> Result<(), AnimationError> {
    let pose = if complete && t.root.ending_style == EndingStyle::Restore {
        t.root.initial.clone()
    } else {
        t.root.sample(elapsed, false)?
    };
    for ((target, property), value) in pose {
        if value != tween::read(engine, target, &property)? {
            let (key, data) = tween::update(engine, target, &property, &value)?;
            updates.push((target, key, data));
        }
    }
    t.info.elapsed = elapsed;
    Ok(())
}
fn refresh(engine: &SceneEngine, t: &mut Timeline) -> bool {
    for ((target, property), last) in &mut t.last {
        match tween::read(engine, *target, property) {
            Ok(value) => *last = value,
            Err(_) => {
                t.info.status = AnimationStatus::Cancelled(CancelReason::TargetUnavailable);
                return false;
            }
        }
    }
    true
}
