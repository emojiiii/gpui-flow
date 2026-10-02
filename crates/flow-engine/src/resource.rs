//! Runtime resource observations. Loading, decoding and font resolution belong
//! to the host; this module only owns live identity, settlement and event policy.
use crate::{ElementHandle, EngineError, InstanceState, LifecycleKind, ObserverId, SceneEngine};
use gpui_flow_scene::{DisplayId, EventKind, SceneEvent, Shape, display::Text};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

/// The built-in resources that have a real producer in the GPUI adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ResourceKind {
    Image,
    Text,
    Canvas,
}
impl ResourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Text => "text",
            Self::Canvas => "canvas",
        }
    }
}
/// Opaque completion identity. A document ID alone must never accept a result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ResourceTicket {
    owner: ObserverId,
    pub handle: ElementHandle,
    pub attachment_epoch: u64,
    pub generation: u64,
    pub kind: ResourceKind,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceCancellation {
    Superseded,
    Detached,
    Disposed,
    NotAuthorized,
}
impl ResourceCancellation {
    fn as_str(self) -> &'static str {
        match self {
            Self::Superseded => "superseded",
            Self::Detached => "detached",
            Self::Disposed => "disposed",
            Self::NotAuthorized => "not_authorized",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceState {
    Pending,
    Loaded,
    Failed(String),
    Cancelled(ResourceCancellation),
}
impl ResourceState {
    pub fn is_settled(&self) -> bool {
        !matches!(self, Self::Pending)
    }
    pub fn event_kind(&self) -> EventKind {
        EventKind::Custom(format!("resource.{}", self.as_str()))
    }
    fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Loaded => "loaded",
            Self::Failed(_) => "failed",
            Self::Cancelled(_) => "cancelled",
        }
    }
}
/// A backend may report a terminal outcome, never manufacture another Pending.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceOutcome {
    Loaded,
    Failed(String),
    Cancelled(ResourceCancellation),
}
impl From<ResourceOutcome> for ResourceState {
    fn from(value: ResourceOutcome) -> Self {
        match value {
            ResourceOutcome::Loaded => Self::Loaded,
            ResourceOutcome::Failed(error) => Self::Failed(error),
            ResourceOutcome::Cancelled(reason) => Self::Cancelled(reason),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceEvent {
    pub id: DisplayId,
    pub ticket: ResourceTicket,
    pub state: ResourceState,
    /// False for an old attachment/destroyed instance: its event is observable
    /// by the resource owner but must not be routed into a replacement tree.
    pub dispatched: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceCompletion {
    Accepted,
    AlreadySettled,
    Stale,
}
#[derive(Debug, Clone, PartialEq)]
pub enum ResourceError {
    Engine(EngineError),
    Capacity { limit: usize, requested: usize },
    Disposed,
}
impl std::fmt::Display for ResourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Engine(error) => error.fmt(f),
            Self::Capacity { limit, requested } => {
                write!(
                    f,
                    "resource limit {limit} exceeded by {requested} attached resources"
                )
            }
            Self::Disposed => write!(f, "resource tracker is disposed"),
        }
    }
}
impl std::error::Error for ResourceError {}
impl From<EngineError> for ResourceError {
    fn from(value: EngineError) -> Self {
        Self::Engine(value)
    }
}
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ResourceCounts {
    pub pending: usize,
    pub loaded: usize,
    pub failed: usize,
    pub cancelled: usize,
}
impl ResourceCounts {
    pub fn total(self) -> usize {
        self.pending + self.loaded + self.failed + self.cancelled
    }
}
/// Ready = committed engine layout available. ViewReady = first accepted host
/// paint receipt. Complete = a receipt with every tracked resource terminal,
/// including failures/cancellations; it never promises every resource succeeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceViewMilestone {
    Ready,
    ViewReady,
    Complete,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceViewEvent {
    pub milestone: ResourceViewMilestone,
    pub document_epoch: u64,
    pub resource_epoch: u64,
    pub counts: ResourceCounts,
}
/// A host must obtain this after resource synchronization and acknowledge it
/// only from its actual paint boundary. It is not a GPU presentation fence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceFrame {
    owner: ObserverId,
    engine: u64,
    revision: u64,
    document_epoch: u64,
    resource_epoch: u64,
    counts: ResourceCounts,
}
#[derive(Clone, PartialEq)]
enum Source {
    Image(String),
    Text(Text),
    Canvas(gpui_flow_scene::display::Canvas),
}
impl Source {
    fn from_shape(shape: &Shape) -> Option<Self> {
        match shape {
            Shape::Image(image) => Some(Self::Image(image.source.clone())),
            Shape::Text(text) => Some(Self::Text(text.clone())),
            Shape::Canvas(canvas) => Some(Self::Canvas(canvas.clone())),
            _ => None,
        }
    }
    fn matches_shape(&self, shape: &Shape) -> bool {
        match (self, shape) {
            (Self::Image(source), Shape::Image(image)) => *source == image.source,
            (Self::Text(source), Shape::Text(text)) => source == text,
            (Self::Canvas(source), Shape::Canvas(canvas)) => source == canvas,
            _ => false,
        }
    }
    fn kind(&self) -> ResourceKind {
        match self {
            Self::Image(_) => ResourceKind::Image,
            Self::Text(_) => ResourceKind::Text,
            Self::Canvas(_) => ResourceKind::Canvas,
        }
    }
}
struct Entry {
    id: DisplayId,
    ticket: ResourceTicket,
    source: Source,
    state: ResourceState,
    invalidated: bool,
}
impl Entry {
    fn matches(&self, engine: &SceneEngine) -> bool {
        !self.invalidated
            && engine.instance(self.ticket.handle).is_ok_and(|instance| {
                instance.state == InstanceState::Attached
                    && instance.attachment_epoch == self.ticket.attachment_epoch
            })
            && engine
                .object(self.ticket.handle)
                .is_ok_and(|object| self.source.matches_shape(&object.shape))
    }
    fn event(&self, state: ResourceState) -> ResourceEvent {
        ResourceEvent {
            id: self.id.clone(),
            ticket: self.ticket,
            state,
            dispatched: false,
        }
    }
}
/// One bounded observer per view/engine, never serialized or cloned. Call
/// `dispose` before dropping a live host; SceneView does this on GPUI release.
/// The observer holds only a Weak reference, so an accidentally dropped tracker
/// cannot keep resource state alive. Engine destruction removes the observer.
pub struct SceneResources {
    engine: u64,
    limit: usize,
    entries: Rc<RefCell<BTreeMap<ElementHandle, Entry>>>,
    observer: Option<ObserverId>,
    next_generation: u64,
    resource_epoch: u64,
    ready_document: Option<u64>,
    painted_document: Option<u64>,
    completed: Option<(u64, u64)>,
}
impl SceneResources {
    pub const DEFAULT_LIMIT: usize = 4096;
    pub fn new(engine: &mut SceneEngine) -> Result<Self, ResourceError> {
        Self::with_limit(engine, Self::DEFAULT_LIMIT)
    }
    pub fn with_limit(engine: &mut SceneEngine, limit: usize) -> Result<Self, ResourceError> {
        let entries: Rc<RefCell<BTreeMap<ElementHandle, Entry>>> = Rc::default();
        let weak = Rc::downgrade(&entries);
        let observer = engine.on_change(None, move |changes| {
            let Some(entries) = weak.upgrade() else {
                return;
            };
            for entry in entries.borrow_mut().values_mut() {
                let unmounted = changes.lifecycle.iter().any(|event| {
                    event.handle == entry.ticket.handle
                        && matches!(
                            event.kind,
                            LifecycleKind::Unmounted | LifecycleKind::Destroyed
                        )
                });
                let changed = changes.properties.iter().any(|property| {
                    if property.id.as_ref() != Some(&entry.id) {
                        return false;
                    }
                    if property.property == "shape" {
                        match entry.source {
                            Source::Image(_) => {
                                property.old["type"] != property.new["type"]
                                    || property.old["geometry"]["source"]
                                        != property.new["geometry"]["source"]
                            }
                            Source::Text(_) | Source::Canvas(_) => property.old != property.new,
                        }
                    } else {
                        // Conservative support for future granular shape deltas.
                        property.property.starts_with("shape.")
                            || (property.property == "text.content"
                                && matches!(&entry.source, Source::Text(_)))
                    }
                });
                // Commit observation catches A -> B -> A even without a render
                // between changes. It never queues one allocation per edit.
                entry.invalidated |= unmounted || changed;
            }
        })?;
        Ok(Self {
            engine: engine.engine_id(),
            limit,
            entries,
            observer: Some(observer),
            next_generation: 0,
            resource_epoch: 0,
            ready_document: None,
            painted_document: None,
            completed: None,
        })
    }
    fn check(&self, engine: &SceneEngine) -> Result<(), ResourceError> {
        if self.observer.is_none() {
            return Err(ResourceError::Disposed);
        }
        if self.engine != engine.engine_id() {
            return Err(EngineError::InvalidHandle.into());
        }
        Ok(())
    }
    /// Reconcile all attached built-in text/images/Canvas surfaces, including invisible nodes.
    /// Discovery is inert: it never loads bytes, launches work or grants access.
    /// Capacity/generation failures occur before mutating the tracked set.
    pub fn synchronize(
        &mut self,
        engine: &mut SceneEngine,
    ) -> Result<Vec<ResourceEvent>, ResourceError> {
        self.check(engine)?;
        let requested = engine
            .scene()
            .nodes()
            .filter(|node| {
                matches!(
                    node.object.shape,
                    Shape::Image(_) | Shape::Text(_) | Shape::Canvas(_)
                )
            })
            .count();
        if requested > self.limit {
            return Err(ResourceError::Capacity {
                limit: self.limit,
                requested,
            });
        }
        let desired = engine
            .scene()
            .nodes()
            .filter_map(|node| {
                let source = Source::from_shape(&node.object.shape)?;
                let handle = engine.handle(&node.object.id)?;
                let instance = engine.instance(handle).ok()?;
                Some((
                    handle,
                    node.object.id.clone(),
                    instance.attachment_epoch,
                    source,
                ))
            })
            .collect::<Vec<_>>();
        let entries = self.entries.borrow();
        let obsolete = entries
            .iter()
            .filter(|(_, entry)| !entry.matches(engine))
            .map(|(handle, _)| *handle)
            .collect::<Vec<_>>();
        let additions = desired
            .into_iter()
            .filter(|(handle, _, _, _)| {
                entries
                    .get(handle)
                    .is_none_or(|entry| !entry.matches(engine))
            })
            .collect::<Vec<_>>();
        drop(entries);
        let next_generation = self
            .next_generation
            .checked_add(additions.len() as u64)
            .ok_or(EngineError::Exhausted)?;
        let changed = !obsolete.is_empty() || !additions.is_empty();
        let epoch = self
            .resource_epoch
            .checked_add(u64::from(changed))
            .ok_or(EngineError::Exhausted)?;
        let mut events = vec![];
        {
            let mut entries = self.entries.borrow_mut();
            for handle in obsolete {
                let entry = entries.remove(&handle).expect("tracked resource");
                if entry.state == ResourceState::Pending {
                    let reason = if engine.instance(handle).is_ok_and(|instance| {
                        instance.state == InstanceState::Attached
                            && instance.attachment_epoch == entry.ticket.attachment_epoch
                    }) {
                        ResourceCancellation::Superseded
                    } else {
                        ResourceCancellation::Detached
                    };
                    events.push(entry.event(ResourceState::Cancelled(reason)));
                }
            }
            for (handle, id, attachment_epoch, source) in additions {
                self.next_generation += 1;
                let entry = Entry {
                    id,
                    ticket: ResourceTicket {
                        owner: self.observer.expect("active resource observer"),
                        handle,
                        attachment_epoch,
                        generation: self.next_generation,
                        kind: source.kind(),
                    },
                    source,
                    state: ResourceState::Pending,
                    invalidated: false,
                };
                events.push(entry.event(ResourceState::Pending));
                entries.insert(handle, entry);
            }
        }
        debug_assert_eq!(self.next_generation, next_generation);
        self.resource_epoch = epoch;
        for event in &mut events {
            dispatch(engine, event)?;
        }
        Ok(events)
    }
    pub fn ticket(&self, engine: &SceneEngine, handle: ElementHandle) -> Option<ResourceTicket> {
        self.entries
            .borrow()
            .get(&handle)
            .filter(|entry| entry.matches(engine))
            .map(|entry| entry.ticket)
    }
    pub fn state(&self, engine: &SceneEngine, ticket: ResourceTicket) -> Option<ResourceState> {
        self.entries
            .borrow()
            .get(&ticket.handle)
            .filter(|entry| entry.ticket == ticket && entry.matches(engine))
            .map(|entry| entry.state.clone())
    }
    /// Invalid/stale results do not mutate counters, emit events, or address a
    /// replacement instance. Terminal outcomes are accepted once per ticket.
    pub fn complete(
        &mut self,
        engine: &mut SceneEngine,
        ticket: ResourceTicket,
        outcome: ResourceOutcome,
    ) -> Result<(ResourceCompletion, Option<ResourceEvent>), ResourceError> {
        self.check(engine)?;
        let event = {
            let mut entries = self.entries.borrow_mut();
            let Some(entry) = entries
                .get_mut(&ticket.handle)
                .filter(|entry| entry.ticket == ticket && entry.matches(engine))
            else {
                return Ok((ResourceCompletion::Stale, None));
            };
            if entry.state.is_settled() {
                return Ok((ResourceCompletion::AlreadySettled, None));
            }
            entry.state = outcome.into();
            entry.event(entry.state.clone())
        };
        let mut event = event;
        dispatch(engine, &mut event)?;
        Ok((ResourceCompletion::Accepted, Some(event)))
    }
    /// Host configuration/font/cache changes are explicit; ordinary redraws
    /// never turn failures into an unbounded automatic retry loop.
    pub fn invalidate(
        &mut self,
        engine: &SceneEngine,
        kind: ResourceKind,
    ) -> Result<(), ResourceError> {
        self.check(engine)?;
        for entry in self.entries.borrow_mut().values_mut() {
            if entry.ticket.kind == kind {
                entry.invalidated = true;
            }
        }
        Ok(())
    }
    pub fn counts(&self, engine: &SceneEngine) -> ResourceCounts {
        let mut counts = ResourceCounts::default();
        for entry in self
            .entries
            .borrow()
            .values()
            .filter(|entry| entry.matches(engine))
        {
            match entry.state {
                ResourceState::Pending => counts.pending += 1,
                ResourceState::Loaded => counts.loaded += 1,
                ResourceState::Failed(_) => counts.failed += 1,
                ResourceState::Cancelled(_) => counts.cancelled += 1,
            }
        }
        counts
    }
    /// Return a receipt only for a fully discovered, current resource set.
    pub fn frame(&self, engine: &SceneEngine) -> Option<ResourceFrame> {
        if self.check(engine).is_err() {
            return None;
        }
        let entries = self.entries.borrow();
        let required = engine
            .scene()
            .nodes()
            .filter(|node| {
                matches!(
                    node.object.shape,
                    Shape::Image(_) | Shape::Text(_) | Shape::Canvas(_)
                )
            })
            .count();
        if entries.len() != required || entries.values().any(|entry| !entry.matches(engine)) {
            return None;
        }
        Some(ResourceFrame {
            owner: self.observer.expect("active resource observer"),
            engine: self.engine,
            revision: engine.revision(),
            document_epoch: engine.document_epoch(),
            resource_epoch: self.resource_epoch,
            counts: self.counts(engine),
        })
    }
    /// Emits Ready once per document, after the engine's real derived layout.
    /// Pending/failed resources are deliberately independent of this boundary.
    pub fn layout_ready(&mut self, engine: &SceneEngine) -> Option<ResourceViewEvent> {
        let frame = self.frame(engine)?;
        if self.ready_document == Some(frame.document_epoch) {
            return None;
        }
        self.ready_document = Some(frame.document_epoch);
        Some(view_event(ResourceViewMilestone::Ready, frame))
    }
    /// Call from a real backend paint boundary. A stale receipt, including one
    /// predating a resource result, cannot announce completion of a newer frame.
    pub fn frame_drawn(
        &mut self,
        engine: &SceneEngine,
        frame: ResourceFrame,
    ) -> Vec<ResourceViewEvent> {
        if self.frame(engine) != Some(frame) {
            return vec![];
        }
        let mut events = vec![];
        if self.painted_document != Some(frame.document_epoch) {
            self.painted_document = Some(frame.document_epoch);
            events.push(view_event(ResourceViewMilestone::ViewReady, frame));
        }
        let completion = (frame.document_epoch, frame.resource_epoch);
        if frame.counts.pending == 0 && self.completed != Some(completion) {
            self.completed = Some(completion);
            events.push(view_event(ResourceViewMilestone::Complete, frame));
        }
        events
    }
    /// Releases this tracker and cancels its pending observations exactly once.
    /// Shared backend caches/tasks are not evicted or cancelled for other views.
    pub fn dispose(
        &mut self,
        engine: &mut SceneEngine,
    ) -> Result<Vec<ResourceEvent>, ResourceError> {
        if self.observer.is_none() {
            return Ok(vec![]);
        }
        self.check(engine)?;
        if let Some(observer) = self.observer.take() {
            engine.off_change(observer);
        }
        let entries = std::mem::take(&mut *self.entries.borrow_mut());
        let mut events = entries
            .into_values()
            .filter(|entry| entry.state == ResourceState::Pending)
            .map(|entry| entry.event(ResourceState::Cancelled(ResourceCancellation::Disposed)))
            .collect::<Vec<_>>();
        for event in &mut events {
            dispatch(engine, event)?;
        }
        Ok(events)
    }
}
fn view_event(milestone: ResourceViewMilestone, frame: ResourceFrame) -> ResourceViewEvent {
    ResourceViewEvent {
        milestone,
        document_epoch: frame.document_epoch,
        resource_epoch: frame.resource_epoch,
        counts: frame.counts,
    }
}
fn dispatch(engine: &mut SceneEngine, event: &mut ResourceEvent) -> Result<(), ResourceError> {
    let live = engine.instance(event.ticket.handle).is_ok_and(|instance| {
        instance.state == InstanceState::Attached
            && instance.attachment_epoch == event.ticket.attachment_epoch
    });
    if !live {
        return Ok(());
    }
    let detail = match &event.state {
        ResourceState::Failed(error) => Some(error.as_str()),
        ResourceState::Cancelled(reason) => Some(reason.as_str()),
        _ => None,
    };
    let mut scene_event =
        SceneEvent::new(event.state.event_kind(), event.id.clone()).with_data(serde_json::json!({
            "resource": event.ticket.kind.as_str(), "generation": event.ticket.generation,
            "attachment_epoch": event.ticket.attachment_epoch, "state": event.state.as_str(),
            "detail": detail,
        }));
    scene_event.cancelable = false;
    scene_event.sequence = event.ticket.generation;
    engine.dispatch(scene_event)?;
    event.dispatched = true;
    Ok(())
}
