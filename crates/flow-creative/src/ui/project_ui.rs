use super::*;
use std::time::Duration;

impl CreativeCanvas {
    pub(super) fn close_ready(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.apply_form_edit(cx);
        if self.saving || self.importing {
            self.pending_window_close = Some(window.window_handle());
            self.notice = Some("正在完成本地保存，完成后关闭…".into());
            return false;
        }
        let Some(mut project) = self.project.clone() else {
            return true;
        };
        let document = self.canvas.read(cx).editor.document();
        if self.last_saved_document.as_ref() == Some(&document) {
            return true;
        }
        // A close request waits for the bounded local write; no detached task can
        // lose the last keystroke after the view has been destroyed.
        match project.save_document(&document, &self.registry) {
            Ok(()) => {
                self.project = Some(project);
                self.last_saved_document = Some(document);
                true
            }
            Err(error) => {
                self.notice = Some(format!("保存失败，窗口已保留：{error}"));
                cx.notify();
                false
            }
        }
    }
    fn close_after_saved(&mut self, cx: &mut Context<Self>) {
        if let Some(handle) = self.pending_window_close.take() {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        }
    }

    pub(super) fn queue_form_edit(&mut self, cx: &mut Context<Self>) {
        self.form_serial += 1;
        let serial = self.form_serial;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(300))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.form_serial == serial {
                    this.apply_form_edit(cx);
                }
            });
        })
        .detach();
    }
    pub(super) fn apply_form_edit(&mut self, cx: &mut Context<Self>) {
        if !self.canvas.read(cx).is_editable() {
            return;
        }
        let Some(source) = self.form_source.clone() else {
            return;
        };
        let id = source.0.clone();
        let Some(mut node) = self.canvas.read(cx).editor.graph().node(&id).cloned() else {
            return;
        };
        let value = self.form.read(cx).value();
        let Some(data) = form_data_update(&node, &source, &value) else {
            return;
        };
        node.data = data;
        if self.registry.validate_node(&node).is_err() {
            return;
        }
        self.form_source = Some(form_identity(&node));
        self.command(
            CanvasCommand::SetNodeData {
                node: id,
                data: node.data,
            },
            cx,
        );
    }

    /// Bind a project after constructing this view from `project.document()`.
    /// Ctrl/Cmd+S and background changes then use the same local project owner.
    pub fn bind_project(&mut self, project: LocalProject, cx: &mut Context<Self>) {
        self.last_saved_document = Some(project.document().clone());
        self.project = Some(project);
        self.refresh_assets(cx);
        self.canvas.update(cx, |v, _| v.set_host_persistence(true));
        self.notice = None;
        cx.notify();
    }
    pub fn set_generation_available(&mut self, available: bool, cx: &mut Context<Self>) {
        self.generation_available = available;
        cx.notify();
    }
    /// Host results use the same validation, undo and local-save path as edits.
    /// Imported asset keys must already belong to the bound project.
    pub fn apply_generation_update(
        &mut self,
        node: NodeId,
        status: &str,
        asset: Option<&str>,
        cx: &mut Context<Self>,
    ) -> Result<(), CreativeError> {
        let mut current = self
            .canvas
            .read(cx)
            .editor
            .graph()
            .node(&node)
            .cloned()
            .ok_or_else(|| CreativeError("生成节点已被删除".into()))?;
        if GenerationKind::from_node_kind(&current.kind).is_none() {
            return Err(CreativeError("目标不是生成节点".into()));
        }
        current.data["status"] = Value::String(status.into());
        if let Some(key) = asset {
            if let Some(project) = &self.project {
                project
                    .resolve_asset(key)
                    .map_err(|e| CreativeError(e.to_string()))?;
            }
            current.data["asset"] = Value::String(key.into());
        }
        self.registry.validate_node(&current)?;
        self.canvas
            .update(cx, |v, cx| {
                v.execute_command(
                    CanvasCommand::SetNodeData {
                        node,
                        data: current.data,
                    },
                    cx,
                )
            })
            .map_err(|e| CreativeError(e.to_string()))
    }
    fn refresh_assets(&mut self, cx: &mut Context<Self>) {
        let mut assets = CreativeAssets::demo();
        if let Some(project) = &self.project {
            for asset in project.assets() {
                if let Ok(path) = project.resolve_asset(&asset.key) {
                    assets.register(&asset.key, path);
                }
            }
        }
        self.canvas.update(cx, |v, cx| {
            v.registry = demo_renderers(assets);
            cx.notify();
        });
    }
    pub(super) fn queue_autosave(&mut self, cx: &mut Context<Self>) {
        if self.project.is_none() || self.importing {
            return;
        }
        self.save_serial += 1;
        let serial = self.save_serial;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(450))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.save_serial == serial && !this.saving {
                    this.save_project(cx);
                }
            });
        })
        .detach();
    }
    pub(super) fn save_project(&mut self, cx: &mut Context<Self>) {
        if self.saving || self.importing {
            return;
        }
        let Some(mut project) = self.project.clone() else {
            return;
        };
        let document = self.canvas.read(cx).editor.document();
        if self.last_saved_document.as_ref() == Some(&document) {
            if self.return_after_save {
                self.return_after_save = false;
                cx.emit(CreativeEvent::BackToProjects);
            }
            self.close_after_saved(cx);
            return;
        }
        let registry = self.registry.clone();
        self.saving = true;
        let saved = document.clone();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { project.save_document(&document, &registry).map(|_| project) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.saving = false;
                match result {
                    Ok(project) => {
                        this.project = Some(project);
                        this.last_saved_document = Some(saved);
                        // A user may type while the background write is running.
                        // Include that draft before deciding it is safe to leave.
                        this.apply_form_edit(cx);
                        if this.last_saved_document.as_ref()
                            != Some(&this.canvas.read(cx).editor.document())
                        {
                            this.queue_autosave(cx);
                        } else {
                            if this.return_after_save {
                                this.return_after_save = false;
                                cx.emit(CreativeEvent::BackToProjects);
                            }
                            this.close_after_saved(cx);
                        }
                    }
                    Err(error) => {
                        this.return_after_save = false;
                        this.pending_window_close = None;
                        this.notice = Some(format!("保存失败，修改仍在画布中：{error}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
    pub(super) fn import_image(&mut self, replace_selection: bool, cx: &mut Context<Self>) {
        let dialog = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("选择本地图片 · 复制到项目素材目录".into()),
        });
        self.import_image_request(
            replace_selection,
            async move {
                match dialog.await {
                    Ok(Ok(paths)) => Ok(paths),
                    Ok(Err(error)) => Err(error.to_string()),
                    Err(error) => Err(error.to_string()),
                }
            },
            cx,
        );
    }
    pub(super) fn import_explicit_image_path(&mut self, cx: &mut Context<Self>) {
        let value = self.local_path.read(cx).value();
        let path = PathBuf::from(value.trim());
        if !path.is_absolute() {
            self.notice = Some("请输入已选择图片的完整绝对路径".into());
            cx.notify();
            return;
        }
        self.import_image_request(false, std::future::ready(Ok(Some(vec![path]))), cx);
    }
    fn import_image_request(
        &mut self,
        replace_selection: bool,
        paths: impl std::future::Future<Output = Result<Option<Vec<PathBuf>>, String>> + 'static,
        cx: &mut Context<Self>,
    ) {
        self.apply_form_edit(cx);
        if self.saving || self.importing || !self.canvas.read(cx).is_editable() {
            return;
        }
        let Some(mut project) = self.project.clone() else {
            self.notice = Some("请先新建本地项目，再导入图片".into());
            cx.notify();
            return;
        };
        let replacement = self
            .selected(cx)
            .filter(|n| replace_selection && n.kind == "image")
            .map(|n| n.id);
        let document = self.canvas.read(cx).editor.document();
        let registry = self.registry.clone();
        self.importing = true;
        self.canvas.update(cx, |v, cx| v.set_editable(false, cx));
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = match paths.await {
                Ok(Some(paths)) if !paths.is_empty() => Some(
                    cx.background_executor()
                        .spawn(async move {
                            project
                                .save_document(&document, &registry)
                                .map_err(|e| e.to_string())?;
                            let mut imports = vec![];
                            let mut errors = vec![];
                            for path in paths {
                                match project.import_image(&path) {
                                    Ok(asset) => imports.push(asset),
                                    Err(error) => errors.push(format!(
                                        "{}: {error}",
                                        path.file_name().unwrap_or_default().to_string_lossy()
                                    )),
                                }
                            }
                            Ok::<_, String>((project, imports, errors))
                        })
                        .await,
                ),
                Ok(_) => None,
                Err(error) => Some(Err(error)),
            };
            let _ = this.update(cx, |this, cx| {
                this.importing = false;
                this.canvas.update(cx, |v, cx| v.set_editable(true, cx));
                match result {
                    Some(Ok((project, assets, errors))) => {
                        this.show_import_path = !errors.is_empty();
                        this.last_saved_document = Some(project.document().clone());
                        this.project = Some(project);
                        this.refresh_assets(cx);
                        let replace = if assets.len() == 1 { replacement } else { None };
                        let imported_count = assets.len();
                        for (index, asset) in assets.into_iter().enumerate() {
                            if let Some(id) = replace.as_ref() {
                                if let Some(node) =
                                    this.canvas.read(cx).editor.graph().node(id).cloned()
                                {
                                    let mut data = node.data;
                                    data["asset"] = Value::String(asset.key);
                                    data["caption"] = Value::String(asset.name);
                                    this.command(
                                        CanvasCommand::SetNodeData {
                                            node: id.clone(),
                                            data,
                                        },
                                        cx,
                                    );
                                }
                            } else {
                                let view = this.canvas.read(cx);
                                let id = view
                                    .editor
                                    .graph()
                                    .fresh_id(&format!("image-{}", this.next_id));
                                this.next_id += 1;
                                let position =
                                    view.editor.viewport.screen_to_world(WorldPoint::new(
                                        this.canvas_size.width / 2. - 180. + index as f32 * 28.,
                                        this.canvas_size.height / 2. - 160. + index as f32 * 28.,
                                    ));
                                if let Ok(mut node) = this.registry.make_node(
                                    "image",
                                    id.clone(),
                                    position,
                                    Some(
                                        serde_json::json!({"asset":asset.key,"caption":asset.name}),
                                    ),
                                ) {
                                    node.title = asset.name;
                                    node.size = imported_image_size(asset.width, asset.height);
                                    if this.command(CanvasCommand::AddNode(node), cx) {
                                        this.command(
                                            CanvasCommand::SelectNode {
                                                node: id.into(),
                                                mode: SelectionMode::Replace,
                                            },
                                            cx,
                                        );
                                    }
                                }
                            }
                        }
                        this.notice = Some(if errors.is_empty() {
                            format!("已导入 {imported_count} 张图片，素材已复制到项目")
                        } else {
                            format!("已导入 {imported_count} 张；{}", errors.join("；"))
                        });
                        this.queue_autosave(cx);
                    }
                    Some(Err(error)) => {
                        this.show_import_path = true;
                        this.pending_window_close = None;
                        this.notice = Some(format!(
                            "文件选择/导入失败，可使用右侧本地路径入口：{error}"
                        ));
                    }
                    None => {}
                }
                if this.pending_window_close.is_some() {
                    this.save_project(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }
}
fn imported_image_size(width: u32, height: u32) -> WorldSize {
    let width = width.max(1) as f32;
    let height = height.max(1) as f32;
    if width >= height {
        WorldSize::new(420., 420. * height / width)
    } else {
        WorldSize::new(420. * width / height, 420.)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[::core::prelude::v1::test]
    fn image_import_preserves_aspect_ratio_at_default_size() {
        let size = imported_image_size(1600, 900);
        assert_eq!(size.width, 420.);
        assert!((size.width / size.height - 16. / 9.).abs() < 0.001);
        let narrow = imported_image_size(1, 1000);
        assert!((narrow.width / narrow.height - 0.001).abs() < 0.000001);
    }
}
