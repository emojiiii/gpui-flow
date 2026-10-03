//! Shared mask layout and picking policy for retained and cached scenes.
use crate::{
    Affine, Bounds, DisplayId, HitPolicy, HitTestResult, MaskMode, Point, Scene, SceneError,
};
use std::collections::{BTreeMap, BTreeSet};
type MaskParentGeometry = BTreeMap<DisplayId, Option<(Affine, Bounds)>>;

impl Scene {
    /// Only visible, positive-opacity direct markers enable sibling masking.
    pub fn has_active_masks(&self, id: &DisplayId) -> bool {
        self.get(id).is_some_and(|node| {
            node.children.iter().any(|child| {
                self.get(child).is_some_and(|child| {
                    let object = &child.object;
                    object.mask != MaskMode::None && object.visible && object.style.opacity > 0.
                })
            })
        })
    }

    /// Geometric bounds use stable z-order through the first marker, inclusive,
    /// when masks are active. Otherwise eraser subtrees do not expand bounds.
    /// Rendering/effect allocation remains conservative and is separate.
    pub fn mask_layout_children(&self, id: &DisplayId) -> Result<Vec<&DisplayId>, SceneError> {
        let node = self
            .get(id)
            .ok_or_else(|| SceneError::MissingNode(id.clone()))?;
        for child in &node.children {
            if self.get(child).is_none() {
                return Err(SceneError::MissingNode(child.clone()));
            }
        }
        let mut children: Vec<_> = node.children.iter().collect();
        if self.has_active_masks(id) {
            children.sort_by_key(|id| self.get(id).expect("child").object.z_index);
            if let Some(index) = children
                .iter()
                .position(|id| self.get(id).expect("child").object.mask != MaskMode::None)
            {
                children.truncate(index + 1);
            }
        } else {
            children
                .retain(|id| self.get(id).expect("child").object.eraser == crate::EraserMode::None);
        }
        Ok(children)
    }

    /// A path marker picked on behalf of a mask-only parent uses its contour,
    /// even without paint. ClippingPath retains ordinary self-paint picking.
    pub fn mask_hit_uses_geometry(&self, id: &DisplayId) -> bool {
        self.get(id).is_some_and(|node| {
            node.object.mask == MaskMode::Path
                && node.parent.as_ref().is_some_and(|parent| {
                    self.get(parent).is_some_and(|p| !p.object.hit_children)
                        && self.has_active_masks(parent)
                })
        })
    }

    /// Apply hitChildren and mask rules after geometric/resource hit filters.
    /// Candidates must be front-to-back, with visibility, locks, clipping and
    /// own HitPolicy already checked. Keeping original IDs until this point lets
    /// hosts sample Image alpha before proxying a hit to its mask-only parent.
    pub fn resolve_mask_hits(
        &self,
        candidates: Vec<HitTestResult>,
        world_tolerance: f32,
    ) -> Result<Vec<HitTestResult>, SceneError> {
        if !world_tolerance.is_finite()
            || world_tolerance < 0.
            || candidates.iter().any(|hit| !hit.world_point.is_finite())
        {
            return Err(SceneError::InvalidGeometry("invalid mask hit query".into()));
        }
        let mut geometry = MaskParentGeometry::new();
        let mut result = Vec::new();
        let mut seen = BTreeSet::new();
        for hit in &candidates {
            let node = self
                .get(&hit.id)
                .ok_or_else(|| SceneError::MissingNode(hit.id.clone()))?;
            if node.object.mask != MaskMode::None
                && !node.object.mask.paints_self()
                && let Some(parent) = node.parent.as_ref().filter(|id| self.has_active_masks(id))
            {
                let overlaps_content =
                    self.get(parent)
                        .expect("parent")
                        .children
                        .iter()
                        .any(|sibling| {
                            self.get(sibling).expect("sibling").object.mask == MaskMode::None
                                && candidates.iter().any(|other| {
                                    other.path.contains(sibling)
                                        && self
                                            .mask_hit_target(
                                                &other.id,
                                                Some(parent),
                                                other.world_point,
                                                world_tolerance,
                                                &mut geometry,
                                            )
                                            .is_some()
                                })
                        });
                if !overlaps_content {
                    continue;
                }
            }
            let Some(target) = self.mask_hit_target(
                &hit.id,
                None,
                hit.world_point,
                world_tolerance,
                &mut geometry,
            ) else {
                continue;
            };
            if !seen.insert(target.clone()) {
                continue;
            }
            if target == hit.id {
                result.push(hit.clone());
            } else if self.get(&target).expect("target").object.hit_policy != HitPolicy::None {
                let Some(inverse) = self.world_transform(&target)?.inverse() else {
                    continue;
                };
                let local = inverse.transform_point(hit.world_point);
                if local.is_finite() {
                    result.push(HitTestResult {
                        id: target.clone(),
                        world_point: hit.world_point,
                        local_point: local,
                        path: self.ancestor_path(&target)?,
                    });
                }
            }
        }
        Ok(result)
    }

    fn mask_hit_target(
        &self,
        id: &DisplayId,
        stop_at: Option<&DisplayId>,
        world: Point,
        tolerance: f32,
        geometry: &mut MaskParentGeometry,
    ) -> Option<DisplayId> {
        let mut child = id;
        let mut target = id.clone();
        let mut remaining = crate::MAX_SCENE_DEPTH;
        while let Some(parent) = self.get(child)?.parent.as_ref() {
            if remaining == 0 {
                return None;
            }
            remaining -= 1;
            if Some(parent) == stop_at {
                break;
            }
            if !self.get(parent)?.object.hit_children {
                if !self.has_active_masks(parent) || self.get(child)?.object.mask == MaskMode::None
                {
                    return None;
                }
                // onlyHitMask branches retain their own geometric broad phase;
                // hitChildren=true is the separate ignoreHitWorld case upstream.
                let entry = geometry.entry(parent.clone()).or_insert_with(|| {
                    self.world_transform(parent)
                        .ok()?
                        .inverse()
                        .zip(self.local_bounds_inner(parent).ok()?)
                });
                let (inverse, bounds) = entry.as_ref()?;
                let local = inverse.transform_point(world);
                let radius = if tolerance == 0. {
                    0.
                } else {
                    (tolerance as f64 * inverse.max_scale() as f64).min(f32::MAX as f64) as f32
                };
                if !local.is_finite() || !bounds.expanded(radius).contains(local) {
                    return None;
                }
                target = parent.clone();
            }
            child = parent;
        }
        Some(target)
    }
}
