//! Explicit navigation time and coalesced native frame callbacks.
use super::*;
use gpui_flow_interaction::{DispatchReport, scene::RoutedReport};
use std::time::Duration;

#[derive(Default)]
pub(super) struct NavigationFrames {
    generation: u64,
    pending: Option<(u64, u64)>,
}
impl NavigationFrames {
    fn schedule(&mut self, window: u64) -> Result<Option<u64>, &'static str> {
        if self.pending.is_some_and(|(id, _)| id == window) {
            return Ok(None);
        }
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or("navigation frame generation exhausted")?;
        self.pending = Some((window, self.generation));
        Ok(Some(self.generation))
    }
    fn take(&mut self, window: u64, generation: u64) -> bool {
        if self.pending == Some((window, generation)) {
            self.pending = None;
            true
        } else {
            false
        }
    }
    pub(super) fn cancel(&mut self) {
        // The next request always increments the generation; a cancelled token
        // cannot acquire a later request, including on the same window.
        self.pending = None;
    }
}
fn has_output(report: &DispatchReport) -> bool {
    report.document_changed
        || report.selection_changed
        || report.viewport_changed
        || !report.errors.is_empty()
        || !report.effects.is_empty()
}
impl SceneView {
    /// Event time is observed before dispatch without advancing a viewport that
    /// the user has not seen. Pointer targets therefore stay tied to the pixels
    /// used by the normal picker, including layered surfaces.
    pub(super) fn synchronize_navigation_input(&mut self, cx: &mut Context<Self>) -> bool {
        let report = self
            .interactions
            .synchronize_navigation_time(&mut self.editor, self.state_clock.elapsed());
        let valid = report.errors.is_empty();
        if has_output(&report) {
            self.apply_input_report(report, cx);
        }
        valid
    }
    pub(super) fn prepare_navigation_frame(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.interactions.needs_navigation_frame() {
            self.navigation_frames.cancel();
            return;
        }
        if !window.is_visible() || !window.is_window_active() || cx.reduce_motion() {
            self.navigation_frames.cancel();
            // An inactive/reduced-motion surface must not accumulate a delayed
            // tail that jumps when it resumes. Existing cancellation owns cleanup.
            self.cancel_input(cx);
            return;
        }
        let id = window.window_handle().window_id().as_u64();
        let generation = match self.navigation_frames.schedule(id) {
            Ok(Some(generation)) => generation,
            Ok(None) => return,
            Err(error) => {
                self.interaction_error = Some(error.into());
                self.cancel_input(cx);
                return;
            }
        };
        let entity = cx.entity().downgrade();
        window.on_next_frame(move |window, cx| {
            let _ = entity.update(cx, |this, cx| {
                if !this.navigation_frames.take(id, generation) {
                    return;
                }
                if !this.interactions.needs_navigation_frame() {
                    return;
                }
                if !window.is_visible() || !window.is_window_active() || cx.reduce_motion() {
                    this.cancel_input(cx);
                    return;
                }
                this.advance_navigation_frame(this.state_clock.elapsed(), cx);
                this.prepare_navigation_frame(window, cx);
            });
        });
    }
    fn advance_navigation_frame(&mut self, now: Duration, cx: &mut Context<Self>) {
        let report = self.interactions.advance_navigation(&mut self.editor, now);
        self.emit_input_trace(
            SceneInputOperation::NavigationFrame { now },
            &RoutedReport {
                default: report.clone(),
                ..Default::default()
            },
            cx,
        );
        if has_output(&report) {
            self.apply_input_report(report, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[::core::prelude::v1::test]
    fn navigation_frame_coalesces_and_rejects_old_window_or_cancelled_tokens() {
        let mut state = NavigationFrames::default();
        let first = state.schedule(1).unwrap().unwrap();
        assert_eq!(state.schedule(1).unwrap(), None);
        let moved = state.schedule(2).unwrap().unwrap();
        assert!(!state.take(1, first));
        assert!(state.take(2, moved));
        let cancelled = state.schedule(2).unwrap().unwrap();
        state.cancel();
        let replacement = state.schedule(2).unwrap().unwrap();
        assert!(!state.take(2, cancelled));
        assert!(state.take(2, replacement));
        assert!(!state.take(2, replacement));
    }
    #[::core::prelude::v1::test]
    fn navigation_generation_exhaustion_never_reuses_an_old_callback() {
        let mut state = NavigationFrames {
            generation: u64::MAX,
            pending: None,
        };
        assert!(state.schedule(1).is_err());
        assert_eq!(state.pending, None);
    }
}
