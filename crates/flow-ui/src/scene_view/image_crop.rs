//! Source-frame inner editor. Preview is paint-only; history stores crop data.
use super::*;
use gpui_flow_engine::{ElementHandle, InstanceState};
use gpui_flow_interaction::{
    Capability, HitTarget, SelectionMode,
    scene::{
        IMAGE_CROP_PLUGIN, SceneCommand, SceneImageCropGeometry, SceneImageCropPlugin,
        SceneInteractionError,
    },
};
use gpui_flow_scene::{Shape, display::ImageFit};

pub(super) struct ImageCropMode {
    target: ElementHandle,
    document: u64,
    attachment: u64,
    source: String,
}
impl ImageCropMode {
    fn new(editor: &SceneEditor, target: ElementHandle) -> Option<Self> {
        let instance = editor.engine.instance(target).ok()?;
        if instance.state != InstanceState::Attached
            || !editor.scene().get(&instance.id)?.children.is_empty()
        {
            return None;
        }
        let Shape::Image(image) = &editor.engine.object(target).ok()?.shape else {
            return None;
        };
        Some(Self {
            target,
            document: editor.engine.document_epoch(),
            attachment: instance.attachment_epoch,
            source: image.source.clone(),
        })
    }
    fn valid(&self, editor: &SceneEditor) -> bool {
        self.document == editor.engine.document_epoch()
            && editor.engine.instance(self.target).is_ok_and(|i| {
                i.state == InstanceState::Attached && i.attachment_epoch == self.attachment
            })
            && editor
                .engine
                .object(self.target)
                .is_ok_and(|o| matches!(&o.shape,Shape::Image(i) if i.source==self.source))
            && SceneImageCropGeometry::for_selection(editor)
                .is_some_and(|g| g.target == self.target)
    }
}
impl SceneView {
    /// Open the built-in Image's fixed source-frame editor. The whole source is
    /// temporarily stretched into the unchanged box; handles delimit its crop.
    /// Closing restores canonical fit. Every completed drag is one Undo step.
    pub fn open_image_crop_editor(
        &mut self,
        target: ElementHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), SceneInteractionError> {
        if !self.interactions.allows(Capability::Edit) {
            return Err(SceneInteractionError::CapabilityDenied(Capability::Edit));
        }
        let mode = ImageCropMode::new(&self.editor, target).ok_or_else(|| {
            SceneInteractionError::InvalidInput("crop target is stale or not a leaf Image".into())
        })?;
        let id = self
            .editor
            .engine
            .instance(target)
            .map_err(|e| SceneInteractionError::InvalidInput(e.to_string()))?
            .id;
        if !gpui_flow_interaction::scene::object_editable(self.editor.scene(), &id)
            || self
                .editor
                .scene()
                .world_transform(&id)
                .ok()
                .and_then(gpui_flow_scene::Affine::inverse)
                .is_none()
        {
            return Err(SceneInteractionError::InvalidInput(
                "crop target is locked or non-invertible".into(),
            ));
        }
        self.finish_text_editor(window, cx);
        if self.text_editor_active() {
            return Err(SceneInteractionError::InvalidInput(
                "finish text composition before cropping".into(),
            ));
        }
        self.cancel_input(cx);
        self.execute_command(
            SceneCommand::Select {
                id,
                mode: SelectionMode::Replace,
            },
            cx,
        )?;
        // Selection middleware may reject, retarget or mutate the scene. Do
        // not report a successfully opened editor for an expired target.
        if !mode.valid(&self.editor) {
            return Err(SceneInteractionError::InvalidInput(
                "crop target or selection changed while opening".into(),
            ));
        }
        if !self.interactions.plugin_ids().contains(&IMAGE_CROP_PLUGIN) {
            self.interactions.install(SceneImageCropPlugin::default())?;
        }
        if !self.interactions.is_enabled(IMAGE_CROP_PLUGIN) {
            let report =
                self.interactions
                    .set_enabled(IMAGE_CROP_PLUGIN, true, &mut self.editor)?;
            self.apply_input_report(report, cx);
        }
        self.image_crop = Some(mode);
        window.focus(&self.focus, cx);
        cx.notify();
        Ok(())
    }
    /// Close the source preview. An unfinished pointer drag is cancelled;
    /// completed drags remain in ordinary document history.
    pub fn close_image_crop_editor(&mut self, cx: &mut Context<Self>) {
        if self.image_crop.take().is_some() {
            if self.interactions.capture_owner() == Some(IMAGE_CROP_PLUGIN) {
                let report = self.interactions.cancel(&mut self.editor);
                self.apply_input_report(report, cx);
            }
            cx.notify();
        }
    }
    pub fn image_crop_editor_active(&self) -> bool {
        self.image_crop_target().is_some()
    }
    pub fn image_crop_target(&self) -> Option<ElementHandle> {
        let mode = self.image_crop.as_ref()?;
        (self.interactions.allows(Capability::Edit)
            && self.interactions.is_enabled(IMAGE_CROP_PLUGIN)
            && !self.text_editor_active()
            && mode.valid(&self.editor))
        .then_some(mode.target)
    }
    pub(super) fn reconcile_image_crop_editor(&mut self, cx: &mut Context<Self>) {
        if self.image_crop.is_some() && self.image_crop_target().is_none() {
            self.close_image_crop_editor(cx);
        }
    }
    pub(super) fn image_crop_geometry(&self) -> Option<SceneImageCropGeometry> {
        let target = self.image_crop_target()?;
        SceneImageCropGeometry::for_selection(&self.editor).filter(|g| g.target == target)
    }
    pub(super) fn image_crop_hit(&self, point: WorldPoint) -> Option<HitTarget> {
        let geometry = self.image_crop_geometry()?;
        let id = self.editor.engine.instance(geometry.target).ok()?.id;
        Some(
            geometry
                .hit(&self.editor, point)
                .map_or(HitTarget::Control, |part| HitTarget::ImageCrop {
                    id: id.to_string(),
                    part,
                }),
        )
    }
    pub(super) fn image_crop_preview_id(&self) -> Option<DisplayId> {
        self.editor
            .engine
            .instance(self.image_crop_target()?)
            .ok()
            .map(|i| i.id)
    }
}
/// Full-source preview is applied to a temporary presentation scene only.
/// Export, resource identity, alpha hit policies, serialized data and Undo keep
/// using the canonical image. Native and CPU previews use the same scene.
pub(super) fn crop_preview(scene: &Scene, id: Option<&DisplayId>) -> Result<Scene, SceneError> {
    let mut scene = scene.clone();
    if let Some(id) = id {
        scene.update_object(id, |object| {
            if let Shape::Image(image) = &mut object.shape {
                image.source_crop = None;
                image.fit = ImageFit::Fill;
            }
        })?;
    }
    Ok(scene)
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use gpui_flow_scene::{Bounds as B, Point, Size, display::Image};
    fn editor() -> SceneEditor {
        let mut image = Image::new("source", Size::new(100., 80.));
        image.source_crop = Some(B::new(Point::new(0.25, 0.), Size::new(0.5, 1.)));
        image.fit = ImageFit::Cover;
        let mut scene = Scene::new();
        scene
            .insert(gpui_flow_scene::DisplayObject::new("i", image), None, None)
            .unwrap();
        let mut editor = SceneEditor::new(scene).unwrap();
        editor.selection.select("i".into(), false);
        editor
    }
    #[test]
    fn crop_preview_changes_only_temporary_image_fit_and_source_crop() {
        let editor = editor();
        let before = editor.scene().clone();
        let preview = crop_preview(editor.scene(), Some(&"i".into())).unwrap();
        assert_eq!(editor.scene(), &before);
        let Shape::Image(image) = &preview.get(&"i".into()).unwrap().object.shape else {
            panic!()
        };
        assert_eq!(image.fit, ImageFit::Fill);
        assert_eq!(image.source_crop, None);
        assert!(
            matches!(&before.get(&"i".into()).unwrap().object.shape,Shape::Image(i) if i.fit==ImageFit::Cover && i.source_crop.is_some())
        );
    }
    #[test]
    fn crop_mode_rejects_retyped_target_before_opening() {
        let mut editor = editor();
        let handle = editor.engine.handle(&"i".into()).unwrap();
        editor
            .transaction(|scene| {
                scene.update_object(&"i".into(), |object| {
                    object.shape = gpui_flow_scene::display::Rect::new(100., 80.).into();
                })
            })
            .unwrap();
        assert!(ImageCropMode::new(&editor, handle).is_none());
    }
    #[test]
    fn source_remount_document_and_selection_changes_expire_mode_identity() {
        for action in 0..4 {
            let mut editor = editor();
            let h = editor.engine.handle(&"i".into()).unwrap();
            let mode = ImageCropMode::new(&editor, h).unwrap();
            assert!(mode.valid(&editor));
            match action {
                0 => editor
                    .transaction(|s| {
                        s.update_object(&"i".into(), |o| {
                            if let Shape::Image(i) = &mut o.shape {
                                i.source = "new".into()
                            }
                        })
                    })
                    .unwrap(),
                1 => {
                    let (d, _) = editor.engine.detach(h).unwrap();
                    editor.engine.attach(&d, None, None).unwrap();
                }
                2 => {
                    editor.load_scene(editor.scene().clone()).unwrap();
                }
                _ => editor.selection.clear(),
            }
            assert!(!mode.valid(&editor));
        }
    }
}
