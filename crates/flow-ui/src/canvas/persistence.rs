use super::*;

impl FlowCanvas {
    pub fn is_file_busy(&self) -> bool {
        self.file_busy
    }
    fn begin_file_operation(&mut self, cx: &mut Context<Self>) -> bool {
        if self.file_busy {
            self.message = "A file operation is already in progress".into();
            cx.notify();
            false
        } else {
            self.file_busy = true;
            true
        }
    }
    /// Persist to an explicit host-approved path without relying on a desktop portal.
    pub fn save_to_path(&mut self, path: impl Into<std::path::PathBuf>, cx: &mut Context<Self>) {
        if !self.begin_file_operation(cx) {
            return;
        }
        let path = path.into();
        let document = self.editor.document();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    model::save_document(&path, &document)
                        .map(|_| format!("Saved {}", path.display()))
                        .map_err(|e| e.to_string())
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.file_busy = false;
                this.message = result.unwrap_or_else(|e| format!("Save failed: {e}"));
                cx.notify();
            });
        })
        .detach();
    }
    /// Load an explicit path. Validation finishes before replacing the current document.
    pub fn open_path(&mut self, path: impl Into<std::path::PathBuf>, cx: &mut Context<Self>) {
        if !self.begin_file_operation(cx) {
            return;
        }
        let path = path.into();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { model::load_document(path).map_err(|e| e.to_string()) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.file_busy = false;
                match result {
                    Ok(document) => {
                        this.finish_gesture(cx);
                        match this.editor.load_document(document) {
                            Ok(()) => {
                                this.runtime = Runtime::default();
                                this.workflow = None;
                                this.media_players.clear();
                                this.connecting = None;
                                this.input_node = None;
                                this.input = None;
                                this.input_subscription = None;
                                this.message = "Document opened".into();
                                this.changed(cx);
                            }
                            Err(e) => this.message = e.to_string(),
                        }
                    }
                    Err(e) => this.message = format!("Open failed: {e}"),
                }
                cx.notify();
            });
        })
        .detach();
    }
    pub fn save_file(&mut self, cx: &mut Context<Self>) {
        if let Some(path) = self.document_path.clone() {
            self.save_to_path(path, cx);
            return;
        }
        if !self.begin_file_operation(cx) {
            return;
        }
        let document = self.editor.document();
        let directory = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let dialog = cx.prompt_for_new_path(&directory, Some("canvas.flow.json"));
        cx.spawn(async move |this, cx| {
            let result = match dialog.await {
                Ok(Ok(Some(path))) => Some(
                    cx.background_executor()
                        .spawn(async move {
                            model::save_document(&path, &document)
                                .map(|_| format!("Saved {}", path.display()))
                                .map_err(|e| e.to_string())
                        })
                        .await,
                ),
                Ok(Ok(None)) => None,
                Ok(Err(e)) => Some(Err(e.to_string())),
                Err(e) => Some(Err(e.to_string())),
            };
            let _ = this.update(cx, |this, cx| {
                this.file_busy = false;
                cx.notify();
            });
            if let Some(result) = result {
                let _ = this.update(cx, |this, cx| {
                    this.message = result.unwrap_or_else(|e| format!("Save failed: {e}"));
                    cx.notify();
                });
            }
        })
        .detach();
    }
    pub fn open_file(&mut self, cx: &mut Context<Self>) {
        if let Some(path) = self.document_path.clone() {
            self.open_path(path, cx);
            return;
        }
        if !self.begin_file_operation(cx) {
            return;
        }
        let dialog = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Open flow document".into()),
        });
        cx.spawn(async move |this, cx| {
            let result = match dialog.await {
                Ok(Ok(Some(paths))) => {
                    if let Some(path) = paths.first().cloned() {
                        Some(
                            cx.background_executor()
                                .spawn(async move {
                                    model::load_document(path).map_err(|e| e.to_string())
                                })
                                .await,
                        )
                    } else {
                        None
                    }
                }
                Ok(Ok(None)) => None,
                Ok(Err(e)) => Some(Err(e.to_string())),
                Err(e) => Some(Err(e.to_string())),
            };
            let _ = this.update(cx, |this, cx| {
                this.file_busy = false;
                cx.notify();
            });
            if let Some(result) = result {
                let _ = this.update(cx, |this, cx| {
                    match result {
                        Ok(document) => {
                            this.finish_gesture(cx);
                            match this.editor.load_document(document) {
                                Ok(()) => {
                                    this.input_node = None;
                                    this.runtime = Runtime::default();
                                    this.workflow = None;
                                    this.media_players.clear();
                                    this.connecting = None;
                                    this.message = "Document opened".into();
                                    this.changed(cx);
                                }
                                Err(e) => this.message = e.to_string(),
                            }
                        }
                        Err(e) => this.message = format!("Open failed: {e}"),
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }
    pub fn export_json(&self) -> Result<String, model::GraphError> {
        self.editor.graph().to_json()
    }
    pub fn import_json(
        &mut self,
        json: &str,
        cx: &mut Context<Self>,
    ) -> Result<(), model::GraphError> {
        let graph = Graph::from_json(json)?;
        self.finish_gesture(cx);
        self.editor.transaction("Import", |g| {
            *g = graph;
            Ok(())
        })?;
        self.runtime = Runtime::default();
        self.workflow = None;
        self.media_players.clear();
        self.connecting = None;
        self.input_node = None;
        self.editor.selection.clear();
        self.changed(cx);
        Ok(())
    }
}
