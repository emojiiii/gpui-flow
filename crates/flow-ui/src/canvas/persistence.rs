use super::*;
use gpui_flow_interaction::{CanvasCommand, DispatchReport, InteractionError, PluginHost};

#[derive(PartialEq)]
struct OpenDocumentVersion {
    revision: u64,
    viewport: model::Viewport,
}

impl OpenDocumentVersion {
    fn new(editor: &Editor) -> Self {
        Self {
            revision: editor.document_revision(),
            viewport: editor.viewport,
        }
    }

    fn install(
        &self,
        document: model::FlowDocument,
        editor: &mut Editor,
        interactions: &mut PluginHost,
        validator: Option<&FlowDocumentValidator>,
    ) -> Result<DispatchReport, String> {
        if *self != Self::new(editor)
            || editor.transaction_active()
            || interactions.has_capture()
            || interactions.connection().active()
        {
            return Err(
                "Open cancelled because the canvas changed or an edit is in progress".into(),
            );
        }
        validate_host_document(&document, validator)?;
        interactions
            .execute(editor, CanvasCommand::LoadDocument(document))
            .map_err(|error| error.to_string())
    }
}

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
            cx.notify();
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
    /// Load an explicit path, keeping edits made while the file is read.
    pub fn open_path(&mut self, path: impl Into<std::path::PathBuf>, cx: &mut Context<Self>) {
        if !self.is_editable() {
            self.message = "Canvas is read-only".into();
            cx.notify();
            return;
        }
        if !self.begin_file_operation(cx) {
            return;
        }
        let opened_against = OpenDocumentVersion::new(&self.editor);
        let path = path.into();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { model::load_document(path).map_err(|e| e.to_string()) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.finish_open(opened_against, Some(result), cx);
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
        let opened_against = OpenDocumentVersion::new(&self.editor);
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
                this.finish_open(opened_against, result, cx);
            });
        })
        .detach();
    }
    fn finish_open(
        &mut self,
        opened_against: OpenDocumentVersion,
        result: Option<Result<model::FlowDocument, String>>,
        cx: &mut Context<Self>,
    ) {
        self.file_busy = false;
        match result {
            Some(Ok(document)) => match opened_against.install(
                document,
                &mut self.editor,
                &mut self.interactions,
                self.document_validator.as_ref(),
            ) {
                Ok(mut report) => {
                    self.cancel_interaction(cx);
                    self.reset_document_runtime();
                    self.message = "Document opened".into();
                    // Reopening an identical graph still replaces its runtime and history.
                    report.document_changed = true;
                    self.apply_interaction_report(report, cx);
                    return;
                }
                Err(error) => self.message = error,
            },
            Some(Err(error)) => self.message = format!("Open failed: {error}"),
            None => {}
        }
        cx.notify();
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

    fn document_with_node(id: &str) -> model::FlowDocument {
        let mut document = model::FlowDocument::default();
        document
            .graph
            .add_node(Node::new(id, id, model::Point::ZERO))
            .unwrap();
        document
    }

    #[test]
    fn pending_open_keeps_a_new_edit_and_is_stale_after_undo() {
        let before = document_with_node("before");
        let mut editor = Editor::from_document(before.clone()).unwrap();
        let version = OpenDocumentVersion::new(&editor);
        let mut host = PluginHost::with_defaults();
        editor
            .transaction("Add during open", |graph| {
                graph.add_node(Node::new("new", "New", model::Point::ZERO))
            })
            .unwrap();
        let edited = editor.document();
        assert!(
            version
                .install(document_with_node("loaded"), &mut editor, &mut host, None)
                .is_err()
        );
        assert_eq!(editor.document(), edited);
        assert!(editor.undo());
        assert_eq!(editor.document(), before);
        assert!(
            version
                .install(document_with_node("loaded"), &mut editor, &mut host, None)
                .is_err()
        );
        assert!(editor.can_redo());
    }

    #[test]
    fn pending_open_keeps_the_viewport_and_an_active_capture() {
        let mut editor = Editor::from_document(document_with_node("before")).unwrap();
        let version = OpenDocumentVersion::new(&editor);
        let mut host = PluginHost::with_defaults();
        editor.viewport.pan.x = 120.;
        let before = editor.document();
        assert!(
            version
                .install(document_with_node("loaded"), &mut editor, &mut host, None)
                .is_err()
        );
        let version = OpenDocumentVersion::new(&editor);
        host.execute(&mut editor, CanvasCommand::CapturePointer { pointer_id: 1 })
            .unwrap();
        assert!(
            version
                .install(document_with_node("loaded"), &mut editor, &mut host, None)
                .is_err()
        );
        assert!(host.has_capture());
        assert_eq!(editor.document(), before);
    }

    #[test]
    fn matching_open_preserves_history_on_rejection_and_loads_on_success() {
        let mut editor = Editor::from_document(document_with_node("before")).unwrap();
        editor
            .transaction("Add before open", |graph| {
                graph.add_node(Node::new("existing", "Existing", model::Point::ZERO))
            })
            .unwrap();
        editor.selection.select_node("existing", false);
        let document = editor.document();
        let selection = editor.selection.clone();
        let mut incoming = document_with_node("loaded");
        incoming.viewport.pan.x = 80.;
        let mut host = PluginHost::with_defaults();
        let version = OpenDocumentVersion::new(&editor);
        let reject: FlowDocumentValidator = Arc::new(|_| Err("Rejected".into()));
        assert!(matches!(
            version.install(incoming.clone(), &mut editor, &mut host, Some(&reject)),
            Err(error) if error == "Rejected"
        ));
        host.set_editable(false, &mut editor);
        assert!(
            version
                .install(incoming.clone(), &mut editor, &mut host, None)
                .is_err()
        );
        assert_eq!(editor.document(), document);
        assert_eq!(editor.selection, selection);
        assert!(editor.can_undo());
        host.set_editable(true, &mut editor);
        let report = version
            .install(incoming.clone(), &mut editor, &mut host, None)
            .unwrap();
        assert!(report.document_changed && report.selection_changed && report.viewport_changed);
        assert_eq!(editor.document(), incoming);
        assert!(editor.selection.is_empty());
        assert!(!editor.can_undo());
    }

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
