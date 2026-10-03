use gpui_flow_scene::{DisplayId, Scene, SceneError};
pub(crate) fn tree_ids(scene: &Scene) -> Vec<DisplayId> {
    let mut out = vec![];
    let mut stack: Vec<_> = scene.roots().iter().rev().cloned().collect();
    while let Some(id) = stack.pop() {
        if let Some(n) = scene.get(&id) {
            stack.extend(n.children.iter().rev().cloned());
            out.push(id);
        }
    }
    out
}
pub(crate) fn paint_ids(scene: &Scene) -> Vec<DisplayId> {
    fn ordered(scene: &Scene, ids: &[DisplayId]) -> Vec<DisplayId> {
        let mut ids = ids.to_vec();
        ids.sort_by_key(|id| scene.get(id).expect("node").object.z_index);
        ids
    }
    let mut out = vec![];
    let mut stack: Vec<_> = ordered(scene, scene.roots()).into_iter().rev().collect();
    while let Some(id) = stack.pop() {
        let node = scene.get(&id).expect("node");
        stack.extend(ordered(scene, &node.children).into_iter().rev());
        out.push(id);
    }
    out
}
pub(crate) fn extract_subtree(scene: &Scene, root: &DisplayId) -> Result<Scene, SceneError> {
    scene.transfer_subtree(root)
}
pub(crate) fn insert_forest(
    scene: &mut Scene,
    forest: &Scene,
    parent: Option<DisplayId>,
    index: Option<usize>,
) -> Result<(), SceneError> {
    for id in tree_ids(forest) {
        let n = forest.get(&id).expect("forest");
        let is_root = n.parent.is_none();
        let root_index = forest.roots().iter().position(|r| r == &id);
        scene.insert(
            n.object.clone(),
            if is_root {
                parent.clone()
            } else {
                n.parent.clone()
            },
            if is_root {
                index.map(|i| i + root_index.unwrap_or(0))
            } else {
                None
            },
        )?;
    }
    Ok(())
}
