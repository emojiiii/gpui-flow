use super::super::{GeometryEdit, SceneGeometryTools, SceneResizeGeometry, SceneRotationGeometry};
use super::*;
use gpui_flow_editor::{PathControlGeometry, apply_skew, bounded_point};

impl ScenePluginHost {
    pub(super) fn apply_geometry(
        &mut self,
        editor: &mut SceneEditor,
        target: ElementHandle,
        edit: GeometryEdit,
    ) -> Result<(), InteractionError> {
        let invalid = |text: &str| InteractionError::InvalidInput(text.into());
        if self.capture_target_stale(editor) {
            return Err(invalid("stale geometry capture"));
        }
        let geometry = SceneGeometryTools::for_selection(editor, edit.mode(), self.transforms)
            .filter(|g| g.target == target)
            .ok_or_else(|| invalid("unsupported geometry target"))?;
        let instance = editor
            .engine
            .instance(target)
            .map_err(|e| invalid(&e.to_string()))?;
        let guard = self
            .capture
            .as_ref()
            .and_then(|c| c.selection_guard.as_ref());
        if guard.is_some_and(|g| g.targets.as_slice() != [(target, instance.attachment_epoch)]) {
            return Err(invalid("geometry capture membership changed"));
        }
        let id = instance.id;
        let before = editor
            .scene()
            .get(&id)
            .ok_or_else(|| invalid("missing geometry target"))?;
        let mut expected = before.clone();
        match edit {
            GeometryEdit::PathPoint { point, position } => {
                PathControlGeometry::set_point(&mut expected.object.shape, point, position)
                    .map_err(InteractionError::InvalidInput)?;
            }
            GeometryEdit::Pivot { position } => {
                if position.is_some_and(|p| !bounded_point(p)) {
                    return Err(invalid("pivot exceeds the finite coordinate budget"));
                }
                expected.object.pivot = position;
            }
            GeometryEdit::Skew { axis, shear } => {
                let (original, pivot) = guard
                    .and_then(|g| g.geometry_baseline)
                    .unwrap_or((geometry.original, geometry.pivot));
                expected.object.transform = apply_skew(original, pivot, shear, axis)
                    .ok_or_else(|| invalid("skew exceeds the invertible shear budget"))?;
                if !SceneResizeGeometry::accepts_scene_transform(
                    editor.scene(),
                    &id,
                    expected.object.transform,
                    self.transforms,
                ) {
                    return Err(invalid("unsupported skew presentation"));
                }
            }
        }
        // A derived layout cannot quietly change the target's other points,
        // metadata, anchor, hierarchy or any geometry inside a skewed group.
        let mut descendants = Vec::new();
        let mut pending = before.children.clone();
        while let Some(id) = pending.pop() {
            let node = editor
                .scene()
                .get(&id)
                .ok_or_else(|| invalid("missing geometry descendant"))?;
            pending.extend(node.children.iter().cloned());
            descendants.push(node.clone());
        }
        let parent_world = before
            .parent
            .as_ref()
            .map(|p| editor.scene().world_transform(p))
            .transpose()?
            .unwrap_or(Affine::IDENTITY);
        let expected_world = parent_world * expected.object.transform;
        let capabilities = self.transforms;
        editor.transaction_validated(
            |scene| scene.update_object(&id, |object| *object = expected.object.clone()),
            |scene| {
                if !object_editable(scene, &id)
                    || scene.get(&id) != Some(&expected)
                    || scene.world_transform(&id)? != expected_world
                    || descendants
                        .iter()
                        .any(|node| scene.get(&node.object.id) != Some(node))
                    || (matches!(edit, GeometryEdit::Skew { .. })
                        && (!SceneRotationGeometry::accepts_subtree(scene, &id, capabilities)
                            || !SceneResizeGeometry::accepts_scene_transform(
                                scene,
                                &id,
                                expected.object.transform,
                                capabilities,
                            )))
                {
                    return Err(SceneError::InvalidGeometry(
                        "derived layout changed the geometry control contract".into(),
                    ));
                }
                Ok(())
            },
        )?;
        if let Some(guard) = self
            .capture
            .as_mut()
            .and_then(|c| c.selection_guard.as_mut())
        {
            guard.accepted_revision = editor.engine.revision();
        }
        Ok(())
    }
}
