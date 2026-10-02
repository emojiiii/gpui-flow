use super::*;
use gpui_flow_scene::{Bounds, Shape, display::Image};
impl ScenePluginHost {
    pub(super) fn apply_image_crop(
        &mut self,
        editor: &mut SceneEditor,
        target: ElementHandle,
        crop: Option<Bounds>,
    ) -> Result<(), InteractionError> {
        if self.capture_target_stale(editor) {
            return Err(InteractionError::InvalidInput(
                "stale image crop capture".into(),
            ));
        }
        let geometry = super::super::SceneImageCropGeometry::for_selection(editor)
            .filter(|g| g.target == target)
            .ok_or_else(|| {
                InteractionError::InvalidInput("unsupported image crop target".into())
            })?;
        let instance = editor
            .engine
            .instance(target)
            .map_err(|e| InteractionError::InvalidInput(e.to_string()))?;
        if self
            .capture
            .as_ref()
            .and_then(|c| c.selection_guard.as_ref())
            .is_some_and(|g| g.targets.as_slice() != [(target, instance.attachment_epoch)])
        {
            return Err(InteractionError::InvalidInput(
                "image crop capture membership changed".into(),
            ));
        }
        if let Some(crop) = crop {
            Image::validate_source_crop(crop).map_err(InteractionError::InvalidInput)?;
        }
        let id = instance.id;
        let mut expected = editor
            .engine
            .object(target)
            .map_err(|e| InteractionError::InvalidInput(e.to_string()))?
            .clone();
        let Shape::Image(image) = &mut expected.shape else {
            unreachable!()
        };
        image.source_crop = crop;
        image.validate().map_err(InteractionError::InvalidInput)?;
        editor.transaction_validated(
            |scene| {
                scene.update_object(&id, |object| {
                    if let Shape::Image(image) = &mut object.shape {
                        image.source_crop = crop;
                    }
                })
            },
            |scene| {
                if !object_editable(scene, &id)
                    || scene
                        .get(&id)
                        .is_none_or(|node| node.object != expected || !node.children.is_empty())
                    || scene.world_transform(&id)? != geometry.world
                {
                    return Err(SceneError::InvalidGeometry(
                        "derived layout changed the image crop contract".into(),
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
