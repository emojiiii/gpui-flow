//! Opt-in real-GPUI-frame batching, separate from immediate editor transactions.
use super::{SceneView, SceneViewEvent};
use gpui::{Context, Window};
use gpui_flow_editor::SceneEditor;
use gpui_flow_layout::{FlowFrameQueue, FlowReport};
use gpui_flow_scene::{Scene, SceneError};

/// One completed scheduled callback, including rejected batches. A successful
/// report is a document/layout commit receipt, not a paint/GPU fence.
#[derive(Debug, Clone)]
pub struct FlowFrameEvent {
    pub generation: u64,
    pub edits: usize,
    pub revision_before: u64,
    pub revision_after: u64,
    pub result: Result<Option<FlowReport>, String>,
}
#[derive(Default)]
pub(crate) struct FrameLayoutState {
    queue: Option<FlowFrameQueue>,
    generation: u64,
    scheduled_window: Option<u64>,
}
impl FrameLayoutState {
    fn advance(&mut self) -> Result<(), String> {
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or("flow frame generation exhausted")?;
        self.scheduled_window = None;
        Ok(())
    }
    fn replace(&mut self, queue: Option<FlowFrameQueue>) -> Result<(), String> {
        self.advance()?;
        self.queue = queue;
        Ok(())
    }
    fn cancel(&mut self) -> Result<bool, String> {
        self.advance()?;
        Ok(self.queue.as_mut().is_some_and(FlowFrameQueue::cancel))
    }
    fn pending(&self) -> usize {
        self.queue.as_ref().map_or(0, FlowFrameQueue::pending_edits)
    }
    fn schedule(&mut self, window: u64) -> Result<Option<u64>, String> {
        if self.scheduled_window.is_some_and(|w| w != window) {
            return Err("queued layout belongs to another native window".into());
        }
        if self.scheduled_window.is_some() || self.pending() == 0 {
            return Ok(None);
        }
        self.advance()?;
        self.scheduled_window = Some(window);
        Ok(Some(self.generation))
    }
    fn flush(
        &mut self,
        token: u64,
        window: u64,
        current_window: Option<u64>,
        editor: &mut SceneEditor,
    ) -> Option<FlowFrameEvent> {
        if self.generation != token || self.scheduled_window != Some(window) {
            return None;
        }
        self.scheduled_window = None;
        let edits = self.pending();
        let before = editor.engine.revision();
        let result = if current_window.is_some_and(|current| current != window) {
            Err("view moved to another native window before queued layout commit".into())
        } else {
            self.queue
                .as_mut()?
                .flush(editor)
                .map_err(|e| e.to_string())
        };
        Some(FlowFrameEvent {
            generation: token,
            edits,
            revision_before: before,
            revision_after: editor.engine.revision(),
            result,
        })
    }
}
impl SceneView {
    /// Replace or remove the opt-in frame queue, discarding its staged edits.
    /// Existing immediate editor APIs remain immediate. Old posted callbacks are
    /// generation-guarded and can never flush a replacement queue.
    pub fn set_flow_frame_queue(
        &mut self,
        queue: Option<FlowFrameQueue>,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        self.frame_layout.replace(queue)?;
        cx.notify();
        Ok(())
    }
    pub fn pending_flow_edits(&self) -> usize {
        self.frame_layout.pending()
    }
    /// Stage a trusted host edit and schedule exactly one callback after the
    /// current GPUI frame. More edits before that callback join the same atomic
    /// transaction/Undo. Failed enqueue preserves earlier staged edits.
    /// This is a programmatic API, not a bypassable UI permission boundary.
    pub fn enqueue_flow_edit(
        &mut self,
        edit: impl FnOnce(&mut Scene) -> Result<(), SceneError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let window_id = window.window_handle().window_id().as_u64();
        if self
            .frame_layout
            .scheduled_window
            .is_some_and(|w| w != window_id)
        {
            return Err("queued layout belongs to another native window".into());
        }
        self.frame_layout
            .queue
            .as_mut()
            .ok_or("no Flow frame queue installed")?
            .enqueue(&self.editor, edit)
            .map_err(|e| e.to_string())?;
        self.retry_flow_frame(window, cx)
    }
    /// Retry a failed pending batch after backend recovery. Scene/provider epoch
    /// mismatches still reject; cancel and restage those edits explicitly.
    pub fn retry_flow_frame(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let id = window.window_handle().window_id().as_u64();
        let Some(token) = self.frame_layout.schedule(id)? else {
            return Ok(());
        };
        let entity = cx.entity().downgrade();
        window.on_next_frame(move |_, cx| {
            let _ = entity.update(cx, |view, cx| {
                let Some(event) =
                    view.frame_layout
                        .flush(token, id, view.interaction_window, &mut view.editor)
                else {
                    return;
                };
                if event.revision_after != event.revision_before {
                    cx.emit(SceneViewEvent::Changed);
                }
                cx.emit(SceneViewEvent::FlowFrame(event));
                cx.notify();
            });
        });
        Ok(())
    }
    pub fn cancel_flow_frame(&mut self, cx: &mut Context<Self>) -> Result<bool, String> {
        let cancelled = self.frame_layout.cancel()?;
        cx.notify();
        Ok(cancelled)
    }
    /// Force a full layout next time, e.g. after invalidating the font backend.
    pub fn invalidate_flow_frame_layout(&mut self) {
        if let Some(q) = &mut self.frame_layout.queue {
            q.invalidate();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_flow_layout::FlowLayout;
    use gpui_flow_scene::{Affine, DisplayObject, display};
    fn setup() -> (SceneEditor, FrameLayoutState) {
        let mut scene = Scene::new();
        scene
            .insert(
                DisplayObject::new("rect", display::Rect::new(10., 10.)),
                None,
                None,
            )
            .unwrap();
        let editor = SceneEditor::new(scene).unwrap();
        let mut state = FrameLayoutState::default();
        state
            .replace(Some(FlowFrameQueue::new(FlowLayout::default())))
            .unwrap();
        (editor, state)
    }
    fn edit(s: &mut FrameLayoutState, e: &SceneEditor, x: f32) {
        s.queue
            .as_mut()
            .unwrap()
            .enqueue(e, |scene| {
                scene.update_object(&"rect".into(), |o| o.transform = Affine::translation(x, 0.))
            })
            .unwrap();
    }
    #[test]
    fn one_scheduled_token_coalesces_multiple_edits_into_one_undo() {
        let (mut e, mut s) = setup();
        let original = e.scene().clone();
        edit(&mut s, &e, 10.);
        let token = s.schedule(5).unwrap().unwrap();
        for x in 11..31 {
            edit(&mut s, &e, x as f32);
            assert_eq!(s.schedule(5).unwrap(), None);
        }
        assert_eq!(e.scene(), &original);
        assert_eq!(e.engine.revision(), 0);
        assert_eq!(s.pending(), 21);
        let receipt = s.flush(token, 5, Some(5), &mut e).unwrap();
        assert_eq!(receipt.edits, 21);
        assert_eq!(receipt.revision_after, 1);
        assert!(receipt.result.is_ok());
        assert_eq!(s.pending(), 0);
        assert_eq!(
            e.scene().get(&"rect".into()).unwrap().object.transform.tx,
            30.
        );
        assert!(s.flush(token, 5, Some(5), &mut e).is_none());
        assert!(e.undo());
        assert_eq!(e.scene(), &original);
        assert!(!e.undo());
    }
    #[test]
    fn cancel_and_replace_cannot_retarget_an_old_callback() {
        let (mut e, mut s) = setup();
        edit(&mut s, &e, 10.);
        let old = s.schedule(5).unwrap().unwrap();
        assert!(s.cancel().unwrap());
        edit(&mut s, &e, 20.);
        let current = s.schedule(5).unwrap().unwrap();
        assert!(s.flush(old, 5, Some(5), &mut e).is_none());
        assert_eq!(s.pending(), 1);
        assert!(s.flush(current, 5, Some(5), &mut e).unwrap().result.is_ok());
        edit(&mut s, &e, 30.);
        let old = s.schedule(5).unwrap().unwrap();
        s.replace(Some(FlowFrameQueue::new(FlowLayout::default())))
            .unwrap();
        edit(&mut s, &e, 40.);
        let current = s.schedule(5).unwrap().unwrap();
        assert!(s.flush(old, 5, Some(5), &mut e).is_none());
        assert!(s.flush(current, 5, Some(5), &mut e).unwrap().result.is_ok());
        assert_eq!(
            e.scene().get(&"rect".into()).unwrap().object.transform.tx,
            40.
        );
    }
    #[test]
    fn stale_commit_rejects_without_overwriting_external_edit_or_history() {
        let (mut e, mut s) = setup();
        edit(&mut s, &e, 10.);
        let token = s.schedule(5).unwrap().unwrap();
        e.transaction(|scene| {
            scene.update_object(&"rect".into(), |o| {
                o.transform = Affine::translation(50., 0.)
            })
        })
        .unwrap();
        let before = e.scene().clone();
        let receipt = s.flush(token, 5, Some(5), &mut e).unwrap();
        assert!(receipt.result.is_err());
        assert_eq!(receipt.revision_before, receipt.revision_after);
        assert_eq!(e.scene(), &before);
        assert_eq!(s.pending(), 1);
        let retry = s.schedule(5).unwrap().unwrap();
        assert!(s.flush(retry, 5, Some(5), &mut e).unwrap().result.is_err());
        assert!(s.cancel().unwrap());
        assert!(e.undo());
        assert!(!e.undo());
    }
    #[test]
    fn window_migration_rejects_then_explicit_retry_uses_current_window() {
        let (mut e, mut s) = setup();
        edit(&mut s, &e, 10.);
        let token = s.schedule(5).unwrap().unwrap();
        assert!(s.schedule(6).is_err());
        assert!(s.flush(token, 5, Some(6), &mut e).unwrap().result.is_err());
        assert_eq!(e.engine.revision(), 0);
        assert_eq!(s.pending(), 1);
        let retry = s.schedule(6).unwrap().unwrap();
        assert!(s.flush(retry, 6, Some(6), &mut e).unwrap().result.is_ok());
        assert_eq!(e.engine.revision(), 1);
    }
    #[test]
    fn empty_callback_and_generation_exhaustion_do_not_publish() {
        let (e, mut s) = setup();
        assert_eq!(s.schedule(5).unwrap(), None);
        let generation = s.generation;
        s.generation = u64::MAX;
        assert!(s.replace(None).is_err());
        assert!(s.queue.is_some());
        assert_eq!(s.pending(), 0);
        assert_eq!(e.engine.revision(), 0);
        s.generation = generation;
    }
}
