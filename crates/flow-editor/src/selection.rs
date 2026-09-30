use gpui_flow_scene::{DisplayId, Scene};
use std::collections::BTreeSet;
/// IDs rather than cloned objects, so selection survives valid scene edits.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SceneSelection {
    pub ids: BTreeSet<DisplayId>,
}
impl SceneSelection {
    pub fn clear(&mut self) {
        self.ids.clear();
    }
    pub fn select(&mut self, id: DisplayId, additive: bool) {
        if !additive {
            self.clear();
        }
        self.ids.insert(id);
    }
    pub fn toggle(&mut self, id: DisplayId) {
        if !self.ids.remove(&id) {
            self.ids.insert(id);
        }
    }
    pub fn retain_existing(&mut self, scene: &Scene) {
        self.ids.retain(|id| scene.get(id).is_some());
    }
    /// Selected descendants of selected ancestors are excluded to prevent double transformations.
    pub fn roots(&self, scene: &Scene) -> Vec<DisplayId> {
        self.ids
            .iter()
            .filter(|id| {
                if scene.get(id).is_none() {
                    return false;
                }
                let mut parent = scene.get(id).and_then(|n| n.parent.as_ref());
                while let Some(id) = parent {
                    if self.ids.contains(id) {
                        return false;
                    }
                    parent = scene.get(id).and_then(|n| n.parent.as_ref());
                }
                true
            })
            .cloned()
            .collect()
    }
}

impl SceneSelection {
    /// Conservative rectangle selection using visible clipped bounds and picking policies.
    /// Rounded/rotated masks use their AABBs; point picking remains exact.
    pub fn in_rect(
        scene: &Scene,
        rect: gpui_flow_core::Rect,
    ) -> Result<Self, gpui_flow_scene::SceneError> {
        use gpui_flow_scene::HitPolicy;
        fn intersect(
            a: gpui_flow_core::Rect,
            b: gpui_flow_core::Rect,
        ) -> Option<gpui_flow_core::Rect> {
            let min =
                gpui_flow_core::Point::new(a.min_x().max(b.min_x()), a.min_y().max(b.min_y()));
            let max =
                gpui_flow_core::Point::new(a.max_x().min(b.max_x()), a.max_y().min(b.max_y()));
            (max.x >= min.x && max.y >= min.y).then(|| gpui_flow_core::Rect::from_points(min, max))
        }
        let mut selection = Self::default();
        for item in scene.paint_list()? {
            if item.locked
                || !item.hit_enabled
                || item.object.hit_policy == HitPolicy::None
                || item.world_transform.inverse().is_none()
            {
                continue;
            }
            if item.object.hit_policy == HitPolicy::Painted
                && item
                    .object
                    .style
                    .fill
                    .as_ref()
                    .is_none_or(|p| !p.is_visible())
                && item
                    .object
                    .style
                    .stroke
                    .as_ref()
                    .is_none_or(|s| s.width <= 0. || !s.paint.is_visible())
            {
                continue;
            }
            let mut bounds = scene.world_bounds(&item.object.id)?;
            for clip in item.clips {
                bounds = bounds.and_then(|b| intersect(b, clip.world_bounds()));
            }
            if bounds.is_some_and(|b| b.intersects(rect)) {
                selection.ids.insert(item.object.id.clone());
            }
        }
        Ok(selection)
    }
}
