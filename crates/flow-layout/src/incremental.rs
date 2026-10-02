//! Conservative dependency selection, with no mutable candidate/cache state.
use crate::{
    AlignItems, FlowError, FlowLayout, FlowReport, ItemBox, MAX_FLOW_LAYOUT_PASSES,
    MeasuredFlowLayout,
};
use gpui_flow_scene::{DisplayId, Scene};
use std::collections::{BTreeSet, VecDeque};

#[derive(Default)]
pub(crate) struct Selection {
    pub containers: BTreeSet<DisplayId>,
    pub texts: BTreeSet<DisplayId>,
}
impl FlowLayout {
    pub(crate) fn dirty_selection(&self, old: &Scene, next: &Scene) -> Selection {
        let mut selection = Selection::default();
        // Only geometric and declared stroke dependencies enter the graph.
        // Metadata, fills, opacity and visibility do not consume layout work.
        for (id, style) in &self.containers {
            let Some(node) = next.get(id) else {
                continue;
            };
            let changed = old.get(id).is_none_or(|before| {
                before.children != node.children
                    || before.object.shape != node.object.shape
                    || before.parent != node.parent
            }) || node.children.iter().any(|child| {
                if self.items.get(child).is_some_and(|item| !item.in_flow) {
                    return false;
                }
                let Some(after) = next.get(child) else {
                    return true;
                };
                old.get(child).is_none_or(|before| {
                    before.object.shape != after.object.shape
                        || before.object.transform != after.object.transform
                        || before.parent != after.parent
                        || (style.item_box == ItemBox::Stroke
                            && (before.object.style.stroke != after.object.style.stroke
                                || before.object.style.stroke_layers
                                    != after.object.style.stroke_layers
                                || before.object.style.fill_rule != after.object.style.fill_rule))
                })
            });
            if changed {
                selection.containers.insert(id.clone());
            }
        }
        for id in self.texts.keys() {
            if next.get(id).is_some_and(|node| {
                old.get(id)
                    .is_none_or(|before| before.object.shape != node.object.shape)
            }) {
                selection.texts.insert(id.clone());
                if let Some(parent) = next.get(id).and_then(|n| n.parent.as_ref())
                    && self.containers.contains_key(parent)
                    && self.items.get(id).is_none_or(|i| i.in_flow)
                {
                    selection.containers.insert(parent.clone());
                }
            }
        }
        // Content dimensions travel upwards; allocations travel downwards.
        // Translation-only movement of a fixed child does not dirty its contents.
        // Every successful insertion is a previously unseen configured node, so
        // this closure is bounded by the finite layout graph, never an observer.
        let mut pending: VecDeque<_> = selection.containers.iter().cloned().collect();
        while let Some(id) = pending.pop_front() {
            let Some(node) = next.get(&id) else {
                continue;
            };
            let style = &self.containers[&id];
            if (style.width_sizing.is_content() || style.height_sizing.is_content())
                && let Some(parent) = &node.parent
                && self.containers.contains_key(parent)
                && self.items.get(&id).is_none_or(|i| i.in_flow)
                && selection.containers.insert(parent.clone())
            {
                pending.push_back(parent.clone());
            }
            for child in &node.children {
                let item = self.items.get(child).cloned().unwrap_or_default();
                if !item.in_flow {
                    continue;
                }
                if self.texts.contains_key(child) {
                    selection.texts.insert(child.clone());
                }
                let allocated = item.grow > 0.0
                    || item.basis.is_some()
                    || item.width_percent.is_some()
                    || item.height_percent.is_some()
                    || item.min_width.is_some()
                    || item.max_width.is_some()
                    || item.min_height.is_some()
                    || item.max_height.is_some()
                    || item.align_self.unwrap_or(style.align_items) == AlignItems::Stretch;
                if allocated
                    && self.containers.contains_key(child)
                    && selection.containers.insert(child.clone())
                {
                    pending.push_back(child.clone());
                }
            }
        }
        selection
    }

    /// Reflow only the affected dependency closure. `previous` must already be a
    /// fixed point of this exact layout and backend generation. Use arranged()
    /// for initial layout, changed configuration, or global font invalidation.
    /// Validation still covers the full candidate; this reduces solver work, not
    /// the cost of cloning, validating or scanning the entire document.
    pub fn arranged_dirty(
        &self,
        previous: &Scene,
        candidate: &Scene,
    ) -> Result<(Scene, FlowReport), FlowError> {
        let selected = self.dirty_selection(previous, candidate);
        self.arranged_selected(candidate, MAX_FLOW_LAYOUT_PASSES, None, Some(&selected))
    }
}
impl MeasuredFlowLayout {
    /// Measured equivalent of FlowLayout::arranged_dirty with the same stable
    /// configuration/backend-generation precondition.
    pub fn arranged_dirty(
        &self,
        previous: &Scene,
        candidate: &Scene,
    ) -> Result<(Scene, FlowReport), FlowError> {
        let selected = self.layout.dirty_selection(previous, candidate);
        self.layout.arranged_selected(
            candidate,
            MAX_FLOW_LAYOUT_PASSES,
            Some(self.measurer.as_ref()),
            Some(&selected),
        )
    }
}
