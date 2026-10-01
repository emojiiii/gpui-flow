use crate::{DirtyFlags, LayoutStats};
use gpui_flow_scene::{Affine, Bounds, ClipRegion, DisplayId, Scene, SceneError};
use std::collections::{BTreeMap, BTreeSet};
/// Cached geometry. `local_bounds` is the clipped geometric subtree including
/// strokes; `content_bounds` excludes stroke. No unsupported effects are inferred.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutEntry {
    pub world_transform: Affine,
    pub inverse: Option<Affine>,
    pub content_bounds: Option<Bounds>,
    pub local_bounds: Option<Bounds>,
    pub world_bounds: Option<Bounds>,
    /// Conservative world-space visual bounds, including descendant shadows.
    /// Kept separate from geometric layout, selection and hit bounds.
    pub effect_bounds: Option<Bounds>,
    pub opacity: f32,
    pub visible: bool,
    pub locked: bool,
    pub hit_enabled: bool,
    pub clips: Vec<ClipRegion>,
    pub revision: u64,
}
#[derive(Debug, Default, Clone, PartialEq)]
pub struct LayoutSnapshot {
    pub revision: u64,
    pub(crate) entries: BTreeMap<DisplayId, LayoutEntry>,
    pub stats: LayoutStats,
}
impl LayoutSnapshot {
    pub fn get(&self, id: &DisplayId) -> Option<&LayoutEntry> {
        self.entries.get(id)
    }
    pub fn entries(&self) -> impl Iterator<Item = (&DisplayId, &LayoutEntry)> {
        self.entries.iter()
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
pub(crate) fn intersection(a: Bounds, b: Bounds) -> Option<Bounds> {
    let min = gpui_flow_scene::Point::new(a.min_x().max(b.min_x()), a.min_y().max(b.min_y()));
    let max = gpui_flow_scene::Point::new(a.max_x().min(b.max_x()), a.max_y().min(b.max_y()));
    (max.x >= min.x && max.y >= min.y).then(|| Bounds::from_points(min, max))
}
#[derive(Clone)]
struct ParentState {
    matrix: Affine,
    opacity: f32,
    visible: bool,
    locked: bool,
    hit: bool,
    clips: Vec<ClipRegion>,
}
impl Default for ParentState {
    fn default() -> Self {
        Self {
            matrix: Affine::IDENTITY,
            opacity: 1.,
            visible: true,
            locked: false,
            hit: true,
            clips: vec![],
        }
    }
}
struct Builder<'a> {
    scene: &'a Scene,
    old: &'a LayoutSnapshot,
    dirty: &'a BTreeMap<DisplayId, DirtyFlags>,
    next: LayoutSnapshot,
    affected: BTreeSet<DisplayId>,
}
impl Builder<'_> {
    fn visit(
        &mut self,
        id: &DisplayId,
        parent: &ParentState,
        parent_changed: bool,
    ) -> Result<bool, SceneError> {
        self.next.stats.visited += 1;
        let node = self.scene.get(id).expect("validated scene");
        let object = &node.object;
        let flags = self.dirty.get(id).copied().unwrap_or_default();
        let old = self.old.get(id);
        let world_changed = old.is_none()
            || parent_changed
            || old.is_some_and(|entry| {
                entry.locked != (parent.locked || object.locked)
                    || entry.hit_enabled != parent.hit
                    || entry.clips != parent.clips
                    || entry.visible != (parent.visible && object.visible)
            })
            || flags
                .intersects(DirtyFlags::TRANSFORM | DirtyFlags::OPACITY | DirtyFlags::STRUCTURE);
        let mut entry = if !world_changed {
            old.expect("cached").clone()
        } else {
            self.next.stats.world_recomputed += 1;
            let matrix = parent.matrix * object.transform;
            if !matrix.is_finite() {
                return Err(SceneError::InvalidGeometry(
                    "world transform overflow".into(),
                ));
            }
            LayoutEntry {
                world_transform: matrix,
                inverse: matrix.inverse(),
                content_bounds: None,
                local_bounds: None,
                world_bounds: None,
                effect_bounds: None,
                opacity: parent.opacity * object.style.opacity,
                visible: parent.visible && object.visible,
                locked: parent.locked || object.locked,
                hit_enabled: parent.hit,
                clips: parent.clips.clone(),
                revision: self.next.revision,
            }
        };
        let clip = object.shape.child_clip();
        let mut clips = entry.clips.clone();
        if let Some((bounds, corner_radius)) = clip {
            clips.push(ClipRegion {
                local_bounds: bounds,
                corner_radius,
                world_transform: entry.world_transform,
            });
        }
        let child_parent = ParentState {
            matrix: entry.world_transform,
            opacity: entry.opacity,
            visible: entry.visible,
            locked: entry.locked,
            hit: entry.hit_enabled && object.hit_children,
            clips,
        };
        let children = node.children.clone();
        let mut child_bounds_changed = false;
        for child in &children {
            child_bounds_changed |= self.visit(child, &child_parent, world_changed)?;
        }
        let bounds_changed = old.is_none()
            || child_bounds_changed
            || flags.intersects(DirtyFlags::GEOMETRY | DirtyFlags::STRUCTURE);
        if bounds_changed {
            self.next.stats.bounds_recomputed += 1;
            entry.content_bounds = object.shape.local_bounds();
            entry.local_bounds = object.paint_bounds();
            for child in &children {
                if let Some(bounds) = self.next.entries[child].local_bounds {
                    let bounds = self
                        .scene
                        .get(child)
                        .expect("child")
                        .object
                        .transform
                        .transform_bounds(bounds);
                    let bounds = clip.map_or(Some(bounds), |(c, _)| intersection(bounds, c));
                    if let Some(bounds) = bounds {
                        entry.local_bounds =
                            Some(entry.local_bounds.map_or(bounds, |b| b.union(bounds)));
                    }
                }
            }
        } else if world_changed {
            entry.content_bounds = old.and_then(|e| e.content_bounds);
            entry.local_bounds = old.and_then(|e| e.local_bounds);
        }
        if world_changed || bounds_changed {
            entry.world_bounds = entry
                .local_bounds
                .map(|b| entry.world_transform.transform_bounds(b));
        }
        if entry.local_bounds.is_some_and(|b| !b.is_valid())
            || entry.world_bounds.is_some_and(|b| !b.is_valid())
        {
            return Err(SceneError::InvalidGeometry("cached bounds overflow".into()));
        }
        entry.effect_bounds = entry.world_bounds.map(|b| {
            object
                .style
                .shadow
                .map_or(b, |s| s.expand_bounds(b, entry.world_transform))
        });
        for child in &children {
            let mut bounds = self.next.entries[child].effect_bounds;
            if let Some((c, _)) = clip {
                bounds =
                    bounds.and_then(|b| intersection(b, entry.world_transform.transform_bounds(c)));
            }
            if let Some(b) = bounds {
                entry.effect_bounds = Some(entry.effect_bounds.map_or(b, |a| a.union(b)));
            }
        }
        if entry.effect_bounds.is_some_and(|b| !b.is_valid()) {
            return Err(SceneError::InvalidGeometry(
                "cached effect bounds overflow".into(),
            ));
        }
        let visually_dirty = flags.intersects(
            DirtyFlags::PAINT
                | DirtyFlags::TRANSFORM
                | DirtyFlags::GEOMETRY
                | DirtyFlags::OPACITY
                | DirtyFlags::ORDER
                | DirtyFlags::STRUCTURE,
        );
        if flags.intersects(DirtyFlags::PAINT) {
            self.next.stats.paint_invalidated += 1;
        }
        if world_changed
            || bounds_changed
            || visually_dirty
            || old.is_none_or(|e| e.effect_bounds != entry.effect_bounds)
        {
            entry.revision = self.next.revision;
            self.affected.insert(id.clone());
        }
        self.next.entries.insert(id.clone(), entry);
        Ok(bounds_changed || flags.intersects(DirtyFlags::TRANSFORM | DirtyFlags::STRUCTURE))
    }
}
pub(crate) fn update_layout(
    scene: &Scene,
    old: &LayoutSnapshot,
    dirty: &BTreeMap<DisplayId, DirtyFlags>,
    revision: u64,
) -> Result<(LayoutSnapshot, Vec<Bounds>), SceneError> {
    let mut builder = Builder {
        scene,
        old,
        dirty,
        next: LayoutSnapshot {
            revision,
            ..Default::default()
        },
        affected: BTreeSet::new(),
    };
    for root in scene.roots() {
        builder.visit(root, &ParentState::default(), false)?;
    }
    for id in old.entries.keys() {
        if !builder.next.entries.contains_key(id) {
            builder.affected.insert(id.clone());
        }
    }
    let mut damage = vec![];
    for id in &builder.affected {
        for entry in [old.get(id), builder.next.get(id)].into_iter().flatten() {
            if !entry.visible || entry.opacity <= 0. {
                continue;
            }
            let mut bounds = entry.effect_bounds;
            for clip in &entry.clips {
                bounds = bounds.and_then(|b| intersection(b, clip.world_bounds()));
            }
            if let Some(b) = bounds
                && !damage.contains(&b)
            {
                damage.push(b);
            }
        }
    }
    Ok((builder.next, damage))
}
