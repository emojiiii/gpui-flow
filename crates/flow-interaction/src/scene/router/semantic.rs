//! Cancelable non-pointer requests. The native adapter owns the default, after
//! revalidating its displayed-frame token; this router never invents a pointer.
use super::*;
use gpui_flow_scene::{SemanticAction, SemanticSource};

/// One-use continuation; focus notifications share the request's original
/// 256-action budget instead of starting a second queue operation.
pub struct SemanticDefault {
    pub(super) target: ElementHandle,
    pub(super) attachment: u64,
    pub(super) document: u64,
    pub(super) budget: usize,
}
pub struct SemanticDispatch {
    pub report: RoutedReport,
    pub default: Option<SemanticDefault>,
}
impl SceneEventRouter {
    /// Notify the ordinary capture/target/bubble listeners and drain their
    /// bounded action queue before an adapter considers a semantic default.
    /// Callers must still check their displayed target and current policy after
    /// this returns. A prevented request still drains cleanup/listener actions.
    pub fn dispatch_semantic(
        &mut self,
        editor: &mut SceneEditor,
        host: &mut ScenePluginHost,
        target: ElementHandle,
        action: SemanticAction,
        source: SemanticSource,
    ) -> Result<SemanticDispatch, EngineError> {
        Self::validate_focus_target(editor, Some(target))?;
        let epoch = Self::epoch(&editor.engine, Some(target));
        let document = editor.engine.document_epoch();
        self.event_time = None;
        self.actions.begin_operation();
        editor.reconcile_external();
        let mut out = RoutedReport::default();
        self.retire_invalid(editor, host, &mut out);
        self.sequence = self.sequence.saturating_add(1);
        let mut event = SceneEvent::new(
            EventKind::SemanticAction,
            editor.engine.instance(target)?.id,
        );
        event.sequence = self.sequence;
        event.input = Some(EventInput::Semantic { action, source });
        // No world/local position, pressure, pointer identity or timestamp.
        let result = editor.engine.dispatch(event)?;
        out.default.default_prevented = result.event.default_prevented();
        out.events.push(result.event);
        let mut budget = 256;
        self.finish_dispatch(editor, host, &mut out, &mut budget, true);
        if editor.engine.document_epoch() != document
            || !Self::current(&editor.engine, target, epoch)
        {
            out.default.default_prevented = true;
            out.errors
                .push("semantic target invalidated during notification".into());
        }
        let allowed = budget > 0
            && !out.default.default_prevented
            && out.errors.is_empty()
            && out.default.errors.is_empty();
        Ok(SemanticDispatch {
            report: out,
            default: allowed.then_some(SemanticDefault {
                target,
                attachment: epoch,
                document,
                budget,
            }),
        })
    }
}

impl SceneEventRouter {
    pub fn set_focus_after_semantic(
        &mut self,
        editor: &mut SceneEditor,
        host: &mut ScenePluginHost,
        permit: SemanticDefault,
    ) -> Result<RoutedReport, EngineError> {
        if editor.engine.document_epoch() != permit.document
            || !Self::current(&editor.engine, permit.target, permit.attachment)
        {
            return Err(EngineError::InvalidHandle);
        }
        let mut out = RoutedReport::default();
        let mut budget = permit.budget;
        self.event_time = None;
        self.transfer_focus(
            editor,
            host,
            Some(permit.target),
            true,
            &mut out,
            &mut budget,
        );
        self.finish_dispatch(editor, host, &mut out, &mut budget, true);
        Ok(out)
    }
}
