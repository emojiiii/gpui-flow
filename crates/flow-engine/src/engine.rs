use crate::change::diff;
use crate::layout::update_layout;
use crate::registry::apply_property;
use crate::subtree::{extract_subtree, insert_forest, tree_ids};
use crate::*;
use gpui_flow_scene::{
    DispatchResult, DisplayId, DisplayObject, EventDispatcher, EventKind, ListenerOptions, Scene,
    SceneError, SceneEvent, Shape,
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    panic::{AssertUnwindSafe, catch_unwind},
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT_ENGINE: AtomicU64 = AtomicU64::new(1);
struct Instance {
    id: DisplayId,
    generation: u64,
    state: InstanceState,
    attachment_epoch: u64,
    disposers: BTreeMap<DisposerId, Box<dyn FnOnce()>>,
}
struct Observer {
    owner: Option<ElementHandle>,
    callback: Box<dyn FnMut(&ChangeSet)>,
}
#[derive(Default)]
enum CommitMode {
    #[default]
    Normal,
    Reset,
    Detach(u64, Scene),
    Attach(u64),
}
/// Canonical live owner. Only document snapshots are cloneable/serializable.
/// Transactions describe atomic net state, not intermediate operations inside
/// their closure. Use explicit destroy/create calls when replacing identity.
pub struct SceneEngine {
    id: u64,
    scene: Scene,
    registry: ElementRegistry,
    revision: u64,
    document_epoch: u64,
    layout: LayoutSnapshot,
    layout_provider: Option<crate::derive::InstalledLayout>,
    pub(crate) deriving_layout: std::cell::Cell<bool>,
    instances: Vec<Instance>,
    free: Vec<usize>,
    ids: BTreeMap<DisplayId, usize>,
    detached: BTreeMap<u64, Scene>,
    next_sequence: u64,
    dispatcher: EventDispatcher,
    listeners: BTreeMap<ListenerId, ElementHandle>,
    observers: BTreeMap<ObserverId, Observer>,
    last_change: ChangeSet,
    callback_panics: usize,
}
#[derive(Debug, Clone, PartialEq)]
pub struct CloneResult {
    pub root: ElementHandle,
    pub ids: BTreeMap<DisplayId, DisplayId>,
    pub changes: ChangeSet,
}
impl SceneEngine {
    pub fn new(scene: Scene) -> Result<Self, EngineError> {
        Self::with_registry(scene, ElementRegistry::default())
    }
    pub fn with_registry(mut scene: Scene, registry: ElementRegistry) -> Result<Self, EngineError> {
        registry.prepare_scene(&mut scene)?;
        let id = NEXT_ENGINE
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| v.checked_add(1))
            .map_err(|_| EngineError::Exhausted)?;
        let dirty = scene
            .nodes()
            .map(|n| (n.object.id.clone(), DirtyFlags::ALL))
            .collect();
        let (layout, _) = update_layout(&scene, &LayoutSnapshot::default(), &dirty, 0)?;
        let mut engine = Self {
            id,
            scene,
            registry,
            revision: 0,
            document_epoch: 0,
            layout,
            instances: vec![],
            free: vec![],
            ids: BTreeMap::new(),
            detached: BTreeMap::new(),
            next_sequence: 0,
            dispatcher: EventDispatcher::new(),
            listeners: BTreeMap::new(),
            observers: BTreeMap::new(),
            last_change: ChangeSet::default(),
            callback_panics: 0,
            layout_provider: None,
            deriving_layout: std::cell::Cell::new(false),
        };
        let ids = tree_ids(&engine.scene);
        for id in ids {
            engine.allocate(id, InstanceState::Attached)?;
        }
        Ok(engine)
    }
    pub fn scene(&self) -> &Scene {
        &self.scene
    }
    /// Runtime identity shared with ElementHandle::engine_id; never persist it.
    pub fn engine_id(&self) -> u64 {
        self.id
    }
    /// Normalize an inert candidate using this engine's registered definitions,
    /// without committing identity, revisions, observers, or history. Derived
    /// layout providers can measure custom geometry before the final transaction.
    /// Definitions must be pure and deterministic: preparation and the final
    /// commit may invoke them more than once for the same property input.
    pub fn prepare_scene(&self, mut scene: Scene) -> Result<Scene, EngineError> {
        self.registry
            .prepare_changes(&mut scene, Some(&self.scene))?;
        scene.validate()?;
        Ok(scene)
    }
    pub fn registry(&self) -> &ElementRegistry {
        &self.registry
    }
    /// Install/replace runtime configuration without changing the document or
    /// history. The current scene must already be a fixed point of this provider;
    /// perform an explicit undoable initial layout before installing it.
    pub fn install_layout_provider(
        &mut self,
        provider: impl LayoutProvider,
    ) -> Result<LayoutProviderId, EngineError> {
        let stroke_bounds = catch_unwind(AssertUnwindSafe(|| provider.uses_stroke_bounds()))
            .map_err(|_| EngineError::Layout("provider dependency declaration panicked".into()))?;
        let next = crate::derive::run(&provider, self, self.scene.clone())?;
        if next != self.scene {
            return Err(EngineError::Layout(
                "arrange the initial scene before installing a reactive provider".into(),
            ));
        }
        let id = LayoutProviderId {
            engine: self.id,
            sequence: self.sequence()?,
        };
        self.layout_provider = Some(crate::derive::InstalledLayout {
            id,
            provider: std::sync::Arc::new(provider),
            stroke_bounds,
        });
        Ok(id)
    }
    pub fn layout_provider(&self) -> Option<LayoutProviderId> {
        self.layout_provider.as_ref().map(|p| p.id)
    }
    /// Stale/foreign tokens cannot remove a replacement. No document mutation.
    pub fn remove_layout_provider(&mut self, id: LayoutProviderId) -> bool {
        if self.layout_provider() != Some(id) {
            return false;
        }
        self.layout_provider = None;
        true
    }
    fn derive_candidate(
        &self,
        candidate: Scene,
        origin: ChangeOrigin,
    ) -> Result<Scene, EngineError> {
        if !matches!(origin, ChangeOrigin::UndoRedo | ChangeOrigin::Import)
            && let Some(installed) = &self.layout_provider
            && crate::derive::inputs_changed(&self.scene, &candidate, installed.stroke_bounds)
        {
            return crate::derive::run(installed.provider.as_ref(), self, candidate);
        }
        Ok(candidate)
    }
    /// Increments only for explicit new-document reset, independent of later edits.
    pub fn document_epoch(&self) -> u64 {
        self.document_epoch
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn layout(&self) -> &LayoutSnapshot {
        &self.layout
    }
    pub fn last_change(&self) -> &ChangeSet {
        &self.last_change
    }
    /// Number of application observer/disposer panics caught to preserve cleanup.
    pub fn callback_panics(&self) -> usize {
        self.callback_panics
    }
    pub fn handle(&self, id: &DisplayId) -> Option<ElementHandle> {
        self.ids.get(id).map(|&slot| self.make_handle(slot))
    }
    fn make_handle(&self, slot: usize) -> ElementHandle {
        ElementHandle {
            engine: self.id,
            slot,
            generation: self.instances[slot].generation,
        }
    }
    fn check(&self, h: ElementHandle) -> Result<&Instance, EngineError> {
        let instance = self
            .instances
            .get(h.slot)
            .ok_or(EngineError::InvalidHandle)?;
        if h.engine != self.id || h.generation != instance.generation {
            return Err(EngineError::InvalidHandle);
        }
        Ok(instance)
    }
    fn live(&self, h: ElementHandle) -> Result<&Instance, EngineError> {
        let i = self.check(h)?;
        if i.state == InstanceState::Destroyed {
            Err(EngineError::InvalidHandle)
        } else {
            Ok(i)
        }
    }
    fn attached_id(&self, h: ElementHandle) -> Result<DisplayId, EngineError> {
        let i = self.live(h)?;
        if i.state != InstanceState::Attached {
            Err(EngineError::DetachedHandle)
        } else {
            Ok(i.id.clone())
        }
    }
    pub fn instance(&self, h: ElementHandle) -> Result<InstanceInfo, EngineError> {
        let i = self.check(h)?;
        Ok(InstanceInfo {
            handle: h,
            id: i.id.clone(),
            state: i.state,
            attachment_epoch: i.attachment_epoch,
        })
    }
    pub fn object(&self, h: ElementHandle) -> Result<&DisplayObject, EngineError> {
        let i = self.live(h)?;
        if let Some(n) = self.scene.get(&i.id) {
            return Ok(&n.object);
        }
        self.detached
            .values()
            .find_map(|s| s.get(&i.id))
            .map(|n| &n.object)
            .ok_or(EngineError::InvalidHandle)
    }
    fn allocate(
        &mut self,
        id: DisplayId,
        state: InstanceState,
    ) -> Result<ElementHandle, EngineError> {
        let slot = if let Some(slot) = self.free.pop() {
            let generation = self.instances[slot]
                .generation
                .checked_add(1)
                .ok_or(EngineError::Exhausted)?;
            self.instances[slot] = Instance {
                id: id.clone(),
                generation,
                state,
                attachment_epoch: self.revision,
                disposers: BTreeMap::new(),
            };
            slot
        } else {
            let slot = self.instances.len();
            self.instances.push(Instance {
                id: id.clone(),
                generation: 1,
                state,
                attachment_epoch: self.revision,
                disposers: BTreeMap::new(),
            });
            slot
        };
        self.ids.insert(id, slot);
        Ok(self.make_handle(slot))
    }
    fn sequence(&mut self) -> Result<u64, EngineError> {
        self.next_sequence = self
            .next_sequence
            .checked_add(1)
            .ok_or(EngineError::Exhausted)?;
        Ok(self.next_sequence)
    }
    pub fn transaction(
        &mut self,
        edit: impl FnOnce(&mut Scene) -> Result<(), SceneError>,
    ) -> Result<ChangeSet, EngineError> {
        self.transaction_with_origin(ChangeOrigin::Edit, edit)
    }
    pub fn transaction_with_origin(
        &mut self,
        origin: ChangeOrigin,
        edit: impl FnOnce(&mut Scene) -> Result<(), SceneError>,
    ) -> Result<ChangeSet, EngineError> {
        let mut next = self.scene.clone();
        edit(&mut next)?;
        self.apply(next, origin, CommitMode::Normal)
    }
    /// One atomic edit with a final contract check after normalization and
    /// reactive layout, before identities, observers or document state change.
    pub fn transaction_validated(
        &mut self,
        edit: impl FnOnce(&mut Scene) -> Result<(), SceneError>,
        validate: impl FnOnce(&Scene) -> Result<(), SceneError>,
    ) -> Result<ChangeSet, EngineError> {
        let mut next = self.scene.clone();
        edit(&mut next)?;
        self.apply_validated(next, ChangeOrigin::Edit, CommitMode::Normal, validate)
    }
    pub fn replace_scene(&mut self, scene: Scene) -> Result<ChangeSet, EngineError> {
        self.apply(scene, ChangeOrigin::UndoRedo, CommitMode::Normal)
    }
    /// Load a new document, preserving definitions but invalidating every old
    /// instance and scoped resource, even when serialized IDs are reused.
    pub fn reset_scene(&mut self, scene: Scene) -> Result<ChangeSet, EngineError> {
        self.apply(scene, ChangeOrigin::Import, CommitMode::Reset)
    }
    pub fn create(
        &mut self,
        kind: &str,
        id: impl Into<DisplayId>,
        properties: Value,
        parent: Option<ElementHandle>,
        index: Option<usize>,
    ) -> Result<(ElementHandle, ChangeSet), EngineError> {
        let object = self.registry.create(kind, id, properties)?;
        let id = object.id.clone();
        let parent = parent.map(|h| self.attached_id(h)).transpose()?;
        let changes = self.transaction(|scene| scene.insert(object, parent, index))?;
        Ok((self.handle(&id).expect("created"), changes))
    }
    /// A fresh immutable local-distance measurement of an attached Path/Pen.
    /// Retained snapshots do not track edits; query again for current geometry.
    pub fn measure_path(
        &self,
        handle: ElementHandle,
    ) -> Result<gpui_flow_scene::display::PathMeasure, EngineError> {
        self.attached_id(handle)?;
        let path = match &self.object(handle)?.shape {
            Shape::Path(path) => path,
            Shape::Pen(pen) => &pen.path,
            _ => {
                return Err(EngineError::Property(
                    "motion path requires a Path or Pen".into(),
                ));
            }
        };
        path.measure().map_err(EngineError::Property)
    }
    /// Total path length in the element's local coordinates, excluding moves.
    pub fn motion_total(&self, handle: ElementHandle) -> Result<f64, EngineError> {
        Ok(self.measure_path(handle)?.total_length())
    }
    /// Sample a local-space arc distance, returning world position and tangent
    /// through the current committed ancestor transforms. Distance is not world
    /// arc length under scaling. This read-only query never moves another object.
    pub fn motion_point(
        &self,
        handle: ElementHandle,
        distance: f64,
    ) -> Result<Option<gpui_flow_scene::display::PathSample>, EngineError> {
        let id = self.attached_id(handle)?;
        self.measure_path(handle)?
            .sample(distance)
            .map_err(EngineError::Property)?
            .map(|point| {
                point
                    .transformed(self.world_transform(&id)?)
                    .map_err(EngineError::Property)
            })
            .transpose()
    }
    /// Atomically replace local Path/Pen geometry while retaining kind, identity,
    /// transforms and style. Invalid/oversized data leaves the engine unchanged.
    pub fn set_path_data(
        &mut self,
        handle: ElementHandle,
        path: gpui_flow_scene::display::Path,
    ) -> Result<ChangeSet, EngineError> {
        // Validate before serde can replace non-finite floats with JSON null.
        path.to_svg().map_err(EngineError::Property)?;
        let value = serde_json::to_value(path).map_err(|e| EngineError::Property(e.to_string()))?;
        self.set_property(handle, PropertyKey::PathData, value)
    }
    pub fn set_property(
        &mut self,
        handle: ElementHandle,
        key: PropertyKey,
        value: Value,
    ) -> Result<ChangeSet, EngineError> {
        self.set_properties(handle, [(key, value)])
    }
    /// Apply a property batch as one validated revision and change notification.
    pub fn set_properties(
        &mut self,
        handle: ElementHandle,
        properties: impl IntoIterator<Item = (PropertyKey, Value)>,
    ) -> Result<ChangeSet, EngineError> {
        self.set_properties_batch(
            ChangeOrigin::Edit,
            properties
                .into_iter()
                .map(|(key, value)| (handle, key, value)),
        )
    }
    /// Validate a multi-instance property batch without committing revisions,
    /// notifications or lifecycle changes. Definitions remain trusted pure code.
    pub fn validate_properties_batch(
        &self,
        properties: impl IntoIterator<Item = (ElementHandle, PropertyKey, Value)>,
    ) -> Result<(), EngineError> {
        self.validate_properties_batch_with_origin(ChangeOrigin::Edit, properties)
    }
    pub fn validate_properties_batch_with_origin(
        &self,
        origin: ChangeOrigin,
        properties: impl IntoIterator<Item = (ElementHandle, PropertyKey, Value)>,
    ) -> Result<(), EngineError> {
        self.properties_candidate(origin, properties).map(|_| ())
    }
    /// One validated change batch across multiple attached instances, used by
    /// explicit animation/layout controllers. Invalid input changes no instance.
    pub fn set_properties_batch(
        &mut self,
        origin: ChangeOrigin,
        properties: impl IntoIterator<Item = (ElementHandle, PropertyKey, Value)>,
    ) -> Result<ChangeSet, EngineError> {
        let next = self.properties_candidate(origin, properties)?;
        self.apply(next, origin, CommitMode::Normal)
    }
    fn properties_candidate(
        &self,
        origin: ChangeOrigin,
        properties: impl IntoIterator<Item = (ElementHandle, PropertyKey, Value)>,
    ) -> Result<Scene, EngineError> {
        let mut objects = BTreeMap::new();
        let mut animated_geometry = Vec::new();
        for (handle, key, value) in properties {
            let id = self.attached_id(handle)?;
            let object = objects
                .entry(id.clone())
                .or_insert_with(|| self.scene.get(&id).expect("attached").object.clone());
            apply_property(object, &key, value)?;
            if origin == ChangeOrigin::Animation
                && matches!(key, PropertyKey::Transform | PropertyKey::Shape)
            {
                animated_geometry.push((id, key));
            }
        }
        let mut next = self.scene.clone();
        for (id, mut object) in objects {
            // Normalize once per object after all its properties are applied.
            if object.shape != next.get(&id).expect("attached").object.shape
                && let Shape::Custom(custom) = &object.shape
                && let Some(definition) = self.registry.get(&custom.kind)
            {
                object.shape = self
                    .registry
                    .create(definition.kind(), id.clone(), custom.data.clone())?
                    .shape;
            }
            next.update_object(&id, |current| *current = object)?;
        }
        self.registry
            .prepare_changes(&mut next, Some(&self.scene))?;
        next.validate()?;
        let derived = self.derive_candidate(next.clone(), origin)?;
        for (id, key) in animated_geometry {
            let expected = &next.get(&id).expect("attached").object;
            let actual = &derived
                .get(&id)
                .expect("provider preserves topology")
                .object;
            if (key == PropertyKey::Transform && expected.transform != actual.transform)
                || (key == PropertyKey::Shape && expected.shape != actual.shape)
            {
                return Err(EngineError::Layout(format!(
                    "layout owns animated {} on {id}",
                    key.name()
                )));
            }
        }
        Ok(derived)
    }
    pub fn reparent(
        &mut self,
        handle: ElementHandle,
        parent: Option<ElementHandle>,
        index: Option<usize>,
        preserve_world: bool,
    ) -> Result<ChangeSet, EngineError> {
        let id = self.attached_id(handle)?;
        let parent = parent.map(|h| self.attached_id(h)).transpose()?;
        self.transaction(|scene| scene.reparent(&id, parent, index, preserve_world))
    }
    pub fn detach(
        &mut self,
        handle: ElementHandle,
    ) -> Result<(DetachedSubtree, ChangeSet), EngineError> {
        let id = self.attached_id(handle)?;
        let subtree = extract_subtree(&self.scene, &id)?;
        let token = self
            .next_sequence
            .checked_add(1)
            .ok_or(EngineError::Exhausted)?;
        let mut next = self.scene.clone();
        next.remove(&id)?;
        let changes = self.apply(
            next,
            ChangeOrigin::Lifecycle,
            CommitMode::Detach(token, subtree),
        )?;
        self.next_sequence = token;
        Ok((
            DetachedSubtree {
                engine: self.id,
                token,
                root: id,
            },
            changes,
        ))
    }
    pub fn detached_scene(&self, subtree: &DetachedSubtree) -> Result<&Scene, EngineError> {
        if subtree.engine != self.id {
            return Err(EngineError::InvalidSubtree);
        }
        self.detached
            .get(&subtree.token)
            .ok_or(EngineError::InvalidSubtree)
    }
    pub fn attach(
        &mut self,
        subtree: &DetachedSubtree,
        parent: Option<ElementHandle>,
        index: Option<usize>,
    ) -> Result<ChangeSet, EngineError> {
        let detached = self.detached_scene(subtree)?.clone();
        let parent = parent.map(|h| self.attached_id(h)).transpose()?;
        let mut next = self.scene.clone();
        insert_forest(&mut next, &detached, parent, index)?;
        self.apply(
            next,
            ChangeOrigin::Lifecycle,
            CommitMode::Attach(subtree.token),
        )
    }
    pub fn destroy(&mut self, handle: ElementHandle) -> Result<ChangeSet, EngineError> {
        if self.check(handle)?.state == InstanceState::Destroyed {
            return Ok(self.no_change(ChangeOrigin::Lifecycle));
        }
        let id = self.attached_id(handle)?;
        self.transaction_with_origin(ChangeOrigin::Lifecycle, |scene| {
            scene.remove(&id)?;
            Ok(())
        })
    }
    pub fn destroy_detached(
        &mut self,
        subtree: &DetachedSubtree,
    ) -> Result<ChangeSet, EngineError> {
        let ids = tree_ids(self.detached_scene(subtree)?);
        let revision = self.revision.checked_add(1).ok_or(EngineError::Exhausted)?;
        let mut change = ChangeSet {
            before_revision: self.revision,
            revision,
            origin: ChangeOrigin::Lifecycle,
            ..Default::default()
        };
        for id in ids.iter().rev() {
            let handle = self.handle(id).expect("detached instance");
            change.lifecycle.push(LifecycleEvent {
                handle,
                id: id.clone(),
                kind: LifecycleKind::Destroyed,
                old_parent: self
                    .detached_scene(subtree)?
                    .get(id)
                    .and_then(|n| n.parent.clone()),
                new_parent: None,
            });
        }
        self.detached.remove(&subtree.token);
        self.revision = revision;
        self.layout.revision = revision;
        for event in &change.lifecycle {
            self.instances[event.handle.slot].state = InstanceState::Destroyed;
            self.ids.remove(&event.id);
        }
        self.publish(&change);
        for event in &change.lifecycle {
            self.cleanup(event.handle);
        }
        self.last_change = change.clone();
        Ok(change)
    }
    /// Clone attached or detached input state with new document IDs. Runtime
    /// listeners, animation clocks and resource ownership are never copied.
    pub fn clone_subtree(
        &mut self,
        handle: ElementHandle,
        parent: Option<ElementHandle>,
        index: Option<usize>,
    ) -> Result<CloneResult, EngineError> {
        let instance = self.live(handle)?;
        let source = if instance.state == InstanceState::Attached {
            &self.scene
        } else {
            self.detached
                .values()
                .find(|s| s.get(&instance.id).is_some())
                .ok_or(EngineError::InvalidHandle)?
        };
        let source = extract_subtree(source, &instance.id)?;
        let parent = parent.map(|h| self.attached_id(h)).transpose()?;
        let mut mapping = BTreeMap::new();
        let mut reserved: BTreeSet<_> = self.ids.keys().cloned().collect();
        let mut sequence = 0u64;
        for old in tree_ids(&source) {
            let id = loop {
                sequence = sequence.checked_add(1).ok_or(EngineError::Exhausted)?;
                let id = DisplayId::from(format!("{}-copy-{sequence}", old.as_str()));
                if reserved.insert(id.clone()) {
                    break id;
                }
            };
            mapping.insert(old, id);
        }
        let mut clone = Scene::new();
        for old in tree_ids(&source) {
            let node = source.get(&old).expect("source");
            let mut object = node.object.clone();
            object.id = mapping[&old].clone();
            clone.insert(
                object,
                node.parent.as_ref().map(|p| mapping[p].clone()),
                None,
            )?;
        }
        let root = mapping[source.roots().first().expect("subtree root")].clone();
        let mut next = self.scene.clone();
        insert_forest(&mut next, &clone, parent, index)?;
        let changes = self.apply(next, ChangeOrigin::Edit, CommitMode::Normal)?;
        Ok(CloneResult {
            root: self.handle(&root).expect("clone"),
            ids: mapping,
            changes,
        })
    }
    fn no_change(&self, origin: ChangeOrigin) -> ChangeSet {
        ChangeSet {
            before_revision: self.revision,
            revision: self.revision,
            origin,
            ..Default::default()
        }
    }
    fn apply(
        &mut self,
        next: Scene,
        origin: ChangeOrigin,
        mode: CommitMode,
    ) -> Result<ChangeSet, EngineError> {
        self.apply_validated(next, origin, mode, |_| Ok(()))
    }
    fn apply_validated(
        &mut self,
        mut next: Scene,
        origin: ChangeOrigin,
        mode: CommitMode,
        validate: impl FnOnce(&Scene) -> Result<(), SceneError>,
    ) -> Result<ChangeSet, EngineError> {
        self.registry
            .prepare_changes(&mut next, Some(&self.scene))?;
        if !matches!(mode, CommitMode::Reset) {
            next = self.derive_candidate(next, origin)?;
        }
        validate(&next)?;
        // Detached IDs remain reserved. Only the matching attach operation may
        // reuse them; ordinary history/import cannot alias retained instances.
        let attaching = match &mode {
            CommitMode::Attach(token) => Some(
                self.detached
                    .get(token)
                    .ok_or(EngineError::InvalidSubtree)?,
            ),
            _ => None,
        };
        for node in next.nodes() {
            if let Some(handle) = self.handle(&node.object.id)
                && self.check(handle)?.state == InstanceState::Detached
                && !matches!(mode, CommitMode::Reset)
                && attaching.is_none_or(|s| s.get(&node.object.id).is_none())
            {
                return Err(EngineError::Scene(SceneError::DuplicateId(
                    node.object.id.clone(),
                )));
            }
        }
        if self.scene == next && !matches!(mode, CommitMode::Reset) {
            return Ok(self.no_change(origin));
        }
        let document_epoch = if matches!(mode, CommitMode::Reset) {
            self.document_epoch
                .checked_add(1)
                .ok_or(EngineError::Exhausted)?
        } else {
            self.document_epoch
        };
        let revision = self.revision.checked_add(1).ok_or(EngineError::Exhausted)?;
        let mut change = diff(
            &self.scene,
            &next,
            &self.registry,
            self.revision,
            revision,
            origin,
        )?;
        let (layout, damage) = update_layout(&next, &self.layout, &change.dirty, revision)?;
        change.layout = layout.stats;
        change.damage = damage;
        let old_ids = tree_ids(&self.scene);
        let new_ids = tree_ids(&next);
        let replaced: BTreeSet<_> = old_ids
            .iter()
            .filter(|id| {
                next.get(id).is_some_and(|n| {
                    matches!(mode, CommitMode::Reset)
                        || element_kind(&n.object)
                            != element_kind(&self.scene.get(id).expect("old").object)
                })
            })
            .cloned()
            .collect();
        let removed: Vec<_> = old_ids
            .iter()
            .filter(|id| next.get(id).is_none() || replaced.contains(*id))
            .cloned()
            .collect();
        let added: Vec<_> = new_ids
            .iter()
            .filter(|id| self.scene.get(id).is_none() || replaced.contains(*id))
            .cloned()
            .collect();
        // Validate possible slot generation exhaustion before committing anything.
        if self
            .free
            .iter()
            .chain(removed.iter().filter_map(|id| self.ids.get(id)))
            .any(|&slot| self.instances[slot].generation == u64::MAX)
        {
            return Err(EngineError::Exhausted);
        }
        let mut cleanup = vec![];
        if matches!(mode, CommitMode::Reset) {
            let detached_ids: Vec<_> = self.detached.values().flat_map(tree_ids).collect();
            for id in detached_ids.into_iter().rev() {
                let handle = self.handle(&id).expect("detached");
                self.instances[handle.slot].state = InstanceState::Destroyed;
                self.ids.remove(&id);
                cleanup.push(handle);
                change.lifecycle.push(LifecycleEvent {
                    handle,
                    id,
                    kind: LifecycleKind::Destroyed,
                    old_parent: None,
                    new_parent: None,
                });
            }
            self.detached.clear();
        }
        for id in removed.iter().rev() {
            let handle = self.handle(id).expect("old handle");
            let parent = self.scene.get(id).and_then(|n| n.parent.clone());
            change.lifecycle.push(LifecycleEvent {
                handle,
                id: id.clone(),
                kind: LifecycleKind::Unmounted,
                old_parent: parent.clone(),
                new_parent: None,
            });
            if matches!(mode, CommitMode::Detach(..)) {
                self.instances[handle.slot].state = InstanceState::Detached;
                change.lifecycle.push(LifecycleEvent {
                    handle,
                    id: id.clone(),
                    kind: LifecycleKind::Detached,
                    old_parent: parent,
                    new_parent: None,
                });
            } else {
                self.instances[handle.slot].state = InstanceState::Destroyed;
                self.ids.remove(id);
                cleanup.push(handle);
                change.lifecycle.push(LifecycleEvent {
                    handle,
                    id: id.clone(),
                    kind: LifecycleKind::Destroyed,
                    old_parent: parent,
                    new_parent: None,
                });
            }
        }
        for id in &added {
            let (handle, created) = if let Some(handle) = self.handle(id) {
                self.instances[handle.slot].state = InstanceState::Attached;
                (handle, false)
            } else {
                (self.allocate(id.clone(), InstanceState::Attached)?, true)
            };
            let parent = next.get(id).and_then(|n| n.parent.clone());
            self.instances[handle.slot].attachment_epoch = revision;
            if created {
                change.lifecycle.push(LifecycleEvent {
                    handle,
                    id: id.clone(),
                    kind: LifecycleKind::Created,
                    old_parent: None,
                    new_parent: parent.clone(),
                });
            }
            change.lifecycle.push(LifecycleEvent {
                handle,
                id: id.clone(),
                kind: LifecycleKind::Mounted,
                old_parent: None,
                new_parent: parent,
            });
        }
        for structural in &change.structure {
            if structural.kind == StructureKind::Reparented && !replaced.contains(&structural.id) {
                change.lifecycle.push(LifecycleEvent {
                    handle: self.handle(&structural.id).expect("retained"),
                    id: structural.id.clone(),
                    kind: LifecycleKind::Reparented,
                    old_parent: structural.old_parent.clone(),
                    new_parent: structural.new_parent.clone(),
                });
            }
        }
        if matches!(mode, CommitMode::Reset) {
            self.layout_provider = None;
        }
        match mode {
            CommitMode::Detach(token, subtree) => {
                self.detached.insert(token, subtree);
            }
            CommitMode::Attach(token) => {
                self.detached.remove(&token);
            }
            CommitMode::Normal | CommitMode::Reset => {}
        }
        self.scene = next;
        self.document_epoch = document_epoch;
        self.layout = layout;
        self.revision = revision;
        self.publish(&change);
        for handle in cleanup {
            self.cleanup(handle);
        }
        self.last_change = change.clone();
        Ok(change)
    }
    pub fn on(
        &mut self,
        handle: ElementHandle,
        kind: EventKind,
        options: ListenerOptions,
        handler: impl FnMut(&mut SceneEvent) + 'static,
    ) -> Result<ListenerId, EngineError> {
        let id = self.live(handle)?.id.clone();
        let raw = self.dispatcher.on(id, kind, options, handler);
        let token = ListenerId {
            engine: self.id,
            raw,
        };
        self.listeners.insert(token, handle);
        Ok(token)
    }
    pub fn off(&mut self, id: ListenerId) -> bool {
        if id.engine != self.id {
            return false;
        }
        self.listeners.remove(&id);
        self.dispatcher.off(id.raw)
    }
    pub fn listener_scope_count(&self) -> usize {
        self.listeners.len()
    }
    pub fn property_descriptor(
        &self,
        handle: ElementHandle,
        key: &PropertyKey,
    ) -> Result<PropertyDescriptor, EngineError> {
        if let PropertyKey::Custom(name) = key {
            let object = self.object(handle)?;
            let Shape::Custom(custom) = &object.shape else {
                return Err(EngineError::Property("not a custom element".into()));
            };
            return self
                .registry
                .get(&custom.kind)
                .and_then(|d| d.schema().descriptor(name).cloned())
                .ok_or_else(|| {
                    EngineError::Property(format!("no declared custom property {name}"))
                });
        }
        self.live(handle)?;
        key.descriptor()
            .ok_or_else(|| EngineError::Property("unknown property".into()))
    }
    pub fn listener_count(&self) -> usize {
        self.dispatcher.len()
    }
    pub fn dispatch(&mut self, event: SceneEvent) -> Result<DispatchResult, EngineError> {
        let result = self.dispatcher.dispatch(&self.scene, event);
        self.listeners
            .retain(|id, _| self.dispatcher.contains_listener(id.raw));
        Ok(result?)
    }
    pub fn on_change(
        &mut self,
        owner: Option<ElementHandle>,
        handler: impl FnMut(&ChangeSet) + 'static,
    ) -> Result<ObserverId, EngineError> {
        if let Some(owner) = owner {
            self.live(owner)?;
        }
        let id = ObserverId {
            engine: self.id,
            sequence: self.sequence()?,
        };
        self.observers.insert(
            id,
            Observer {
                owner,
                callback: Box::new(handler),
            },
        );
        Ok(id)
    }
    pub fn off_change(&mut self, id: ObserverId) -> bool {
        id.engine == self.id && self.observers.remove(&id).is_some()
    }
    pub fn add_disposer(
        &mut self,
        owner: ElementHandle,
        dispose: impl FnOnce() + 'static,
    ) -> Result<DisposerId, EngineError> {
        self.live(owner)?;
        let id = DisposerId {
            engine: self.id,
            sequence: self.sequence()?,
        };
        self.instances[owner.slot]
            .disposers
            .insert(id, Box::new(dispose));
        Ok(id)
    }
    pub fn dispose(&mut self, id: DisposerId) -> bool {
        if id.engine != self.id {
            return false;
        }
        for instance in &mut self.instances {
            if let Some(disposer) = instance.disposers.remove(&id) {
                if catch_unwind(AssertUnwindSafe(disposer)).is_err() {
                    self.callback_panics += 1;
                }
                return true;
            }
        }
        false
    }
    fn publish(&mut self, change: &ChangeSet) {
        for observer in self.observers.values_mut() {
            // Instance observers receive only their own deltas/lifecycle or
            // destruction; a global observer receives the whole change stream.
            let relevant = observer.owner.is_none_or(|h| {
                change.lifecycle.iter().any(|e| e.handle == h)
                    || self.instances.get(h.slot).is_some_and(|i| {
                        i.generation == h.generation && change.dirty.contains_key(&i.id)
                    })
            });
            if relevant && catch_unwind(AssertUnwindSafe(|| (observer.callback)(change))).is_err() {
                self.callback_panics += 1;
            }
        }
    }
    fn cleanup(&mut self, handle: ElementHandle) {
        let listener_ids: Vec<_> = self
            .listeners
            .iter()
            .filter(|(_, owner)| **owner == handle)
            .map(|(id, _)| *id)
            .collect();
        for id in listener_ids {
            self.off(id);
        }
        self.observers
            .retain(|_, observer| observer.owner != Some(handle));
        let disposers = std::mem::take(&mut self.instances[handle.slot].disposers);
        for disposer in disposers.into_values() {
            if catch_unwind(AssertUnwindSafe(disposer)).is_err() {
                self.callback_panics += 1;
            }
        }
        if !self.free.contains(&handle.slot) {
            self.free.push(handle.slot);
        }
    }
}
impl Drop for SceneEngine {
    fn drop(&mut self) {
        for instance in &mut self.instances {
            for disposer in std::mem::take(&mut instance.disposers).into_values() {
                let _ = catch_unwind(AssertUnwindSafe(disposer));
            }
        }
    }
}
fn element_kind(object: &DisplayObject) -> (&str, Option<&str>) {
    match &object.shape {
        Shape::Custom(c) => ("custom", Some(&c.kind)),
        shape => (shape.tag(), None),
    }
}
