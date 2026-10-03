use crate::{Affine, Bounds, DisplayId, DisplayObject, HitPolicy, Point};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

pub const SCENE_VERSION: u32 = 1;
pub const MAX_SCENE_DEPTH: usize = 256;
fn version() -> u32 {
    SCENE_VERSION
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SceneIssue {
    pub path: String,
    pub message: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SceneError {
    MissingNode(DisplayId),
    DuplicateId(DisplayId),
    InvalidParent(DisplayId),
    InvalidIndex(usize),
    Cycle,
    NonInvertibleTransform(DisplayId),
    InvalidGeometry(String),
    Validation(Vec<SceneIssue>),
    Serialization(String),
    DepthLimit,
}
impl fmt::Display for SceneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingNode(id) => write!(f, "display object not found: {id}"),
            Self::DuplicateId(id) => write!(f, "duplicate display id: {id}"),
            Self::InvalidParent(id) => write!(f, "object cannot contain children: {id}"),
            Self::InvalidIndex(i) => write!(f, "invalid child index {i}"),
            Self::Cycle => write!(f, "scene tree contains a cycle"),
            Self::NonInvertibleTransform(id) => write!(f, "transform is not invertible: {id}"),
            Self::InvalidGeometry(s) => write!(f, "invalid scene geometry: {s}"),
            Self::Serialization(s) => write!(f, "scene serialization: {s}"),
            Self::DepthLimit => write!(f, "scene exceeds maximum depth of {MAX_SCENE_DEPTH}"),
            Self::Validation(issues) => {
                write!(f, "scene validation failed")?;
                for i in issues {
                    write!(f, "; {}: {}", i.path, i.message)?;
                }
                Ok(())
            }
        }
    }
}
impl std::error::Error for SceneError {}
impl From<serde_json::Error> for SceneError {
    fn from(e: serde_json::Error) -> Self {
        Self::Serialization(e.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SceneNode {
    pub object: DisplayObject,
    pub parent: Option<DisplayId>,
    pub children: Vec<DisplayId>,
}
/// A forest of ordered display trees. Mutators are atomic, preserve bidirectional
/// parent/child links, and reject invalid geometry or cyclic hierarchy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    #[serde(default = "version")]
    pub version: u32,
    #[serde(default)]
    nodes: BTreeMap<DisplayId, SceneNode>,
    #[serde(default)]
    roots: Vec<DisplayId>,
    #[serde(default)]
    pub data: Value,
}
impl Default for Scene {
    fn default() -> Self {
        Self {
            version: SCENE_VERSION,
            nodes: BTreeMap::new(),
            roots: Vec::new(),
            data: Value::Null,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClipRegion {
    pub local_bounds: Bounds,
    pub corner_radius: f32,
    pub corner_radii: Option<crate::display::CornerRadii>,
    pub world_transform: Affine,
}
impl ClipRegion {
    /// Local rectangle geometry before applying this clip's origin/transform.
    pub fn local_rect(self) -> crate::display::Rect {
        crate::display::Rect {
            size: self.local_bounds.size,
            corner_radius: self.corner_radius,
            corner_radii: self.corner_radii,
        }
    }
    /// Native Text/Image may use only genuinely rectangular clips. An explicit
    /// zero array overrides a nonzero scalar but invalid radii never qualify.
    pub fn is_rectangular(self) -> bool {
        self.corner_radius.is_finite()
            && self.corner_radius >= 0.
            && self
                .corner_radii
                .map_or(self.corner_radius == 0., |r| r.0.iter().all(|v| *v == 0.))
    }
    /// Broad-phase visibility for a positive-area object rectangle. Exact
    /// rounded/transformed point containment still uses each clip's contains.
    /// Edge-only contact is empty; boundaries of nonempty rectangles retain the
    /// inclusive Bounds::contains convention.
    pub fn has_visible_area(mut bounds: Bounds, clips: &[Self]) -> bool {
        if !bounds.is_valid() || !bounds.size.is_positive() {
            return false;
        }
        for clip in clips {
            let clip_bounds = clip.world_bounds();
            if !clip_bounds.is_valid() {
                return false;
            }
            let Some(next) = crate::geometry::intersection(bounds, clip_bounds) else {
                return false;
            };
            if !next.is_valid() || !next.size.is_positive() {
                return false;
            }
            bounds = next;
        }
        true
    }
    pub fn world_bounds(self) -> Bounds {
        self.world_transform.transform_bounds(self.local_bounds)
    }
    pub fn contains(self, world: Point) -> bool {
        self.world_transform.inverse().is_some_and(|inverse| {
            self.local_rect()
                .contains(inverse.transform_point(world) - self.local_bounds.origin)
        })
    }
}
#[derive(Debug, Clone)]
pub struct PaintItem<'a> {
    pub object: &'a DisplayObject,
    pub world_transform: Affine,
    pub opacity: f32,
    pub locked: bool,
    pub clips: Vec<ClipRegion>,
    pub hit_enabled: bool,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HitTestOptions {
    pub tolerance: f32,
    pub include_locked: bool,
    pub include_invisible: bool,
}
impl Default for HitTestOptions {
    fn default() -> Self {
        Self {
            tolerance: 0.0,
            include_locked: false,
            include_invisible: false,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct HitTestResult {
    pub id: DisplayId,
    pub world_point: Point,
    pub local_point: Point,
    pub path: Vec<DisplayId>,
}
impl Scene {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn get(&self, id: &DisplayId) -> Option<&SceneNode> {
        self.nodes.get(id)
    }
    pub fn roots(&self) -> &[DisplayId] {
        &self.roots
    }
    pub fn nodes(&self) -> impl Iterator<Item = &SceneNode> {
        self.nodes.values()
    }
    pub fn len(&self) -> usize {
        self.nodes.len()
    }
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
    pub fn find_by_name<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a DisplayObject> {
        self.nodes
            .values()
            .map(|n| &n.object)
            .filter(move |o| o.name == name)
    }
    pub fn find_by_class<'a>(&'a self, class: &'a str) -> impl Iterator<Item = &'a DisplayObject> {
        self.nodes
            .values()
            .map(|n| &n.object)
            .filter(move |o| o.classes.contains(class))
    }
    fn siblings_mut(
        &mut self,
        parent: Option<&DisplayId>,
    ) -> Result<&mut Vec<DisplayId>, SceneError> {
        match parent {
            None => Ok(&mut self.roots),
            Some(id) => {
                let node = self
                    .nodes
                    .get_mut(id)
                    .ok_or_else(|| SceneError::MissingNode(id.clone()))?;
                if !node.object.shape.allows_children() {
                    return Err(SceneError::InvalidParent(id.clone()));
                }
                Ok(&mut node.children)
            }
        }
    }
    pub fn insert(
        &mut self,
        object: DisplayObject,
        parent: Option<DisplayId>,
        index: Option<usize>,
    ) -> Result<(), SceneError> {
        if self.nodes.contains_key(&object.id) {
            return Err(SceneError::DuplicateId(object.id));
        }
        object.validate().map_err(SceneError::InvalidGeometry)?;
        let mut candidate = self.clone();
        let siblings = candidate.siblings_mut(parent.as_ref())?;
        let index = index.unwrap_or(siblings.len());
        if index > siblings.len() {
            return Err(SceneError::InvalidIndex(index));
        }
        siblings.insert(index, object.id.clone());
        candidate.nodes.insert(
            object.id.clone(),
            SceneNode {
                object,
                parent,
                children: Vec::new(),
            },
        );
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }
    /// Update common properties or replace a primitive. Identity cannot change;
    /// replacing a container with a leaf while it has children is rejected.
    pub fn update_object(
        &mut self,
        id: &DisplayId,
        update: impl FnOnce(&mut DisplayObject),
    ) -> Result<(), SceneError> {
        let mut candidate = self.clone();
        let node = candidate
            .nodes
            .get_mut(id)
            .ok_or_else(|| SceneError::MissingNode(id.clone()))?;
        update(&mut node.object);
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }
    /// Remove a complete subtree, returning objects in parent-first order.
    pub fn remove(&mut self, id: &DisplayId) -> Result<Vec<DisplayObject>, SceneError> {
        let node = self
            .nodes
            .get(id)
            .ok_or_else(|| SceneError::MissingNode(id.clone()))?;
        let mut candidate = self.clone();
        candidate
            .siblings_mut(node.parent.as_ref())?
            .retain(|child| child != id);
        let mut removed = Vec::new();
        let mut stack = vec![id.clone()];
        let mut seen = BTreeSet::new();
        while let Some(id) = stack.pop() {
            if !seen.insert(id.clone()) {
                return Err(SceneError::Cycle);
            }
            let node = candidate
                .nodes
                .remove(&id)
                .ok_or_else(|| SceneError::MissingNode(id.clone()))?;
            stack.extend(node.children.into_iter().rev());
            removed.push(node.object);
        }
        candidate.validate()?;
        *self = candidate;
        Ok(removed)
    }
    /// Move a subtree. `index` is relative to the destination after removal.
    /// Keeping world geometry requires the new parent's world matrix to invert.
    pub fn reparent(
        &mut self,
        id: &DisplayId,
        parent: Option<DisplayId>,
        index: Option<usize>,
        preserve_world: bool,
    ) -> Result<(), SceneError> {
        let node = self
            .nodes
            .get(id)
            .ok_or_else(|| SceneError::MissingNode(id.clone()))?;
        if let Some(parent) = &parent
            && self.ancestor_path(parent)?.contains(id)
        {
            return Err(SceneError::Cycle);
        }
        let transform = if preserve_world {
            let world = self.world_transform(id)?;
            let inverse = if let Some(parent) = &parent {
                self.world_transform(parent)?
                    .inverse()
                    .ok_or_else(|| SceneError::NonInvertibleTransform(parent.clone()))?
            } else {
                Affine::IDENTITY
            };
            Some(inverse * world)
        } else {
            None
        };
        let mut candidate = self.clone();
        candidate
            .siblings_mut(node.parent.as_ref())?
            .retain(|child| child != id);
        let siblings = candidate.siblings_mut(parent.as_ref())?;
        let index = index.unwrap_or(siblings.len());
        if index > siblings.len() {
            return Err(SceneError::InvalidIndex(index));
        }
        siblings.insert(index, id.clone());
        let object = candidate.nodes.get_mut(id).expect("existing node");
        object.parent = parent;
        if let Some(transform) = transform {
            object.object.transform = transform;
        }
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }
    /// Root-to-target ancestor path, including target.
    pub fn ancestor_path(&self, id: &DisplayId) -> Result<Vec<DisplayId>, SceneError> {
        let mut path = Vec::new();
        let mut current = Some(id);
        let mut visited = BTreeSet::new();
        while let Some(id) = current {
            if !visited.insert(id) {
                return Err(SceneError::Cycle);
            }
            if path.len() >= MAX_SCENE_DEPTH {
                return Err(SceneError::DepthLimit);
            }
            let node = self
                .nodes
                .get(id)
                .ok_or_else(|| SceneError::MissingNode(id.clone()))?;
            path.push(id.clone());
            current = node.parent.as_ref();
        }
        path.reverse();
        Ok(path)
    }
    pub fn world_transform(&self, id: &DisplayId) -> Result<Affine, SceneError> {
        let mut transform = Affine::IDENTITY;
        for id in self.ancestor_path(id)? {
            transform = transform * self.nodes[&id].object.transform;
            if !transform.is_finite() {
                return Err(SceneError::InvalidGeometry(
                    "world transform overflow".into(),
                ));
            }
        }
        Ok(transform)
    }
    pub fn local_to_world(&self, id: &DisplayId, point: Point) -> Result<Point, SceneError> {
        let result = self.world_transform(id)?.transform_point(point);
        if !result.is_finite() {
            return Err(SceneError::InvalidGeometry("coordinate overflow".into()));
        }
        Ok(result)
    }
    pub fn world_to_local(&self, id: &DisplayId, point: Point) -> Result<Point, SceneError> {
        let inverse = self
            .world_transform(id)?
            .inverse()
            .ok_or_else(|| SceneError::NonInvertibleTransform(id.clone()))?;
        let result = inverse.transform_point(point);
        if !result.is_finite() {
            return Err(SceneError::InvalidGeometry("coordinate overflow".into()));
        }
        Ok(result)
    }
    /// Geometric subtree bounds in the object's local coordinate space. Hidden
    /// children are included; clipping and stroke extents are respected. Eraser
    /// child subtrees do not expand their parent, but retain their own bounds.
    /// Alpha masks keep conservative bounds rather than clipping sibling bounds.
    pub fn local_bounds(&self, id: &DisplayId) -> Result<Option<Bounds>, SceneError> {
        self.validate()?;
        self.local_bounds_inner(id)
    }
    fn local_bounds_inner(&self, id: &DisplayId) -> Result<Option<Bounds>, SceneError> {
        let node = self
            .nodes
            .get(id)
            .ok_or_else(|| SceneError::MissingNode(id.clone()))?;
        let mut result = node.object.paint_bounds();
        let clip = node.object.shape.child_clip().map(|(bounds, _, _)| bounds);
        for child in &node.children {
            if let Some(bounds) = self.local_bounds_inner(child)? {
                let bounds = self.nodes[child].object.transform.transform_bounds(bounds);
                if !bounds.is_valid() {
                    return Err(SceneError::InvalidGeometry("child bounds overflow".into()));
                }
                if self.nodes[child].object.eraser != crate::EraserMode::None {
                    continue;
                }
                let bounds = if let Some(clip) = clip {
                    crate::geometry::intersection(bounds, clip)
                } else {
                    Some(bounds)
                };
                if let Some(bounds) = bounds {
                    result = Some(result.map_or(bounds, |r| r.union(bounds)));
                }
            }
        }
        if result.is_some_and(|b| !b.is_valid()) {
            return Err(SceneError::InvalidGeometry(
                "subtree bounds overflow".into(),
            ));
        }
        Ok(result)
    }
    pub fn world_bounds(&self, id: &DisplayId) -> Result<Option<Bounds>, SceneError> {
        let transform = self.world_transform(id)?;
        let bounds = self
            .local_bounds(id)?
            .map(|b| transform.transform_bounds(b));
        if bounds.is_some_and(|b| !b.is_valid()) {
            return Err(SceneError::InvalidGeometry("world bounds overflow".into()));
        }
        Ok(bounds)
    }
    fn ordered<'a>(&'a self, ids: &'a [DisplayId]) -> Vec<&'a DisplayId> {
        let mut ids: Vec<_> = ids.iter().collect();
        ids.sort_by_key(|id| self.nodes[*id].object.z_index);
        ids
    }
    /// Structural object ordering; z_index sorts only among siblings and equal
    /// z values preserve insertion order. Each object occurs once. A container
    /// may paint its stroke after descendants, so this is not a draw-op list.
    pub fn paint_order(&self) -> Result<Vec<DisplayId>, SceneError> {
        Ok(self
            .paint_list()?
            .into_iter()
            .map(|item| item.object.id.clone())
            .collect())
    }
    pub fn paint_list(&self) -> Result<Vec<PaintItem<'_>>, SceneError> {
        self.paint_list_inner(false)
    }
    fn paint_list_inner(&self, include_invisible: bool) -> Result<Vec<PaintItem<'_>>, SceneError> {
        self.validate()?;
        let mut result = Vec::new();
        let mut stack = Vec::new();
        for id in self.ordered(&self.roots).into_iter().rev() {
            stack.push((
                id,
                Affine::IDENTITY,
                1.0f32,
                false,
                Vec::<ClipRegion>::new(),
                true,
            ));
        }
        while let Some((id, parent, opacity, locked, clips, hit_enabled)) = stack.pop() {
            let node = &self.nodes[id];
            let object = &node.object;
            let opacity = opacity * object.style.opacity;
            if !include_invisible && (!object.visible || opacity <= 0.0) {
                continue;
            }
            let world_transform = parent * object.transform;
            let locked = locked || object.locked;
            result.push(PaintItem {
                object,
                world_transform,
                opacity,
                locked,
                clips: clips.clone(),
                hit_enabled,
            });
            let mut child_clips = clips;
            if let Some((local_bounds, corner_radius, corner_radii)) = object.shape.child_clip() {
                child_clips.push(ClipRegion {
                    corner_radii,
                    local_bounds,
                    corner_radius,
                    world_transform,
                });
            }
            for child in self.ordered(&node.children).into_iter().rev() {
                stack.push((
                    child,
                    world_transform,
                    opacity,
                    locked,
                    child_clips.clone(),
                    hit_enabled && object.hit_children,
                ));
            }
        }
        Ok(result)
    }
    /// Conservative visible positive geometry, without subtracting erased pixels.
    /// Eraser roots and their complete subtrees do not contribute to this extent.
    pub fn visible_bounds(&self) -> Result<Option<Bounds>, SceneError> {
        let mut result = None;
        let mut eraser_subtrees = BTreeSet::new();
        for item in self.paint_list()? {
            let node = &self.nodes[&item.object.id];
            if item.object.eraser != crate::EraserMode::None
                || node
                    .parent
                    .as_ref()
                    .is_some_and(|parent| eraser_subtrees.contains(parent))
            {
                eraser_subtrees.insert(item.object.id.clone());
                continue;
            }
            if let Some(bounds) = item.object.paint_bounds() {
                let mut bounds = Some(item.world_transform.transform_bounds(bounds));
                for clip in &item.clips {
                    bounds =
                        bounds.and_then(|b| crate::geometry::intersection(b, clip.world_bounds()));
                }
                if let Some(bounds) = bounds {
                    result = Some(result.map_or(bounds, |r: Bounds| r.union(bounds)));
                }
            }
        }
        Ok(result)
    }
    pub fn hit_test(
        &self,
        world: Point,
        options: HitTestOptions,
    ) -> Result<Option<HitTestResult>, SceneError> {
        Ok(self.hit_test_all(world, options)?.into_iter().next())
    }
    /// Front-to-back hits. Picking respects inherited visibility, opacity,
    /// locks, hitChildren, nested transformed clips, and non-invertible matrices.
    pub fn hit_test_all(
        &self,
        world: Point,
        options: HitTestOptions,
    ) -> Result<Vec<HitTestResult>, SceneError> {
        if !world.is_finite() || !options.tolerance.is_finite() || options.tolerance < 0.0 {
            return Err(SceneError::InvalidGeometry(
                "invalid hit point or tolerance".into(),
            ));
        }
        let mut result = Vec::new();
        for item in self
            .paint_list_inner(options.include_invisible)?
            .into_iter()
            .rev()
        {
            if !item.hit_enabled
                || (!options.include_locked && item.locked)
                || item.object.hit_policy == HitPolicy::None
                || item.clips.iter().any(|clip| !clip.contains(world))
                || (matches!(
                    item.object.shape,
                    crate::Shape::Text(_) | crate::Shape::Image(_)
                ) && !ClipRegion::has_visible_area(
                    item.world_transform.transform_bounds(
                        item.object.shape.local_bounds().expect("text/image bounds"),
                    ),
                    &item.clips,
                ))
            {
                continue;
            }
            let Some(inverse) = item.world_transform.inverse() else {
                continue;
            };
            let local = inverse.transform_point(world);
            if !local.is_finite() {
                continue;
            }
            let tolerance = if options.tolerance == 0.0 {
                0.0
            } else {
                (options.tolerance as f64 * inverse.max_scale() as f64).min(f32::MAX as f64) as f32
            };
            let hit = match item.object.hit_policy {
                HitPolicy::None => false,
                HitPolicy::Bounds => self
                    .local_bounds_inner(&item.object.id)?
                    .is_some_and(|b| b.expanded(tolerance).contains(local)),
                HitPolicy::Geometry => {
                    item.object
                        .shape
                        .contains_geometry(local, item.object.style.fill_rule)
                        || crate::geometry::stroke_hit(
                            &item.object.shape.flatten(0.1),
                            local,
                            tolerance,
                        )
                }
                HitPolicy::Painted => {
                    item.object
                        .shape
                        .hit_test(local, &item.object.style, tolerance)
                }
            };
            if hit {
                result.push(HitTestResult {
                    id: item.object.id.clone(),
                    world_point: world,
                    local_point: local,
                    path: self.ancestor_path(&item.object.id)?,
                });
            }
        }
        Ok(result)
    }
    pub fn validation_issues(&self) -> Vec<SceneIssue> {
        let mut issues = Vec::new();
        let mut push = |path: String, message: String| issues.push(SceneIssue { path, message });
        if self.version != SCENE_VERSION {
            push(
                "version".into(),
                format!("unsupported scene version {}", self.version),
            );
        }
        let mut roots = BTreeSet::new();
        for id in &self.roots {
            if !roots.insert(id) {
                push("roots".into(), format!("duplicate root {id}"));
            }
            match self.nodes.get(id) {
                None => push("roots".into(), format!("missing root {id}")),
                Some(node) if node.parent.is_some() => {
                    push("roots".into(), format!("root {id} has a parent"))
                }
                _ => {}
            }
        }
        for (id, node) in &self.nodes {
            let path = format!("nodes.{id}");
            if id != &node.object.id {
                push(path.clone(), "map key and display id differ".into());
            }
            if let Err(error) = node.object.validate() {
                push(path.clone(), error);
            }
            if node.object.mask != crate::MaskMode::None
                && !node
                    .parent
                    .as_ref()
                    .and_then(|id| self.nodes.get(id))
                    .is_some_and(|parent| matches!(parent.object.shape, crate::Shape::Group(_)))
            {
                push(
                    path.clone(),
                    "alpha masks require a direct Group parent".into(),
                );
            }
            if !node.object.shape.allows_children() && !node.children.is_empty() {
                push(
                    path.clone(),
                    "leaf primitive cannot contain children".into(),
                );
            }
            match &node.parent {
                Some(parent) => match self.nodes.get(parent) {
                    None => push(path.clone(), format!("missing parent {parent}")),
                    Some(p) if !p.children.contains(id) => {
                        push(path.clone(), "parent does not reference child".into())
                    }
                    _ => {}
                },
                None if !roots.contains(id) => push(
                    path.clone(),
                    "parentless object is missing from roots".into(),
                ),
                _ => {}
            }
            let mut children = BTreeSet::new();
            for child in &node.children {
                if !children.insert(child) {
                    push(path.clone(), format!("duplicate child {child}"));
                }
                match self.nodes.get(child) {
                    None => push(path.clone(), format!("missing child {child}")),
                    Some(c) if c.parent.as_ref() != Some(id) => push(
                        path.clone(),
                        format!("child {child} has a mismatched parent"),
                    ),
                    _ => {}
                }
            }
        }
        // Iterative traversal validates reachability/depth and accumulated geometry
        // without trusting a deserialized tree enough to recurse through it.
        let mut stack: Vec<_> = self
            .roots
            .iter()
            .map(|id| (id, 1usize, Affine::IDENTITY))
            .collect();
        let mut seen = BTreeSet::new();
        let mut total = None;
        while let Some((id, depth, parent)) = stack.pop() {
            if !seen.insert(id) {
                push(
                    format!("nodes.{id}"),
                    "object is reachable more than once or tree is cyclic".into(),
                );
                continue;
            }
            if depth > MAX_SCENE_DEPTH {
                push(format!("nodes.{id}"), "maximum scene depth exceeded".into());
                continue;
            }
            let Some(node) = self.nodes.get(id) else {
                continue;
            };
            let world = parent * node.object.transform;
            if !world.is_finite() {
                push(
                    format!("nodes.{id}.transform"),
                    "accumulated world transform overflow".into(),
                );
            }
            if let Some(bounds) = node.object.paint_bounds() {
                let bounds = world.transform_bounds(bounds);
                if !bounds.is_valid() {
                    push(
                        format!("nodes.{id}.bounds"),
                        "transformed bounds overflow".into(),
                    );
                } else {
                    total = Some(total.map_or(bounds, |r: Bounds| r.union(bounds)));
                }
            }
            for child in &node.children {
                stack.push((child, depth + 1, world));
            }
        }
        if seen.len() != self.nodes.len() {
            push(
                "nodes".into(),
                "scene has unreachable or cyclic objects".into(),
            );
        }
        if total.is_some_and(|b| !b.is_valid()) {
            push("bounds".into(), "aggregate scene bounds overflow".into());
        }
        // Only recurse after the untrusted hierarchy has passed the iterative
        // checks. A shrinking ancestor must not hide overflow in local bounds.
        if issues.is_empty() {
            for root in &self.roots {
                if let Err(error) = self.local_bounds_inner(root) {
                    issues.push(SceneIssue {
                        path: format!("nodes.{root}.bounds"),
                        message: error.to_string(),
                    });
                }
            }
        }
        issues
    }
    pub fn validate(&self) -> Result<(), SceneError> {
        let issues = self.validation_issues();
        if issues.is_empty() {
            Ok(())
        } else {
            Err(SceneError::Validation(issues))
        }
    }
    pub fn to_json(&self) -> Result<String, SceneError> {
        self.validate()?;
        Ok(serde_json::to_string_pretty(self)?)
    }
    pub fn from_json(json: &str) -> Result<Self, SceneError> {
        let scene: Self = serde_json::from_str(json)?;
        scene.validate()?;
        Ok(scene)
    }
}
