use super::*;
use gpui_flow_editor::BuiltinResize;

impl ScenePluginHost {
    pub(super) fn apply_builtin_resize(
        &mut self,
        editor: &mut SceneEditor,
        target: ElementHandle,
        resize: BuiltinResize,
    ) -> Result<(), InteractionError> {
        if self.capture_target_stale(editor) {
            return Err(InteractionError::InvalidInput(
                "stale intrinsic resize target".into(),
            ));
        }
        let instance = editor
            .engine
            .instance(target)
            .map_err(|e| InteractionError::InvalidInput(e.to_string()))?;
        if instance.state != InstanceState::Attached {
            return Err(InteractionError::InvalidInput(
                "detached intrinsic resize target".into(),
            ));
        }
        if let Some(guard) = self
            .capture
            .as_ref()
            .and_then(|c| c.selection_guard.as_ref())
            && guard.targets.as_slice() != [(target, instance.attachment_epoch)]
        {
            return Err(InteractionError::InvalidInput(
                "intrinsic resize capture membership changed".into(),
            ));
        }
        let id = instance.id.clone();
        let node = editor
            .scene()
            .get(&id)
            .ok_or_else(|| SceneError::MissingNode(id.clone()))?;
        let original = node.object.transform;
        let transform = resize.transform;
        if !object_editable(editor.scene(), &id)
            || [original.a, original.b, original.c, original.d]
                != [transform.a, transform.b, transform.c, transform.d]
            || transform.inverse().is_none()
        {
            return Err(InteractionError::InvalidInput(
                "intrinsic resize cannot change the linear pose".into(),
            ));
        }
        let expected = resize
            .apply_to(&node.object)
            .map_err(InteractionError::InvalidInput)?;
        let children = resize.child_transforms();
        let group = matches!(node.object.shape, gpui_flow_scene::Shape::Group(_));
        if group && !children.iter().map(|(id, _)| id).eq(node.children.iter()) {
            return Err(InteractionError::InvalidInput(
                "intrinsic content membership changed".into(),
            ));
        }
        let parent = match &node.parent {
            Some(parent) => editor.scene().world_transform(parent)?,
            None => Affine::IDENTITY,
        };
        let expected_world = parent * transform;
        let capabilities = self.transforms;
        // Validate the derived candidate before the single engine/history commit.
        // Box/Frame children may reflow; the requested target geometry and pose
        // cannot be silently replaced by an auto-size/parent-allocation owner.
        editor.transaction_validated(
            |scene| {
                scene.update_object(&id, |object| *object = expected.clone())?;
                for (child, matrix) in children {
                    if !object_editable(scene, child) {
                        return Err(SceneError::InvalidGeometry(
                            "intrinsic content is locked".into(),
                        ));
                    }
                    scene.update_object(child, |object| object.transform = *matrix)?;
                }
                Ok(())
            },
            |scene| {
                let node = scene
                    .get(&id)
                    .ok_or_else(|| SceneError::MissingNode(id.clone()))?;
                if node.object != expected
                    || (group && !children.iter().map(|(id, _)| id).eq(node.children.iter()))
                    || scene.world_transform(&id)? != expected_world
                    || !object_editable(scene, &id)
                    || !super::super::SceneResizeGeometry::accepts_scene_transform(
                        scene,
                        &id,
                        transform,
                        capabilities,
                    )
                    || children.iter().any(|(child, matrix)| {
                        scene.get(child).is_none_or(|node| {
                            node.parent.as_ref() != Some(&id) || node.object.transform != *matrix
                        })
                    })
                {
                    return Err(SceneError::InvalidGeometry(
                        "derived layout replaced intrinsic geometry or pose".into(),
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
