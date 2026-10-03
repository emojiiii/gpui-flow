use crate::layout::intersection;
use crate::subtree::{paint_ids, tree_ids};
use crate::{ChangeSet, ElementHandle, EngineError, InstanceState, LayoutEntry, SceneEngine};
use gpui_flow_scene::{
    Affine, Bounds, ClipRegion, DisplayId, HitPolicy, HitTestOptions, HitTestResult, PaintItem,
    Point, Scene, SceneError, Shape,
};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Stable depth-first pre-order. Paint order sorts siblings by z-index, retaining
/// insertion order for ties; it does not filter hidden, locked, or clipped nodes.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum QueryOrder {
    #[default]
    Tree,
    Paint,
}

/// Exact, case-sensitive AND conditions, not a selector language. A subtree is
/// an attached instance, includes its root by default, and rejects stale handles.
/// Tags are the serialized Shape::tag() names (including "custom"). Classes are
/// individual members of DisplayObject::classes. No condition means all nodes.
#[derive(Debug, Clone, Copy)]
pub struct ElementQuery<'a> {
    pub id: Option<&'a DisplayId>,
    pub name: Option<&'a str>,
    pub tag: Option<&'a str>,
    pub class: Option<&'a str>,
    pub subtree: Option<ElementHandle>,
    pub include_root: bool,
    pub order: QueryOrder,
}
impl Default for ElementQuery<'_> {
    fn default() -> Self {
        Self {
            id: None,
            name: None,
            tag: None,
            class: None,
            subtree: None,
            include_root: true,
            order: QueryOrder::Tree,
        }
    }
}

/// Deterministic work counters, not elapsed time or a complexity guarantee for
/// whole commits. Metadata is reconciled only for the commit's dirty IDs.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct QueryIndexStats {
    pub metadata_examined: usize,
    pub metadata_updated: usize,
    pub order_nodes_rebuilt: usize,
}
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct QueryStats {
    /// Nodes tested from the smallest available bucket/subtree, before filtering.
    pub candidates_tested: usize,
    pub matches: usize,
}
#[derive(Debug, Clone, Copy)]
struct Position {
    tree: usize,
    end: usize,
    paint: usize,
}
/// IDs, never handles: identity is resolved from the engine after every query.
/// Detached instances are deliberately absent, even though their IDs are reserved.
#[derive(Default)]
pub(crate) struct QueryIndex {
    names: BTreeMap<String, BTreeSet<DisplayId>>,
    tags: BTreeMap<String, BTreeSet<DisplayId>>,
    classes: BTreeMap<String, BTreeSet<DisplayId>>,
    tree: Vec<DisplayId>,
    pub(crate) paint: Vec<DisplayId>,
    // Lookup only: output order is always determined by the explicit ranks.
    positions: HashMap<DisplayId, Position>,
    pub(crate) stats: QueryIndexStats,
}
impl QueryIndex {
    pub(crate) fn new(scene: &Scene) -> Self {
        let mut index = Self::default();
        for node in scene.nodes() {
            index.insert(&node.object);
        }
        index.rebuild_order(scene);
        index.stats.metadata_examined = scene.len();
        index.stats.metadata_updated = scene.len();
        index
    }
    fn insert(&mut self, object: &gpui_flow_scene::DisplayObject) {
        self.names
            .entry(object.name.clone())
            .or_default()
            .insert(object.id.clone());
        self.tags
            .entry(object.shape.tag().into())
            .or_default()
            .insert(object.id.clone());
        for class in &object.classes {
            self.classes
                .entry(class.clone())
                .or_default()
                .insert(object.id.clone());
        }
    }
    fn remove(&mut self, object: &gpui_flow_scene::DisplayObject) {
        fn remove(map: &mut BTreeMap<String, BTreeSet<DisplayId>>, key: &str, id: &DisplayId) {
            if let Some(ids) = map.get_mut(key) {
                ids.remove(id);
                if ids.is_empty() {
                    map.remove(key);
                }
            }
        }
        remove(&mut self.names, &object.name, &object.id);
        remove(&mut self.tags, object.shape.tag(), &object.id);
        for class in &object.classes {
            remove(&mut self.classes, class, &object.id);
        }
    }
    fn rebuild_order(&mut self, scene: &Scene) {
        self.tree = tree_ids(scene);
        self.paint = paint_ids(scene);
        self.positions = self
            .tree
            .iter()
            .enumerate()
            .map(|(rank, id)| {
                (
                    id.clone(),
                    Position {
                        tree: rank,
                        end: rank + 1,
                        paint: 0,
                    },
                )
            })
            .collect();
        for id in self.tree.iter().rev() {
            let end = self.positions[id].end;
            if let Some(parent) = &scene.get(id).expect("attached").parent {
                let position = self.positions.get_mut(parent).expect("parent");
                position.end = position.end.max(end);
            }
        }
        for (rank, id) in self.paint.iter().enumerate() {
            self.positions.get_mut(id).expect("attached").paint = rank;
        }
        self.stats.order_nodes_rebuilt = scene.len();
    }
    /// Called only after the complete candidate (including layout) is validated.
    pub(crate) fn reconcile(&mut self, old: &Scene, new: &Scene, change: &ChangeSet) {
        self.stats = QueryIndexStats::default();
        for id in change.dirty.keys() {
            self.stats.metadata_examined += 1;
            let before = old.get(id).map(|n| &n.object);
            let after = new.get(id).map(|n| &n.object);
            let unchanged = match (before, after) {
                (Some(a), Some(b)) => {
                    a.name == b.name && a.classes == b.classes && a.shape.tag() == b.shape.tag()
                }
                (None, None) => true,
                _ => false,
            };
            if !unchanged {
                if let Some(object) = before {
                    self.remove(object);
                }
                if let Some(object) = after {
                    self.insert(object);
                }
                self.stats.metadata_updated += 1;
            }
        }
        if !change.structure.is_empty() || change.properties.iter().any(|p| p.property == "z_index")
        {
            self.rebuild_order(new);
        }
    }
}

enum Candidates<'a> {
    Slice(&'a [DisplayId]),
    Set(&'a BTreeSet<DisplayId>),
}
impl<'a> Candidates<'a> {
    fn len(&self) -> usize {
        match self {
            Self::Slice(ids) => ids.len(),
            Self::Set(ids) => ids.len(),
        }
    }
    fn iter(&self) -> Box<dyn Iterator<Item = &'a DisplayId> + '_> {
        match self {
            Self::Slice(ids) => Box::new(ids.iter()),
            Self::Set(ids) => Box::new(ids.iter()),
        }
    }
}

impl SceneEngine {
    pub fn query_index_stats(&self) -> QueryIndexStats {
        self.queries.stats
    }
    /// Query only attached nodes and resolve their current generational identity.
    pub fn query(&self, query: ElementQuery<'_>) -> Result<Vec<ElementHandle>, EngineError> {
        self.query_with_stats(query).map(|(handles, _)| handles)
    }
    pub fn query_with_stats(
        &self,
        query: ElementQuery<'_>,
    ) -> Result<(Vec<ElementHandle>, QueryStats), EngineError> {
        let index = &self.queries;
        // Validate scope even when another condition has an empty bucket.
        let scope = query
            .subtree
            .map(|handle| {
                let info = self.instance(handle)?;
                match info.state {
                    InstanceState::Destroyed => return Err(EngineError::InvalidHandle),
                    InstanceState::Detached => return Err(EngineError::DetachedHandle),
                    InstanceState::Attached => {}
                }
                let position = index.positions.get(&info.id).expect("attached indexed");
                Ok((
                    position.tree + usize::from(!query.include_root),
                    position.end,
                ))
            })
            .transpose()?;
        let ordered = match query.order {
            QueryOrder::Tree => &index.tree,
            QueryOrder::Paint => &index.paint,
        };
        let mut candidates = Candidates::Slice(
            scope.map_or(ordered.as_slice(), |(start, end)| &index.tree[start..end]),
        );
        for (key, map) in [
            (query.name, &index.names),
            (query.tag, &index.tags),
            (query.class, &index.classes),
        ] {
            if let Some(key) = key {
                let Some(bucket) = map.get(key) else {
                    return Ok((vec![], QueryStats::default()));
                };
                if bucket.len() < candidates.len() {
                    candidates = Candidates::Set(bucket);
                }
            }
        }
        if let Some(id) = query.id {
            let Some((id, _)) = index.positions.get_key_value(id) else {
                return Ok((vec![], QueryStats::default()));
            };
            candidates = Candidates::Slice(std::slice::from_ref(id));
        }
        let mut stats = QueryStats::default();
        let mut matches = vec![];
        for id in candidates.iter() {
            stats.candidates_tested += 1;
            let position = index.positions[id];
            if scope.is_some_and(|(start, end)| position.tree < start || position.tree >= end) {
                continue;
            }
            let object = &self.scene().get(id).expect("attached indexed").object;
            if query.name.is_some_and(|name| object.name != name)
                || query.tag.is_some_and(|tag| object.shape.tag() != tag)
                || query
                    .class
                    .is_some_and(|class| !object.classes.contains(class))
            {
                continue;
            }
            matches.push(id);
        }
        matches.sort_by_key(|id| match query.order {
            QueryOrder::Tree => index.positions[*id].tree,
            QueryOrder::Paint => index.positions[*id].paint,
        });
        stats.matches = matches.len();
        let handles = matches
            .into_iter()
            .map(|id| self.handle(id).expect("attached identity"))
            .collect();
        Ok((handles, stats))
    }

    pub fn world_transform(&self, id: &DisplayId) -> Result<Affine, EngineError> {
        Ok(self.entry(id)?.world_transform)
    }
    pub fn local_bounds(&self, id: &DisplayId) -> Result<Option<Bounds>, EngineError> {
        Ok(self.entry(id)?.local_bounds)
    }
    pub fn world_bounds(&self, id: &DisplayId) -> Result<Option<Bounds>, EngineError> {
        Ok(self.entry(id)?.world_bounds)
    }
    /// Conservative visual subtree extent. Geometry APIs deliberately exclude shadows.
    pub fn effect_bounds(&self, id: &DisplayId) -> Result<Option<Bounds>, EngineError> {
        Ok(self.entry(id)?.effect_bounds)
    }
    fn entry(&self, id: &DisplayId) -> Result<&LayoutEntry, EngineError> {
        self.layout()
            .get(id)
            .ok_or_else(|| SceneError::MissingNode(id.clone()).into())
    }
    pub fn local_to_world(&self, id: &DisplayId, point: Point) -> Result<Point, EngineError> {
        checked_point(self.entry(id)?.world_transform.transform_point(point))
    }
    pub fn world_to_local(&self, id: &DisplayId, point: Point) -> Result<Point, EngineError> {
        let inverse = self
            .entry(id)?
            .inverse
            .ok_or_else(|| SceneError::NonInvertibleTransform(id.clone()))?;
        checked_point(inverse.transform_point(point))
    }
    /// Uses committed cached matrices/opacity/clips; no layout or Scene::validate
    /// is repeated per frame. Flattening/tessellation remains a backend concern.
    pub fn paint_list(&self) -> Result<Vec<PaintItem<'_>>, EngineError> {
        if self.scene().has_transfer_context() {
            return Err(SceneError::InvalidGeometry(
                "transient subtree context must be inserted before rendering".into(),
            )
            .into());
        }
        let mut result = vec![];
        for id in &self.queries.paint {
            let object = &self.scene().get(id).expect("node").object;
            let e = self.entry(id)?;
            if !e.visible || e.opacity <= 0. {
                continue;
            }
            result.push(PaintItem {
                object,
                world_transform: e.world_transform,
                stroke_scale: if object.style.has_scale_fixed_strokes() {
                    self.scene().stroke_scale(id)?
                } else {
                    gpui_flow_scene::StrokeScaleData::IDENTITY
                },
                stroke_camera: self.scene().resolved_stroke_camera(),
                opacity: e.opacity,
                locked: e.locked,
                clips: e.clips.clone(),
                hit_enabled: e.hit_enabled,
            });
        }
        Ok(result)
    }
    /// Conservative visible positive geometry, without subtracting erased pixels.
    /// Eraser roots and their complete subtrees do not contribute to this extent.
    pub fn visible_bounds(&self) -> Result<Option<Bounds>, EngineError> {
        let mut bounds = None;
        let mut eraser_subtrees = BTreeSet::new();
        for item in self.paint_list()? {
            let node = self.scene().get(&item.object.id).expect("node");
            if item.object.eraser != gpui_flow_scene::EraserMode::None
                || node
                    .parent
                    .as_ref()
                    .is_some_and(|parent| eraser_subtrees.contains(parent))
            {
                eraser_subtrees.insert(item.object.id.clone());
                continue;
            }
            let mut next = self
                .scene()
                .resolved_object(
                    &item.object.id,
                    self.scene().resolved_stroke_camera().unwrap_or(1.),
                )?
                .paint_bounds()
                .map(|b| item.world_transform.transform_bounds(b));
            for clip in item.clips {
                next = next.and_then(|b| intersection(b, clip.world_bounds()));
            }
            if let Some(b) = next {
                bounds = Some(bounds.map_or(b, |a: Bounds| a.union(b)));
            }
        }
        Ok(bounds)
    }
    pub fn resolved_for_view(&self, camera: f32) -> Result<Self, EngineError> {
        Self::with_registry(
            self.scene().resolved_for_view(camera)?,
            self.registry().clone(),
        )
    }
    pub fn hit_test_for_view(
        &self,
        world: Point,
        options: HitTestOptions,
        camera: f32,
    ) -> Result<Option<HitTestResult>, EngineError> {
        self.resolved_for_view(camera)?.hit_test(world, options)
    }
    pub fn visible_bounds_for_view(&self, camera: f32) -> Result<Option<Bounds>, EngineError> {
        self.resolved_for_view(camera)?.visible_bounds()
    }
    pub fn hit_test(
        &self,
        world: Point,
        options: HitTestOptions,
    ) -> Result<Option<HitTestResult>, EngineError> {
        Ok(self.hit_test_all(world, options)?.into_iter().next())
    }
    pub fn hit_test_all(
        &self,
        world: Point,
        options: HitTestOptions,
    ) -> Result<Vec<HitTestResult>, EngineError> {
        self.hit_test_all_with_image_candidates(world, options, &BTreeSet::new())
    }
    /// Ordered broad-phase candidates for a runtime Image pixel picker. Only
    /// the explicitly supplied Image IDs may additionally use their rounded
    /// outer box, avoiding stale intrinsic-size letterboxing. Callers must
    /// supply only current, sampleable decoded frames and filter their alpha;
    /// pending/failed fallback must retain ordinary `hit_test_all` geometry.
    /// All visibility, lock, hit-policy, transform and ancestor-clip gates are
    /// identical to `hit_test_all`; non-Images and default picking are unchanged.
    pub fn hit_test_all_with_image_candidates(
        &self,
        world: Point,
        options: HitTestOptions,
        image_ids: &BTreeSet<DisplayId>,
    ) -> Result<Vec<HitTestResult>, EngineError> {
        if !world.is_finite() || !options.tolerance.is_finite() || options.tolerance < 0. {
            return Err(
                SceneError::InvalidGeometry("invalid hit point or tolerance".into()).into(),
            );
        }
        let mut hits = vec![];
        for id in self.queries.paint.iter().rev() {
            let object = &self.scene().get(id).expect("node").object;
            let e = self.entry(id)?;
            if !e.hit_enabled
                || (!options.include_locked && e.locked)
                || (!options.include_invisible && (!e.visible || e.opacity <= 0.))
                || object.hit_policy == HitPolicy::None
                || e.clips.iter().any(|c| !c.contains(world))
                || (matches!(object.shape, Shape::Text(_) | Shape::Image(_))
                    && !ClipRegion::has_visible_area(
                        e.world_transform.transform_bounds(
                            object.shape.local_bounds().expect("text/image bounds"),
                        ),
                        &e.clips,
                    ))
            {
                continue;
            }
            let Some(inverse) = e.inverse else {
                continue;
            };
            let local = inverse.transform_point(world);
            if !local.is_finite() {
                continue;
            }
            let tolerance =
                (options.tolerance as f64 * inverse.max_scale() as f64).min(f32::MAX as f64) as f32;
            let custom = match &object.shape {
                Shape::Custom(c)
                    if object.hit_policy != HitPolicy::Bounds
                        && c.bounds.expanded(tolerance).contains(local) =>
                {
                    self.registry().get(&c.kind).and_then(|d| {
                        d.schema()
                            .resolve(&c.data)
                            .ok()
                            .and_then(|p| d.hit_test(&p, local, tolerance))
                    })
                }
                _ => None,
            };
            let resolved = self
                .scene()
                .resolved_object(id, self.scene().resolved_stroke_camera().unwrap_or(1.))?;
            let hit = custom.unwrap_or_else(|| match object.hit_policy {
                HitPolicy::None => false,
                HitPolicy::Bounds => e
                    .local_bounds
                    .is_some_and(|b| b.expanded(tolerance).contains(local)),
                HitPolicy::Geometry => {
                    object
                        .shape
                        .contains_geometry(local, object.style.fill_rule)
                        || near_contours(&object.shape, local, tolerance)
                }
                HitPolicy::Painted => resolved.shape.hit_test(local, &resolved.style, tolerance),
            });
            let image_candidate = image_ids.contains(id)
                && matches!(&object.shape, Shape::Image(image) if image.contains(local));
            if hit || image_candidate {
                hits.push(HitTestResult {
                    id: id.clone(),
                    world_point: world,
                    local_point: local,
                    path: self.scene().ancestor_path(id)?,
                });
            }
        }
        Ok(hits)
    }
}
fn checked_point(point: Point) -> Result<Point, EngineError> {
    if point.is_finite() {
        Ok(point)
    } else {
        Err(SceneError::InvalidGeometry("coordinate overflow".into()).into())
    }
}
fn near_contours(shape: &Shape, point: Point, tolerance: f32) -> bool {
    if tolerance <= 0. {
        return false;
    }
    shape.flatten(0.1).iter().any(|c| {
        let hit = c
            .points
            .windows(2)
            .any(|p| segment_distance(point, p[0], p[1]) <= tolerance);
        hit || (c.closed
            && c.points.len() > 1
            && segment_distance(point, *c.points.last().unwrap(), c.points[0]) <= tolerance)
    })
}
fn segment_distance(p: Point, a: Point, b: Point) -> f32 {
    let d = b - a;
    let len = d.x * d.x + d.y * d.y;
    if len <= f32::EPSILON {
        return p.distance(a);
    }
    let t = (((p.x - a.x) * d.x + (p.y - a.y) * d.y) / len).clamp(0., 1.);
    p.distance(a + d * t)
}
