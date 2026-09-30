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
