//! Explicit clipboard gestures. Never interpret copied text as a path or URL.
use super::*;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub(super) struct OwnedCanvasCopy {
    token: String,
    origin: WorldPoint,
}

#[derive(Debug)]
enum PasteContent {
    LocalObjects,
    Image(gpui::Image),
}

fn classify_paste(item: ClipboardItem, token: Option<&str>) -> Result<PasteContent, String> {
    if token.is_some_and(|token| item.metadata().is_some_and(|metadata| metadata == token)) {
        return Ok(PasteContent::LocalObjects);
    }
    let mut image = None;
    for entry in item.into_entries() {
        if let ClipboardEntry::Image(candidate) = entry {
            if image.is_some() {
                return Err(tr(
                    "请一次复制一张 PNG 或 JPEG 图片",
                    "Copy one PNG or JPEG image at a time",
                )
                .into());
            }
            if !matches!(
                candidate.format,
                gpui::ImageFormat::Png | gpui::ImageFormat::Jpeg
            ) {
                return Err(tr(
                    "剪贴板图片目前支持静态 PNG/JPEG，请用导入入口选择其它格式",
                    "Paste a static PNG or JPEG. For other formats, use Import.",
                )
                .into());
            }
            if candidate.bytes.is_empty() || candidate.bytes.len() as u64 > MAX_IMAGE_BYTES {
                return Err(tr(
                    "剪贴板图片不能为空且最多 32 MiB",
                    "Clipboard images must be nonempty and at most 32 MiB",
                )
                .into());
            }
            image = Some(candidate);
        }
    }
    image.map(PasteContent::Image).ok_or_else(|| {
        tr("剪贴板没有 PNG/JPEG 图片或本画布复制的对象；文字请粘贴到文字编辑框，文件请使用导入", "Copy a PNG or JPEG or an object on this canvas. Paste text into the text editor or use Import for files.").into()
    })
}

impl CreativeCanvas {
    pub(super) fn cancel_clipboard_paste(&mut self) {
        if let Some(cancel) = &self.clipboard_cancel {
            cancel.store(true, Ordering::Release);
        }
    }

    pub(super) fn copy_canvas_selection(&mut self, cx: &mut Context<Self>) {
        if self.importing || self.saving {
            return;
        }
        self.apply_form_edit(cx);
        let copied = self.canvas.read(cx).editor.copy_selection();
        let Some(bounds) = copied
            .nodes
            .iter()
            .map(Node::bounds)
            .chain(copied.groups.iter().map(|group| group.bounds))
            .reduce(gpui_flow_core::Rect::union)
        else {
            return;
        };
        let label = if copied.nodes.len() == 1
            && copied.groups.is_empty()
            && copied.nodes[0].kind == "text"
        {
            copied.nodes[0].data["text"]
                .as_str()
                .unwrap_or(tr("文字", "Text"))
                .to_owned()
        } else {
            copied
                .nodes
                .iter()
                .map(|node| node.title.as_str())
                .chain(copied.groups.iter().map(|group| group.title.as_str()))
                .collect::<Vec<_>>()
                .join("\n")
        };
        if !self.command(CanvasCommand::CopySelection, cx) {
            return;
        }
        static NEXT_COPY: AtomicU64 = AtomicU64::new(1);
        let token = format!(
            "flow-creative-local-copy:{}:{}",
            std::process::id(),
            NEXT_COPY.fetch_add(1, Ordering::Relaxed)
        );
        cx.write_to_clipboard(ClipboardItem::new_string_with_metadata(
            label,
            token.clone(),
        ));
        self.owned_canvas_copy = Some(OwnedCanvasCopy {
            token,
            origin: bounds.origin,
        });
        self.notice = Some(
            tr(
                "已复制对象，可在本画布粘贴",
                "Copied objects to paste on this canvas",
            )
            .into(),
        );
        cx.notify();
    }

    pub(super) fn clipboard_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if event.keystroke.key == "escape" && self.clipboard_cancel.is_some() {
            self.cancel_clipboard_paste();
            cx.stop_propagation();
            window.prevent_default();
            return true;
        }
        let modifiers = event.keystroke.modifiers;
        if !self.canvas.read(cx).focus_handle(cx).is_focused(window)
            || !(modifiers.control || modifiers.platform)
            || modifiers.alt
            || modifiers.shift
            || !matches!(event.keystroke.key.as_str(), "c" | "v")
        {
            return false;
        }
        cx.stop_propagation();
        window.prevent_default();
        if event.is_held {
            return true;
        }
        if event.keystroke.key == "c" {
            self.copy_canvas_selection(cx);
        } else {
            self.paste_clipboard(None, cx);
        }
        true
    }

    pub(super) fn paste_clipboard(&mut self, at: Option<WorldPoint>, cx: &mut Context<Self>) {
        self.apply_form_edit(cx);
        if self.importing || self.saving || !self.canvas.read(cx).is_editable() {
            return;
        }
        if at.is_some_and(|point| !point.is_finite()) {
            self.notice = Some(tr("粘贴位置无效", "Invalid paste position").into());
            cx.notify();
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        self.clipboard_cancel = Some(cancel.clone());
        self.notice = Some(tr("正在粘贴…按 Esc 可取消", "Pasting… Press Esc to cancel").into());
        self.importing = true;
        self.canvas
            .update(cx, |canvas, cx| canvas.set_editable(false, cx));
        let request = cx.read_from_clipboard_async();
        let owner_token = self
            .owned_canvas_copy
            .as_ref()
            .map(|copy| copy.token.clone());
        cx.notify();
        cx.spawn(async move |this, cx| {
            let read = request
                .await
                .map_err(|error| {
                    tr_format!(
                        "无法读取系统剪贴板：{error}",
                        "Could not read the system clipboard: {error}"
                    )
                })
                .and_then(|item| {
                    item.ok_or_else(|| {
                        tr(
                            "剪贴板为空或当前系统无法提供图片",
                            "The clipboard is empty or the system cannot supply an image",
                        )
                        .into()
                    })
                })
                .and_then(|item| classify_paste(item, owner_token.as_deref()));
            let _ = this.update(cx, |this, cx| {
                if cancel.load(Ordering::Acquire) {
                    this.finish_clipboard_paste(
                        Some(tr("已取消粘贴", "Paste canceled").into()),
                        cx,
                    );
                    return;
                }
                match read {
                    Ok(PasteContent::LocalObjects) => {
                        this.importing = false;
                        this.clipboard_cancel = None;
                        this.canvas
                            .update(cx, |canvas, cx| canvas.set_editable(true, cx));
                        let destination = at.or_else(|| {
                            this.owned_canvas_copy
                                .as_ref()
                                .map(|copy| copy.origin + WorldPoint::new(32., 32.))
                        });
                        let error = destination.and_then(|destination| {
                            this.canvas
                                .update(cx, |canvas, cx| {
                                    canvas.paste_copied_selection_at(destination, cx)
                                })
                                .err()
                                .map(|error| error.to_string())
                        });
                        this.finish_clipboard_paste(error, cx);
                    }
                    Ok(PasteContent::Image(image)) => {
                        this.store_clipboard_image(image, at, cancel, cx)
                    }
                    Err(error) => this.finish_clipboard_paste(Some(error), cx),
                }
            });
        })
        .detach();
    }

    fn store_clipboard_image(
        &mut self,
        image: gpui::Image,
        at: Option<WorldPoint>,
        cancel: Arc<AtomicBool>,
        cx: &mut Context<Self>,
    ) {
        let Some(mut project) = self.project.clone() else {
            self.finish_clipboard_paste(
                Some(
                    tr(
                        "请先新建本地项目，再粘贴图片",
                        "Create a local project before pasting an image",
                    )
                    .into(),
                ),
                cx,
            );
            return;
        };
        if self.canvas.read(cx).editor.graph().nodes.len() >= MAX_CARDS {
            self.finish_clipboard_paste(
                Some(tr("画布对象数量已达上限", "The canvas is at its object limit").into()),
                cx,
            );
            return;
        }
        let project_id = project.metadata().id.clone();
        let document = self.canvas.read(cx).editor.document();
        let job_cancel = cancel.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    project
                        .import_clipboard_image(&image.bytes, image.format.mime_type(), &job_cancel)
                        .map(|asset| (project, asset))
                        .map_err(|error| error.to_string())
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this
                    .project
                    .as_ref()
                    .is_none_or(|project| project.metadata().id != project_id)
                {
                    this.finish_clipboard_paste(
                        Some(tr("项目已切换，未向当前画布放置旧图片", "The project changed. The image was not placed.").into()),
                        cx,
                    );
                    return;
                }
                match result {
                    Ok((project, asset)) => {
                        this.last_saved_document = Some(project.document().clone());
                        this.project = Some(project);
                        this.refresh_assets(cx);
                        this.importing = false;
                        this.canvas
                            .update(cx, |canvas, cx| canvas.set_editable(true, cx));
                        let notice = if cancel.load(Ordering::Acquire)
                            || this.canvas.read(cx).editor.document() != document
                        {
                            tr("粘贴放置已取消；已复制的图片保留在本项目素材中", "Placement canceled. The image is still available in Project Assets.").into()
                        } else {
                            let view = this.canvas.read(cx);
                            let id = view
                                .editor
                                .graph()
                                .fresh_id(&format!("clipboard-{}", this.next_id));
                            this.next_id += 1;
                            let point = at.unwrap_or_else(|| {
                                view.editor.viewport.screen_to_world(WorldPoint::new(
                                    this.canvas_size.width / 2. - 180.,
                                    (this.canvas_size.height / 2. - 160.).max(116.),
                                ))
                            });
                            match super::project_ui::imported_asset_node(
                                &this.registry,
                                &asset,
                                id.into(),
                                point,
                            ) {
                                Ok(node) => {
                                    let id = node.id.clone();
                                    let added = if at.is_some() {
                                        this.add_drawn_object(node, cx)
                                    } else {
                                        this.add_object_in_free_space(node, cx)
                                    };
                                    if added {
                                        this.command(
                                            CanvasCommand::SelectNode {
                                                node: id,
                                                mode: SelectionMode::Replace,
                                            },
                                            cx,
                                        );
                                        tr("已将剪贴板图片复制到本地项目", "Copied the clipboard image into the project").into()
                                    } else {
                                        tr("图片已复制，但无法放入该位置；可从本项目素材重新放置", "The image could not be placed here. Place it again from Project Assets.")
                                            .into()
                                    }
                                }
                                Err(error) => tr_format!("图片已复制，但未放入画布：{error}", "Copied the image but could not place it on the canvas: {error}"),
                            }
                        };
                        this.finish_clipboard_paste(Some(notice), cx);
                    }
                    Err(error) => {
                        this.finish_clipboard_paste(Some(tr_format!("粘贴图片失败：{error}", "Could not paste image: {error}")), cx)
                    }
                }
            });
        })
        .detach();
    }

    fn finish_clipboard_paste(&mut self, notice: Option<String>, cx: &mut Context<Self>) {
        self.importing = false;
        self.clipboard_cancel = None;
        self.canvas
            .update(cx, |canvas, cx| canvas.set_editable(true, cx));
        let owner = cx.weak_entity();
        cx.defer(move |cx| {
            let _ = owner.update(cx, |this, cx| {
                this.notice = notice;
                cx.notify();
            });
        });
        if self.project.is_none() {
            if let Some(handle) = self.pending_window_close.take() {
                let _ = handle.update(cx, |_, window, _| window.remove_window());
            }
        } else if self.pending_window_close.is_some() || self.return_after_save {
            self.save_project(cx);
        } else {
            self.queue_autosave(cx);
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;
    #[test]
    fn own_metadata_routes_objects_and_external_image_wins_over_old_copy() {
        let token = "local-current-copy";
        assert!(matches!(
            classify_paste(
                ClipboardItem::new_string_with_metadata("title".into(), token.into()),
                Some(token)
            ),
            Ok(PasteContent::LocalObjects)
        ));
        let image = gpui::Image::from_bytes(gpui::ImageFormat::Png, vec![1, 2, 3]);
        assert!(matches!(
            classify_paste(ClipboardItem::new_image(&image), Some(token)),
            Ok(PasteContent::Image(_))
        ));
        assert!(
            classify_paste(
                ClipboardItem::new_string_with_metadata("title".into(), "other-board".into()),
                Some(token)
            )
            .is_err()
        );
        assert!(classify_paste(ClipboardItem::new_string(token.into()), Some(token)).is_err());
    }
    #[test]
    fn clipboard_text_urls_paths_and_unsupported_or_ambiguous_images_are_not_opened() {
        for text in [
            "/tmp/image.png",
            "file:///tmp/image.png",
            "https://example.org/image.png",
        ] {
            assert!(classify_paste(ClipboardItem::new_string(text.into()), None).is_err());
        }
        for format in [
            gpui::ImageFormat::Svg,
            gpui::ImageFormat::Gif,
            gpui::ImageFormat::Webp,
        ] {
            assert!(
                classify_paste(
                    ClipboardItem::new_image(&gpui::Image::from_bytes(format, vec![1])),
                    None
                )
                .is_err()
            );
        }
        let image = gpui::Image::from_bytes(gpui::ImageFormat::Png, vec![1]);
        assert!(
            classify_paste(
                ClipboardItem {
                    entries: vec![
                        ClipboardEntry::Image(image.clone()),
                        ClipboardEntry::Image(image)
                    ]
                },
                None
            )
            .is_err()
        );
    }
}
