use super::*;
use gpui_flow_media::VideoFrame;
use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};

impl CreativeCanvas {
    pub(super) fn capture_video_frame(&mut self, id: &NodeId, cx: &mut Context<Self>) {
        if self.saving || self.importing || !self.canvas.read(cx).is_editable() {
            return;
        }
        let Some(mut project) = self.project.clone() else {
            return;
        };
        let Some(node) = self.selected(cx).filter(|node| &node.id == id) else {
            return;
        };
        let Some(frame) = self.media_views.displayed_video_frame(&node, cx) else {
            return;
        };
        let Some(source) = node.data["asset"].as_str().map(str::to_owned) else {
            return;
        };
        let graph = self.canvas.read(cx).editor.graph();
        if node.kind == "process_video"
            && crate::processing_node::processing_output(graph, &node).is_err()
        {
            return;
        }
        let project_directory = project.directory().to_owned();
        let project_revision = project.revision();
        let source_node = node.id;
        let source_kind = node.kind;
        self.importing = true;
        self.canvas
            .update(cx, |view, cx| view.set_editable(false, cx));
        cx.notify();
        cx.spawn(async move |this, cx| {
            let source_key = source.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    project
                        .resolve_asset_path(&source_key)
                        .map_err(|error| error.to_string())?;
                    let scratch = write_preview_png(frame)?;
                    let output = scratch.output.canonicalize().map_err(|error| error.to_string())?;
                    let asset = project
                        .import_image(output)
                        .map_err(|error| error.to_string())?;
                    Ok::<_, String>((project, asset))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.importing = false;
                this.canvas.update(cx, |view, cx| view.set_editable(true, cx));
                let same_project = this.project.as_ref().is_some_and(|project| {
                    project.directory() == project_directory && project.revision() == project_revision
                });
                if same_project {
                    match result {
                        Ok((project, asset)) => {
                            this.last_saved_document = Some(project.document().clone());
                            this.project = Some(project);
                            this.refresh_assets(cx);
                            let graph = this.canvas.read(cx).editor.graph();
                            let source_is_current = graph.node(&source_node).is_some_and(|node| {
                                node.kind == source_kind
                                    && node.data["asset"].as_str() == Some(source.as_str())
                                    && (node.kind != "process_video"
                                        || crate::processing_node::processing_output(graph, node).is_ok())
                            });
                            if source_is_current {
                                this.place_project_asset(&asset.key, cx);
                            } else {
                                this.notice = Some(
                                    tr(
                                        "源视频已改变，截帧可在项目素材中找到",
                                        "The source video changed. The frame is available in Project Assets.",
                                    )
                                    .into(),
                                );
                            }
                        }
                        Err(error) => this.notice = Some(tr_format!(
                            "无法保存视频帧：{error}",
                            "Could not save the video frame: {error}"
                        )),
                    }
                }
                if this.return_after_save || this.pending_window_close.is_some() {
                    this.save_project(cx);
                } else {
                    this.queue_autosave(cx);
                }
                cx.notify();
            });
        }).detach();
    }
}

fn write_preview_png(frame: VideoFrame) -> Result<processing_tasks::ProcessingScratch, String> {
    let mut rgba = frame.bgra.to_vec();
    for pixel in rgba.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    let mut scratch = processing_tasks::ProcessingScratch::new("png")?;
    let milliseconds = (frame.position * 1000.).round() as u64;
    scratch.output.set_file_name(tr_format!(
        "视频截帧-{milliseconds}ms.png",
        "video-frame-{milliseconds}ms.png"
    ));
    let file = std::fs::File::options()
        .write(true)
        .create_new(true)
        .open(&scratch.output)
        .map_err(|error| error.to_string())?;
    PngEncoder::new(file)
        .write_image(&rgba, frame.width, frame.height, ExtendedColorType::Rgba8)
        .map_err(|error| error.to_string())?;
    Ok(scratch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;

    #[test]
    fn captured_preview_png_preserves_pixel_order_and_colors() {
        let png = write_preview_png(VideoFrame {
            width: 2,
            height: 1,
            bgra: Arc::from([0, 0, 255, 255, 255, 0, 0, 255]),
            position: 1.5,
            sequence: 1,
        })
        .unwrap();
        let pixels = image::open(&png.output).unwrap().into_rgba8();
        assert_eq!(pixels.dimensions(), (2, 1));
        assert_eq!(pixels.into_raw(), [255, 0, 0, 255, 0, 0, 255, 255]);
    }
}
