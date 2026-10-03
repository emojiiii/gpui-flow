use super::*;
use gpui_flow_engine::EngineError;

impl SceneEventRouter {
    /// The live logical keyboard target. Selection and pointer hover are separate.
    pub fn focused(&self, engine: &SceneEngine) -> Option<ElementHandle> {
        self.focus
            .filter(|handle| Self::current(engine, *handle, self.focus_epoch))
    }
    pub(super) fn validate_focus_target(
        editor: &SceneEditor,
        target: Option<ElementHandle>,
    ) -> Result<(), EngineError> {
        if let Some(target) = target {
            editor.engine.object(target)?;
            if editor.engine.instance(target)?.state != InstanceState::Attached {
                return Err(EngineError::DetachedHandle);
            }
        }
        Ok(())
    }
    /// Move focus without selecting. Invalid targets fail before changing any
    /// input state. A transfer cancels the old pointer gesture and held keys,
    /// retaining hover. Non-cancelable Blur runs before Focus; queued actions
    /// between them may invalidate or redirect the requested target.
    pub fn set_focus(
        &mut self,
        editor: &mut SceneEditor,
        host: &mut ScenePluginHost,
        target: Option<ElementHandle>,
    ) -> Result<RoutedReport, EngineError> {
        Self::validate_focus_target(editor, target)?;
        self.event_time = None;
        self.actions.begin_operation();
        let mut out = RoutedReport::default();
        if target.is_some() && target == self.focused(&editor.engine) {
            return Ok(out);
        }
        editor.reconcile_external();
        let mut budget = 256;
        self.transfer_focus(editor, host, target, true, &mut out, &mut budget);
        self.finish_dispatch(editor, host, &mut out, &mut budget, target.is_some());
        Ok(out)
    }
    pub fn clear_focus(
        &mut self,
        editor: &mut SceneEditor,
        host: &mut ScenePluginHost,
    ) -> RoutedReport {
        self.set_focus(editor, host, None)
            .expect("clearing focus has no target validation")
    }
    pub(super) fn transfer_focus(
        &mut self,
        editor: &mut SceneEditor,
        host: &mut ScenePluginHost,
        mut target: Option<ElementHandle>,
        mut cancel_pointer: bool,
        out: &mut RoutedReport,
        budget: &mut usize,
    ) {
        out.focus_requested |= cancel_pointer;
        let mut blurred = None;
        let mut allow_redirect = target.is_some();
        loop {
            allow_redirect &= target.is_some();
            if let Err(error) = Self::validate_focus_target(editor, target) {
                out.errors.push(error.to_string());
                break;
            }
            let old = self.focused(&editor.engine);
            let epoch = Self::epoch(&editor.engine, target);
            if cancel_pointer && (target != old || target.is_none()) {
                self.cancel_contacts(editor, host, None, out);
                self.clear_keys();
            }
            if old != target {
                let related = target.and_then(|h| Self::id(editor, h));
                if old.is_some() {
                    blurred = old.and_then(|h| Self::id(editor, h));
                }
                self.clear_keys();
                self.focus = None;
                self.emit_focus(editor, old, related, EventKind::Blur, out);
                if let Some(redirect) =
                    self.drain_actions(editor, host, out, budget, allow_redirect)
                {
                    target = redirect;
                    cancel_pointer = true;
                    out.focus_requested = true;
                    continue;
                }
                if target.is_some_and(|h| !Self::current(&editor.engine, h, epoch)) {
                    out.errors
                        .push("focus target invalidated during Blur notification".into());
                    break;
                }
                self.focus = target;
                self.focus_epoch = epoch;
                self.emit_focus(editor, target, blurred.take(), EventKind::Focus, out);
            } else {
                self.focus = target;
                self.focus_epoch = Self::epoch(&editor.engine, target);
            }
            if let Some(redirect) = self.drain_actions(editor, host, out, budget, allow_redirect) {
                target = redirect;
                cancel_pointer = true;
                out.focus_requested = true;
                continue;
            }
            break;
        }
        self.retire_invalid(editor, host, out);
    }
    fn emit_focus(
        &mut self,
        editor: &mut SceneEditor,
        target: Option<ElementHandle>,
        related: Option<DisplayId>,
        kind: EventKind,
        out: &mut RoutedReport,
    ) {
        let Some(id) = target.and_then(|h| Self::id(editor, h)) else {
            return;
        };
        self.sequence = self.sequence.saturating_add(1);
        let mut event = SceneEvent::new(kind, id);
        event.related_target = related;
        event.cancelable = false;
        event.sequence = self.sequence;
        event.timestamp_millis = self.event_time;
        match editor.engine.dispatch(event) {
            Ok(result) => out.events.push(result.event),
            Err(error) => out.errors.push(error.to_string()),
        }
    }
}
