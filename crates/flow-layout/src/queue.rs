//! Explicit host-frame staging. This queue does not install a frame callback.
use crate::{
    FlowError, FlowLayout, FlowReport, MAX_FLOW_LAYOUT_PASSES, MeasuredFlowLayout, TextMeasurer,
};
use gpui_flow_editor::SceneEditor;
use gpui_flow_engine::LayoutProviderId;
use gpui_flow_scene::{Scene, SceneError};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::Arc,
};

pub const MAX_FLOW_FRAME_EDITS: usize = 256;
#[derive(Clone, Copy, PartialEq, Eq)]
struct Stamp {
    engine: u64,
    epoch: u64,
    revision: u64,
    provider: Option<LayoutProviderId>,
}
impl Stamp {
    fn of(editor: &SceneEditor) -> Self {
        Self {
            engine: editor.engine.engine_id(),
            epoch: editor.engine.document_epoch(),
            revision: editor.engine.revision(),
            provider: editor.engine.layout_provider(),
        }
    }
}
struct Pending {
    stamp: Stamp,
    scene: Scene,
    edits: usize,
}
/// A bounded coalescing queue for hosts which explicitly flush on their real
/// next-frame callback. Enqueue keeps the committed scene/history untouched.
/// A successful flush commits all input edits and layout as one Undo step.
/// Neither constructing nor enqueueing this value schedules any work by itself.
pub struct FlowFrameQueue {
    layout: FlowLayout,
    measurer: Option<Arc<dyn TextMeasurer>>,
    pending: Option<Pending>,
    settled: Option<Stamp>,
}
impl FlowFrameQueue {
    pub fn new(layout: FlowLayout) -> Self {
        Self {
            layout,
            measurer: None,
            pending: None,
            settled: None,
        }
    }
    pub fn measured(layout: MeasuredFlowLayout) -> Self {
        Self {
            layout: layout.layout,
            measurer: Some(layout.measurer),
            pending: None,
            settled: None,
        }
    }
    pub fn is_pending(&self) -> bool {
        self.pending.is_some()
    }
    pub fn pending_edits(&self) -> usize {
        self.pending.as_ref().map_or(0, |p| p.edits)
    }
    /// Returns true only for the empty-to-pending transition. The host should
    /// request one real next-frame callback on true, then flush in that callback.
    /// Failed edits preserve the prior pending batch and committed document.
    pub fn enqueue(
        &mut self,
        editor: &SceneEditor,
        edit: impl FnOnce(&mut Scene) -> Result<(), SceneError>,
    ) -> Result<bool, FlowError> {
        let stamp = Stamp::of(editor);
        if self.pending.as_ref().is_some_and(|p| p.stamp != stamp) {
            return Err(FlowError::Scheduling("queued layout is stale after another commit/provider change; cancel and enqueue again".into()));
        }
        if self.pending_edits() >= MAX_FLOW_FRAME_EDITS {
            return Err(FlowError::Scheduling(format!(
                "flow frame edit budget exceeded ({MAX_FLOW_FRAME_EDITS})"
            )));
        }
        let mut scene = self
            .pending
            .as_ref()
            .map_or_else(|| editor.scene().clone(), |p| p.scene.clone());
        catch_unwind(AssertUnwindSafe(|| edit(&mut scene))).map_err(|_| {
            FlowError::Scheduling("queued scene edit panicked; candidate discarded".into())
        })??;
        scene.validate()?;
        let first = self.pending.is_none();
        self.pending = Some(Pending {
            stamp,
            scene,
            edits: self.pending_edits() + 1,
        });
        Ok(first)
    }
    /// Cancel staged input without a document/history mutation. Also forgets the
    /// incremental baseline, so the next flush performs a complete arrangement.
    pub fn cancel(&mut self) -> bool {
        self.settled = None;
        self.pending.take().is_some()
    }
    /// Force full remeasurement on the next batch, e.g. after a host font change.
    /// Backend invalidation remains the host's responsibility.
    pub fn invalidate(&mut self) {
        self.settled = None;
    }
    /// Explicit atomic flush. Failed flushes retain pending input for inspection,
    /// retry after backend recovery, or cancel. A no-op flush makes no history.
    pub fn flush(&mut self, editor: &mut SceneEditor) -> Result<Option<FlowReport>, FlowError> {
        let Some(pending) = &self.pending else {
            return Ok(None);
        };
        if pending.stamp != Stamp::of(editor) {
            return Err(FlowError::Scheduling("queued layout is stale after another commit/provider change; cancel and enqueue again".into()));
        }
        let candidate = editor.engine.prepare_scene(pending.scene.clone())?;
        let selected = (self.settled == Some(pending.stamp))
            .then(|| self.layout.dirty_selection(editor.scene(), &candidate));
        let (next, report) = self.layout.arranged_selected(
            &candidate,
            MAX_FLOW_LAYOUT_PASSES,
            self.measurer.as_deref(),
            selected.as_ref(),
        )?;
        let expected = next.clone();
        editor.transaction_validated(
            |scene| {
                *scene = next;
                Ok(())
            },
            |final_scene| {
                if final_scene == &expected {
                    Ok(())
                } else {
                    Err(SceneError::InvalidGeometry(
                        "queued Flow output conflicts with another derived geometry owner".into(),
                    ))
                }
            },
        )?;
        self.pending = None;
        self.settled = Some(Stamp::of(editor));
        Ok(Some(report))
    }
}
