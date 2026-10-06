use super::*;
use std::time::Duration;

impl CreativeCanvas {
    pub(super) fn close_ready(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.clear_asset_export();
        if let Some(cancel) = &self.image_edit_cancel {
            cancel.store(true, std::sync::atomic::Ordering::Release);
        }
        if let Some(job) = &self.custom_job {
            job.cancel.store(true, std::sync::atomic::Ordering::Release);
        }
        if let Some(job) = &self.processing_job {
            job.cancel.store(true, std::sync::atomic::Ordering::Release);
        }
        self.apply_form_edit(cx);
        if self.saving || self.importing || !self.deferred_edits.is_empty() {
            self.pending_window_close = Some(window.window_handle());
            self.notice = Some("正在完成本地保存，完成后关闭…".into());
            return false;
        }
        let Some(mut project) = self.project.clone() else {
            return true;
        };
        let document = self.canvas.read(cx).editor.document();
        if self.last_saved_document.as_ref() == Some(&document)
            && self.pending_project_name.is_none()
        {
            return true;
        }
        // A close request waits for the bounded local write; no detached task can
        // lose the last keystroke after the view has been destroyed.
        match project.save_named_document(
            &document,
            &self.registry,
            self.pending_project_name.as_deref(),
        ) {
            Ok(()) => {
                self.project = Some(project);
                self.last_saved_document = Some(document);
                self.pending_project_name = None;
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
                    if this.deferred_edits.is_empty()
                        && (this.return_after_save || this.pending_window_close.is_some())
                    {
                        this.save_project(cx);
                    }
                }
            });
        })
        .detach();
    }
    pub(super) fn apply_form_edit(&mut self, cx: &mut Context<Self>) {
        // Selection starts a capture before its change event is delivered. Keep
        // the old editor's draft independently of the newly selected form.
        if let Some(source) = self.form_source.clone()
            && let Some(node) = self.canvas.read(cx).editor.graph().node(&source.0)
            && source == form_identity(node)
        {
            let text = self.form.read(cx).value().to_string();
            if form_data_update(node, &source, &text).is_some() {
                self.deferred_edits
                    .insert(source.0.clone(), DeferredFormEdit { source, text });
            } else {
                self.deferred_edits.remove(&source.0);
            }
        }
        if !self.canvas.read(cx).is_editable() {
            return;
        }
        if self.canvas.read(cx).interactions.has_capture()
            || self.canvas.read(cx).editor.transaction_active()
        {
            if !self.deferred_edits.is_empty() {
                self.queue_form_edit(cx);
            }
            return;
        }
        let edits = std::mem::take(&mut self.deferred_edits);
        for (id, draft) in edits {
            let Some(mut node) = self.canvas.read(cx).editor.graph().node(&id).cloned() else {
                continue;
            };
            let Some(data) = form_data_update(&node, &draft.source, &draft.text) else {
                continue;
            };
            node.data = data;
            if let Err(error) = self.registry.validate_node(&node) {
                self.notice = Some(format!("内容暂未保存：{error}"));
                continue;
            }
            let identity = form_identity(&node);
            if self.command(
                CanvasCommand::SetNodeData {
                    node: id,
                    data: node.data,
                },
                cx,
            ) && self.form_source.as_ref() == Some(&draft.source)
            {
                self.form_source = Some(identity);
            }
        }
    }

    /// Bind a project after constructing this view from `project.document()`.
    /// Ctrl/Cmd+S and background changes then use the same local project owner.
    pub fn bind_project(&mut self, project: LocalProject, cx: &mut Context<Self>) {
        let migrated = project.migrated_reference_count();
        self.last_saved_document = Some(project.document().clone());
        self.project = Some(project);
        self.refresh_assets(cx);
        self.canvas.update(cx, |v, _| v.set_host_persistence(true));
        self.notice = (migrated > 0)
            .then(|| format!("已将 {migrated} 个旧参考附件转换为画布图片输入，原素材已保留"));
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
                    .resolve_asset_path(key)
                    .map_err(|e| CreativeError(e.to_string()))?;
            }
            current.data["asset"] = Value::String(key.into());
        }
        self.registry.validate_node(&current)?;
        if let Some(project) = &self.project {
            let mut document = self.canvas.read(cx).editor.document();
            if let Some(candidate) = document.graph.nodes.get_mut(&node) {
                candidate.data = current.data.clone();
            }
            project
                .validate_document(&document, &self.registry)
                .map_err(|error| CreativeError(error.to_string()))?;
        }
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
    pub(super) fn refresh_assets(&mut self, cx: &mut Context<Self>) {
        let mut assets = CreativeAssets::demo();
        if let Some(project) = &self.project {
            for asset in project.assets() {
                if let Ok(path) = project.resolve_asset_path(&asset.key) {
                    assets.register(&asset.key, path);
                    assets
                        .dimensions
                        .insert(asset.key.clone(), (asset.width, asset.height));
                    assets
                        .media_types
                        .insert(asset.key.clone(), asset.media_type.clone());
                }
            }
        }
        self.set_assets(assets, cx);
    }
    /// Install Creative's standard media/text renderers and local asset sources.
    /// Hosts supplying custom renderers can retain their own registry instead.
    pub fn set_assets(&mut self, mut assets: CreativeAssets, cx: &mut Context<Self>) {
        assets.font_families = self.font_families.clone();
        self.mask_views
            .sync(self.canvas.read(cx).editor.graph(), self.project.as_ref());
        assets.masks = self.mask_views.clone();
        self.sync_processing_views(cx);
        assets.processing_errors = self.processing_errors.clone();
        self.sync_custom_views(cx);
        assets.custom_views = self.custom_views.clone();
        self.known_custom_kinds = self
            .canvas
            .read(cx)
            .editor
            .graph()
            .nodes
            .values()
            .filter(|n| n.kind.starts_with("custom."))
            .map(|n| n.kind.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        assets.custom_kinds = self.known_custom_kinds.clone();
        assets.models = self.model_views.clone();
        assets.shapes = self.shape_views.clone();
        assets.media = self.media_views.clone();
        self.media_views.set_owner(cx.entity().downgrade());
        assets.editing_text = self.editing_text.clone();
        assets.editing_image = self.editing_image.clone();
        assets.adjustment_presentation = self.adjustment_presentation.clone();
        let owner = cx.entity().downgrade();
        assets.begin_text_edit = Some(Arc::new(move |node, window, cx| {
            let _ = owner.update(cx, |this, cx| this.begin_text_edit(node, window, cx));
        }));
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
        if !self.deferred_edits.is_empty() {
            self.queue_form_edit(cx);
            return;
        }
        if self.saving || self.importing {
            return;
        }
        let Some(mut project) = self.project.clone() else {
            return;
        };
        let document = self.canvas.read(cx).editor.document();
        if self.last_saved_document.as_ref() == Some(&document)
            && self.pending_project_name.is_none()
        {
            if self.return_after_save {
                self.return_after_save = false;
                cx.emit(CreativeEvent::BackToProjects);
            }
            self.close_after_saved(cx);
            return;
        }
        let registry = self.registry.clone();
        let requested_name = self.pending_project_name.clone();
        self.saving = true;
        let saved = document.clone();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let name = requested_name.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    project
                        .save_named_document(&document, &registry, name.as_deref())
                        .map(|_| project)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.saving = false;
                match result {
                    Ok(project) => {
                        this.project = Some(project);
                        if this.pending_project_name == requested_name {
                            this.pending_project_name = None;
                        }
                        this.last_saved_document = Some(saved);
                        // A user may type while the background write is running.
                        // Include that draft before deciding it is safe to leave.
                        this.apply_form_edit(cx);
                        if this.last_saved_document.as_ref()
                            != Some(&this.canvas.read(cx).editor.document())
                            || this.pending_project_name.is_some()
                            || !this.deferred_edits.is_empty()
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
        self.import_replacement = self
            .selected(cx)
            .filter(|n| replace_selection && n.kind == "image")
            .map(|n| n.id);
        if replace_selection && self.import_replacement.is_none() {
            return;
        }
        let dialog = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: !replace_selection,
            prompt: Some(
                if self.import_model {
                    "选择本地 GLB · 复制到项目素材目录"
                } else {
                    "选择本地图片、视频或音频 · 复制到项目素材目录"
                }
                .into(),
            ),
        });
        self.import_image_request(
            self.import_replacement.clone(),
            None,
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
            self.notice = Some("请输入已选择文件的完整绝对路径".into());
            cx.notify();
            return;
        }
        self.import_image_request(
            self.import_replacement.clone(),
            None,
            std::future::ready(Ok(Some(vec![path]))),
            cx,
        );
    }
    pub(super) fn import_dropped_paths(
        &mut self,
        paths: Vec<PathBuf>,
        at: WorldPoint,
        cx: &mut Context<Self>,
    ) {
        if paths.len() > MAX_CARDS.saturating_sub(self.canvas.read(cx).editor.graph().nodes.len()) {
            self.notice = Some("画布对象数量已达上限，请减少本次导入数量".into());
            cx.notify();
            return;
        }
        self.import_replacement = None;
        self.import_image_request(None, Some(at), std::future::ready(Ok(Some(paths))), cx);
    }
    pub(super) fn import_image_request(
        &mut self,
        replacement: Option<NodeId>,
        drop_at: Option<WorldPoint>,
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
        if replacement.as_ref().is_some_and(|id| {
            self.canvas
                .read(cx)
                .editor
                .graph()
                .node(id)
                .is_none_or(|n| n.kind != "image")
        }) {
            self.notice = Some("要替换的图片节点已不存在，请重新选择图片".into());
            self.import_replacement = None;
            self.show_import_path = false;
            cx.notify();
            return;
        }
        let replace_selection = replacement.is_some();
        let document = self.canvas.read(cx).editor.document();
        let registry = self.registry.clone();
        let is_model = self.import_model;
        self.importing = true;
        self.canvas.update(cx, |v, cx| v.set_editable(false, cx));
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = match paths.await {
                Ok(Some(paths)) if replace_selection && paths.len() > 1 => {
                    Some(Err("替换图片时请选择一个文件".into()))
                }
                Ok(Some(paths)) if !paths.is_empty() => Some(
                    cx.background_executor()
                        .spawn(async move {
                            project
                                .save_document(&document, &registry)
                                .map_err(|e| e.to_string())?;
                            let mut imports = vec![];
                            let mut errors = vec![];
                            for path in paths {
                                let extension = path
                                    .extension()
                                    .and_then(|e| e.to_str())
                                    .unwrap_or_default()
                                    .to_ascii_lowercase();
                                let model_path =
                                    !replace_selection && (is_model || extension == "glb");
                                let media_path = !replace_selection
                                    && matches!(
                                        extension.as_str(),
                                        "mp4" | "webm" | "wav" | "mp3" | "flac" | "ogg" | "m4a"
                                    );
                                match if model_path {
                                    project.import_model(&path)
                                } else if media_path {
                                    project.import_media(&path)
                                } else {
                                    project.import_image(&path)
                                } {
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
                        if errors.is_empty() {
                            this.import_replacement = None;
                        }
                        this.last_saved_document = Some(project.document().clone());
                        this.project = Some(project);
                        this.refresh_assets(cx);
                        let replace = if assets.len() == 1 { replacement } else { None };
                        let imported_count = assets.len();
                        for (index, asset) in assets.into_iter().enumerate() {
                            let is_model = asset.media_type == "model/gltf-binary";
                            let kind = if is_model {
                                "model"
                            } else if asset.media_type.starts_with("video/") {
                                "video"
                            } else if asset.media_type.starts_with("audio/") {
                                "audio"
                            } else {
                                "image"
                            };
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
                                    .fresh_id(&format!("{}-{}", kind, this.next_id));
                                this.next_id += 1;
                                let position = drop_at
                                    .map(|at| {
                                        at + WorldPoint::new(index as f32 * 28., index as f32 * 28.)
                                    })
                                    .unwrap_or_else(|| {
                                        view.editor.viewport.screen_to_world(WorldPoint::new(
                                            this.canvas_size.width / 2. - 180. + index as f32 * 28.,
                                            if is_model {
                                                ((this.canvas_size.height
                                                    - 315. * view.editor.viewport.zoom
                                                    - 250.)
                                                    / 2.)
                                                    .max(96.)
                                                    + index as f32 * 28.
                                            } else {
                                                this.canvas_size.height / 2. - 160.
                                                    + index as f32 * 28.
                                            },
                                        ))
                                    });
                                if let Ok(mut node) = this.registry.make_node(
                                    kind,
                                    id.clone(),
                                    position,
                                    Some(serde_json::json!({"asset":asset.key,"caption":""})),
                                ) {
                                    node.title = asset.name;
                                    node.size = if is_model {
                                        WorldSize::new(420., 315.)
                                    } else if kind == "audio" {
                                        WorldSize::new(400., 144.)
                                    } else {
                                        imported_image_size(asset.width, asset.height)
                                    };
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
                            format!("已导入 {imported_count} 个素材，已复制到本地项目")
                        } else {
                            format!("已导入 {imported_count} 个；{}", errors.join("；"))
                        });
                        this.show_import_path = !errors.is_empty();
                        this.queue_autosave(cx);
                    }
                    Some(Err(error)) => {
                        this.show_import_path = true;
                        this.pending_window_close = None;
                        this.notice = Some(format!(
                            "文件选择/导入失败，可使用右侧本地路径入口：{error}"
                        ));
                    }
                    None => {
                        this.import_replacement = None;
                        this.show_import_path = false;
                    }
                }
                if this.pending_window_close.is_some() {
                    this.save_project(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn change_model_camera(
        &mut self,
        yaw: f32,
        pitch: f32,
        zoom: f32,
        reset: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(mut node) = self
            .selected(cx)
            .filter(|n| matches!(n.kind.as_str(), "model" | "generate_model"))
        else {
            return;
        };
        let mut state = node
            .data
            .get("camera")
            .and_then(|v| {
                serde_json::from_value::<crate::model_preview::OrbitCameraState>(v.clone()).ok()
            })
            .unwrap_or_else(|| crate::model_preview::OrbitCamera::default().state());
        if reset {
            state = crate::model_preview::OrbitCamera::default().state();
        } else {
            state.yaw_radians = (state.yaw_radians + yaw).rem_euclid(std::f32::consts::TAU);
            state.pitch_radians = (state.pitch_radians + pitch).clamp(-1.56, 1.56);
            state.distance_factor = (state.distance_factor * zoom).clamp(0.05, 100.);
        }
        node.data["camera"] = serde_json::to_value(state).expect("bounded camera");
        self.command(
            CanvasCommand::SetNodeData {
                node: node.id,
                data: node.data,
            },
            cx,
        );
    }
}
pub(super) fn imported_image_size(width: u32, height: u32) -> WorldSize {
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
