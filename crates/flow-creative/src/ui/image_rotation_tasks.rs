use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

impl CreativeCanvas {
    pub(super) fn apply_image_rotation(&mut self, cx: &mut Context<Self>) {
        if self.importing || self.saving || !self.canvas.read(cx).is_editable() {
            return;
        }
        let Some(edit) = &self.image_rotation else {
            return;
        };
        let target = edit.node.clone();
        let source = edit.asset.clone();
        let path = edit.path.clone();
        let spec = edit.spec;
        let expected = edit.original_data.clone();
        let result = (|| {
            let dimensions = spec
                .output_dimensions(source.width, source.height)
                .map_err(|e| e.to_string())?;
            let mut project = self
                .project
                .clone()
                .ok_or(tr("请先打开本地项目", "Open a local project first"))?;
            let project_id = project.metadata().id.clone();
            let document = self.canvas.read(cx).editor.document();
            let registry = self.registry.clone();
            let node = document
                .graph
                .node(&target)
                .filter(|n| {
                    n.data == expected
                        && !crate::layer_guard::node_is_locked(&document.graph, &n.id)
                })
                .ok_or(tr(
                    "图片已改变，请重新打开旋转",
                    "The image changed. Reopen Rotation.",
                ))?;
            let bounds = rotated_bounds(node.bounds(), (source.width, source.height), dimensions)?;
            let size = bounds.size;
            let position = bounds.origin;
            let cancel = Arc::new(AtomicBool::new(false));
            self.image_edit_cancel = Some(cancel.clone());
            self.importing = true;
            self.canvas.update(cx, |v, cx| v.set_editable(false, cx));
            cx.notify();
            cx.spawn(async move |this, cx| {
                let task_cancel = cancel.clone();
                let result = cx
                    .background_executor()
                    .spawn(async move {
                        project
                            .save_document(&document, &registry)
                            .map_err(|e| e.to_string())?;
                        let result = (|| {
                            let scratch = super::processing_tasks::ProcessingScratch::new("png")?;
                            crate::image_rotation::write_rotated(
                                &path,
                                &source,
                                &spec,
                                &scratch.output,
                                &task_cancel,
                            )
                            .map_err(|e| e.to_string())?;
                            if task_cancel.load(Ordering::Acquire) {
                                return Err(tr("旋转已取消", "Rotation canceled").into());
                            }
                            project
                                .import_image(&scratch.output)
                                .map_err(|e| e.to_string())
                        })();
                        Ok::<_, String>((project, result))
                    })
                    .await;
                let _ = this.update(cx, |this, cx| {
                    this.image_edit_cancel = None;
                    this.importing = false;
                    this.canvas.update(cx, |v, cx| v.set_editable(true, cx));
                    match result {
                        Ok((project, asset)) => {
                            let same_project = this
                                .project
                                .as_ref()
                                .is_some_and(|p| p.metadata().id == project_id);
                            let graph = this.canvas.read(cx).editor.graph();
                            let same_node = graph.node(&target).is_some_and(|n| {
                                n.data == expected
                                    && !crate::layer_guard::node_is_locked(graph, &n.id)
                            });
                            if same_project {
                                this.last_saved_document = Some(project.document().clone());
                                this.project = Some(project);
                                this.refresh_assets(cx);
                                match asset {
                                    Ok(asset) if same_node && !cancel.load(Ordering::Acquire) => {
                                        this.clear_image_rotation();
                                        if this.apply_image_version_result(
                                            &target,
                                            asset.key,
                                            position,
                                            size,
                                            crate::image_versions::ImageVersionOperation::Rotate,
                                            cx,
                                        ) {
                                            this.notice =
                                                Some(tr("旋转已应用；可从版本面板恢复原图", "Rotation applied. Restore the original from Version History.").into());
                                        }
                                    }
                                    Ok(_) => {
                                        this.clear_image_rotation();
                                        this.notice =
                                            Some(tr("图片已改变或处理已取消，未覆盖当前对象", "The image changed or processing was canceled. The current object was preserved.").into());
                                    }
                                    Err(error) => {
                                        this.notice = Some(error);
                                        this.pending_window_close = None;
                                    }
                                }
                            }
                        }
                        Err(error) => {
                            this.notice = Some(error);
                            this.pending_window_close = None;
                        }
                    }
                    this.queue_autosave(cx);
                    if this.return_after_save || this.pending_window_close.is_some() {
                        this.save_project(cx);
                    }
                    cx.notify();
                });
            })
            .detach();
            Ok::<_, String>(())
        })();
        if let Err(error) = result {
            self.notice = Some(error);
            cx.notify();
        }
    }
}

fn rotated_bounds(
    bounds: gpui_flow_core::Rect,
    source: (u32, u32),
    output: (u32, u32),
) -> Result<gpui_flow_core::Rect, String> {
    if source.0 == 0 || source.1 == 0 {
        return Err(tr("源图尺寸无效", "Invalid source image dimensions").into());
    }
    let scale = (bounds.size.width / source.0 as f32).min(bounds.size.height / source.1 as f32);
    let size = WorldSize::new(output.0 as f32 * scale, output.1 as f32 * scale);
    let center = bounds.center();
    let result = gpui_flow_core::Rect::new(
        center - WorldPoint::new(size.width / 2., size.height / 2.),
        size,
    );
    if !size.is_positive() || !result.is_valid() {
        return Err(tr("旋转后的画布尺寸无效", "Invalid rotated canvas dimensions").into());
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[::core::prelude::v1::test]
    fn rotated_canvas_bounds_preserve_pixel_scale_and_center() {
        let before =
            gpui_flow_core::Rect::new(WorldPoint::new(80., 100.), WorldSize::new(420., 262.5));
        let after = rotated_bounds(before, (640, 400), (736, 736)).unwrap();
        assert_eq!(after.center(), before.center());
        assert_eq!(after.size, WorldSize::new(483., 483.));
        assert!(
            rotated_bounds(
                gpui_flow_core::Rect::new(WorldPoint::ZERO, WorldSize::new(f32::MAX, f32::MAX)),
                (1, 1),
                (2, 2)
            )
            .is_err()
        );
    }
}
