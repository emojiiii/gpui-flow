//! A typed box/translation transaction, never a cached whole Text replacement.
use super::*;
use gpui_flow_core::Size;
use gpui_flow_scene::Shape;
impl ScenePluginHost {
    pub(super) fn apply_text_box_resize(
        &mut self,
        editor: &mut SceneEditor,
        target: ElementHandle,
        size: Size,
        transform: Affine,
    ) -> Result<(), InteractionError> {
        self.apply_text_intrinsic_resize(editor, target, size, transform, None)
    }
    pub(super) fn apply_text_font_resize(
        &mut self,
        editor: &mut SceneEditor,
        target: ElementHandle,
        resize: gpui_flow_editor::TextFontResize,
    ) -> Result<(), InteractionError> {
        if !resize.font_size.is_finite()
            || resize.font_size <= 0.
            || resize.font_size > gpui_flow_editor::MAX_TEXT_RESIZE_FONT_SIZE
        {
            return Err(InteractionError::InvalidInput(
                "unsupported Text font resize".into(),
            ));
        }
        self.apply_text_intrinsic_resize(
            editor,
            target,
            resize.size,
            resize.transform,
            Some((resize.font_size, resize.padding)),
        )
    }
    fn apply_text_intrinsic_resize(
        &mut self,
        editor: &mut SceneEditor,
        target: ElementHandle,
        size: Size,
        transform: Affine,
        font: Option<(f32, [f32; 4])>,
    ) -> Result<(), InteractionError> {
        if self.capture_target_stale(editor) {
            return Err(InteractionError::InvalidInput(
                "stale Text box target".into(),
            ));
        }
        let instance = editor
            .engine
            .instance(target)
            .map_err(|e| InteractionError::InvalidInput(e.to_string()))?;
        if instance.state != InstanceState::Attached {
            return Err(InteractionError::InvalidInput(
                "detached Text box target".into(),
            ));
        }
        if let Some(guard) = self
            .capture
            .as_ref()
            .and_then(|c| c.selection_guard.as_ref())
            && guard.targets.as_slice() != [(target, instance.attachment_epoch)]
        {
            return Err(InteractionError::InvalidInput(
                "Text box capture membership changed".into(),
            ));
        }
        let id = instance.id.clone();
        let node = editor
            .scene()
            .get(&id)
            .ok_or_else(|| SceneError::MissingNode(id.clone()))?;
        let original = node.object.transform;
        let plain = TransformCapabilities { affine_text: true };
        if !object_editable(editor.scene(), &id)
            || !super::super::resize_handles::affine_text_supported(&node.object, plain)
            || [original.a, original.b, original.c, original.d]
                != [transform.a, transform.b, transform.c, transform.d]
            || !size.is_positive()
            || size.width > gpui_flow_editor::MAX_TEXT_BOX_DIMENSION
            || size.height > gpui_flow_editor::MAX_TEXT_BOX_DIMENSION
            || !super::super::SceneResizeGeometry::accepts_transform_with_capabilities(
                editor,
                &id,
                transform,
                self.transforms,
            )
        {
            return Err(InteractionError::InvalidInput(
                "unsupported Text box resize".into(),
            ));
        }
        let parent = match &node.parent {
            Some(id) => editor.scene().world_transform(id)?,
            None => Affine::IDENTITY,
        };
        let expected_world = parent * transform;
        let capabilities = self.transforms;
        editor.transaction_validated(
            |scene| {
                scene.update_object(&id, |object| {
                    if let Shape::Text(text) = &mut object.shape {
                        text.size = size;
                        if let Some((font_size,padding))=font {
                            text.font_size=font_size;
                            text.padding=padding;
                        }
                    }
                    object.transform = transform;
                })
            },
            |scene| {
                let node = scene
                    .get(&id)
                    .ok_or_else(|| SceneError::MissingNode(id.clone()))?;
                if !object_editable(scene, &id)
                    || !matches!(&node.object.shape,Shape::Text(t) if t.size==size && font.is_none_or(|(font_size,padding)|t.font_size==font_size&&t.padding==padding))
                    || node.object.transform != transform
                    || scene.world_transform(&id)? != expected_world
                    || !super::super::resize_handles::affine_text_supported(&node.object, plain)
                    || !super::super::SceneResizeGeometry::accepts_scene_transform(
                        scene,
                        &id,
                        transform,
                        capabilities,
                    )
                {
                    return Err(SceneError::InvalidGeometry(
                        "derived layout replaced the fixed Text box or pose".into(),
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
