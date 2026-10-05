use super::*;
use gpui_flow_interaction::{CanvasCommand, InteractionError};

pub(super) fn validate_host_document(
    document: &model::FlowDocument,
    validator: Option<&FlowDocumentValidator>,
) -> Result<(), String> {
    document.validate().map_err(|e| e.to_string())?;
    if let Some(validator) = validator {
        validator(document)?;
    }
    Ok(())
}

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
        let document = self.editor.document();
        if let Err(error) = validate_host_document(&document, self.document_validator.as_ref()) {
            self.message = format!("Save failed: {error}");
            cx.notify();
            return;
        }
        if !self.begin_file_operation(cx) {
            return;
        }
        let path = path.into();
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
        if !self.is_editable() {
            self.message = "Canvas is read-only".into();
            cx.notify();
            return;
        }
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
                        if let Err(error) = this.replace_document(document, cx) {
                            this.message = error.to_string();
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
        if self.host_persistence {
            cx.emit(FlowEvent::SaveRequested);
            return;
        }
        if let Some(path) = self.document_path.clone() {
            self.save_to_path(path, cx);
            return;
        }
        let document = self.editor.document();
        if let Err(error) = validate_host_document(&document, self.document_validator.as_ref()) {
            self.message = format!("Save failed: {error}");
            cx.notify();
            return;
        }
        if !self.begin_file_operation(cx) {
            return;
        }
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
        if !self.is_editable() {
            self.message = "Canvas is read-only".into();
            cx.notify();
            return;
        }
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
                            if let Err(error) = this.replace_document(document, cx) {
                                this.message = error.to_string();
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
    ) -> Result<(), InteractionError> {
        let graph = Graph::from_json(json)?;
        validate_host_document(
            &model::FlowDocument {
                graph: graph.clone(),
                viewport: self.editor.viewport,
            },
            self.document_validator.as_ref(),
        )
        .map_err(InteractionError::InvalidInput)?;
        self.cancel_interaction(cx);
        self.execute_command(CanvasCommand::ImportGraph(graph), cx)?;
        self.reset_document_runtime();
        self.changed(cx);
        Ok(())
    }
    fn replace_document(
        &mut self,
        document: model::FlowDocument,
        cx: &mut Context<Self>,
    ) -> Result<(), InteractionError> {
        validate_host_document(&document, self.document_validator.as_ref())
            .map_err(InteractionError::InvalidInput)?;
        self.cancel_interaction(cx);
        self.execute_command(CanvasCommand::LoadDocument(document), cx)?;
        self.reset_document_runtime();
        self.message = "Document opened".into();
        self.changed(cx);
        Ok(())
    }
    fn reset_document_runtime(&mut self) {
        self.runtime = Runtime::default();
        self.workflow = None;
        self.media_players.clear();
        self.input_node = None;
        self.input = None;
        self.input_subscription = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;
    #[test]
    fn host_document_contract_validates_without_mutation_or_legacy_changes() {
        let document = model::FlowDocument::default();
        assert!(validate_host_document(&document, None).is_ok());
        let reject: FlowDocumentValidator = Arc::new(|_| Err("unknown host type".into()));
        assert_eq!(
            validate_host_document(&document, Some(&reject)),
            Err("unknown host type".into())
        );
        let never: FlowDocumentValidator =
            Arc::new(|_| panic!("invalid core document reaches host"));
        let mut invalid = document.clone();
        invalid.viewport.zoom = f32::NAN;
        assert!(validate_host_document(&invalid, Some(&never)).is_err());
        assert_eq!(document, model::FlowDocument::default());
    }
}
