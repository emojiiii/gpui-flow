//! Flow-local opt-in identity and execution accounting. Unscoped hosts retain
//! upstream behavior. These counters describe commands, never timing or estimates.
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::Result;

use super::frame::FrameState;

/// Opaque, process-unique identity for a producer's persistent layer keys.
/// Keep one token across frames. A different token invalidates cached tiles.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FlowLayerNamespace(u64, bool);

impl FlowLayerNamespace {
    pub fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(
            NEXT.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
                .expect("Flow layer namespace identities exhausted"),
            false,
        )
    }

    /// A producer whose single-scene layer contents use the window's device
    /// coordinates. Path tiles retain the window projection for identical
    /// multisample coverage. Generic scrolling/content-space layers must use
    /// [`Self::new`] instead.
    pub fn new_window_space() -> Self {
        Self(Self::new().0, true)
    }
}

impl Default for FlowLayerNamespace {
    fn default() -> Self {
        Self::new()
    }
}

thread_local! {
    static NAMESPACE: Cell<Option<FlowLayerNamespace>> = const { Cell::new(None) };
    static CAPTURE: RefCell<Option<Rc<RefCell<Capture>>>> = const { RefCell::new(None) };
}

/// Scope a synchronous renderer draw. Nested calls and unwinding restore the
/// previous scope. This does not propagate to another thread or an async task.
pub fn with_flow_layer_namespace<R>(namespace: FlowLayerNamespace, draw: impl FnOnce() -> R) -> R {
    struct Restore(Option<FlowLayerNamespace>);
    impl Drop for Restore {
        fn drop(&mut self) {
            NAMESPACE.set(self.0);
        }
    }
    let _restore = Restore(NAMESPACE.replace(Some(namespace)));
    draw()
}

/// Captures exactly one successful synchronous renderer draw. Zero/multiple
/// draws, namespace mismatch or any failed draw yield None. Nested collectors
/// restore on unwind; their draws also count toward an enclosing collector.
pub fn with_flow_layer_namespace_receipt<R>(
    namespace: FlowLayerNamespace,
    draw: impl FnOnce() -> R,
) -> (R, Option<FlowExecutionReceipt>) {
    struct Restore(Option<Rc<RefCell<Capture>>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            CAPTURE.with(|slot| {
                *slot.borrow_mut() = self.0.take();
            });
        }
    }
    let capture = Rc::new(RefCell::new(Capture {
        namespace,
        attempts: 0,
        rejected: false,
        receipt: None,
        parent: CAPTURE.with(|slot| slot.borrow().clone()),
    }));
    let _restore = Restore(CAPTURE.with(|slot| slot.replace(Some(capture.clone()))));
    let result = with_flow_layer_namespace(namespace, draw);
    let capture = capture.borrow();
    let receipt = if capture.attempts == 1 && !capture.rejected {
        capture.receipt
    } else {
        None
    };
    (result, receipt)
}

struct Capture {
    namespace: FlowLayerNamespace,
    attempts: u8,
    rejected: bool,
    receipt: Option<FlowExecutionReceipt>,
    parent: Option<Rc<RefCell<Capture>>>,
}

fn visit_captures(mut visit: impl FnMut(&mut Capture)) {
    let mut current = CAPTURE.with(|slot| slot.borrow().clone());
    while let Some(capture) = current {
        let mut capture = capture.borrow_mut();
        visit(&mut capture);
        current = capture.parent.clone();
    }
}

fn publish(receipt: FlowExecutionReceipt) {
    visit_captures(|capture| {
        if receipt.namespace == Some(capture.namespace)
            && capture.attempts == 1
            && !capture.rejected
        {
            capture.receipt = Some(receipt);
        } else {
            capture.rejected = true;
            capture.receipt = None;
        }
    });
}

/// The entry of an actual renderer draw, before surface acquisition can fail.
pub(crate) fn begin_draw(state: &mut FrameState) {
    visit_captures(|capture| {
        capture.attempts = capture.attempts.saturating_add(1).min(2);
        if NAMESPACE.get() != Some(capture.namespace) {
            capture.rejected = true;
        }
        capture.receipt = None;
    });
    begin_frame(state);
}

/// Successful native recording/submission. Upload bytes include alignment and
/// minimum binding padding actually requested from Queue::write_buffer_with;
/// globals, atlas textures and readback copies are deliberately excluded.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FlowExecutionReceipt {
    /// Successful receipts always carry Some(the submitting producer identity).
    pub namespace: Option<FlowLayerNamespace>,
    /// Actual render passes whose target is a layer tile (including resumed
    /// passes). Path intermediate passes are not tile-target passes.
    pub tile_raster_passes: u64,
    pub instance_upload_calls: u64,
    pub instance_upload_bytes: u64,
    pub encoded: bool,
    pub submitted: bool,
    /// True only after a completed, checked pixel readback.
    pub readback_completed: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FlowFailureStage {
    Encoding,
    Submission,
    Readback,
    UnsupportedTransport,
}

#[derive(Default)]
pub(crate) struct FlowFrameState {
    namespace: Option<FlowLayerNamespace>,
    pending: Cell<FlowExecutionReceipt>,
    pub(crate) receipt: Option<FlowExecutionReceipt>,
    pub(crate) failure: Cell<Option<FlowFailureStage>>,
}

impl FlowFrameState {
    pub(crate) fn enabled(&self) -> bool {
        self.namespace.is_some() && !cfg!(target_family = "wasm")
    }
    pub(crate) fn window_space(&self) -> bool {
        self.enabled() && self.namespace.is_some_and(|namespace| namespace.1)
    }
    pub(crate) fn upload(&self, bytes: u64) {
        if self.enabled() {
            let mut counts = self.pending.get();
            counts.instance_upload_calls += 1;
            counts.instance_upload_bytes += bytes;
            self.pending.set(counts);
        }
    }
    pub(crate) fn tile_pass(&self) {
        if self.enabled() {
            let mut counts = self.pending.get();
            counts.tile_raster_passes += 1;
            self.pending.set(counts);
        }
    }
    pub(crate) fn submitted(&mut self) {
        if self.enabled() {
            self.receipt = Some(FlowExecutionReceipt {
                namespace: self.namespace,
                encoded: true,
                submitted: true,
                ..self.pending.get()
            });
            self.failure.set(None);
            publish(self.receipt.expect("receipt just set"));
        }
    }
    /// Publish only after the actual queue accepts a validation-clean submit.
    pub(crate) fn submit(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        commands: wgpu::CommandBuffer,
    ) -> Result<()> {
        let errors = GpuErrors::new(device, self.enabled());
        if self.enabled() {
            self.failure.set(Some(FlowFailureStage::Submission));
        }
        queue.submit(std::iter::once(commands));
        if let Err(error) = errors.check() {
            self.fail(FlowFailureStage::Submission);
            return Err(error);
        }
        self.submitted();
        Ok(())
    }
    #[cfg(all(feature = "flow-headless", not(target_family = "wasm")))]
    pub(crate) fn readback_completed(&mut self) {
        if let Some(receipt) = self.receipt.as_mut() {
            receipt.readback_completed = true;
            publish(*receipt);
        }
    }
    pub(crate) fn fail(&mut self, stage: FlowFailureStage) {
        self.pending.set(FlowExecutionReceipt::default());
        self.receipt = None;
        self.failure.set(Some(stage));
        visit_captures(|capture| {
            capture.rejected = true;
            capture.receipt = None;
        });
    }
}

/// Called at draw entry (even a failed surface acquisition clears old receipts)
/// and at the surfaceless recorder entry. Host None -> None changes no caches.
pub(crate) fn begin_frame(state: &mut FrameState) {
    let namespace = NAMESPACE.get();
    if state.flow.namespace != namespace {
        state.layers.clear();
        state.bind_groups = Default::default();
        state.flow.namespace = namespace;
    }
    state.flow.pending.set(FlowExecutionReceipt::default());
    state.flow.receipt = None;
    state.flow.failure.set(namespace.map(|_| {
        if cfg!(target_family = "wasm") {
            FlowFailureStage::UnsupportedTransport
        } else {
            FlowFailureStage::Encoding
        }
    }));
}

impl crate::WgpuRenderer {
    pub fn flow_execution_receipt(&self) -> Option<&FlowExecutionReceipt> {
        self.fast_frame.flow.receipt.as_ref()
    }
    pub fn flow_failure_stage(&self) -> Option<FlowFailureStage> {
        self.fast_frame.flow.failure.get()
    }
}

/// Native opt-in frames validate encoding separately from queue submission.
/// The synchronous oracle is intentionally unavailable on wasm.
pub(crate) struct GpuErrors(Vec<wgpu::ErrorScopeGuard>);
impl Drop for GpuErrors {
    fn drop(&mut self) {
        // Early-return/unwind paths also restore the nested error scopes LIFO.
        while self.0.pop().is_some() {}
    }
}
impl GpuErrors {
    pub(crate) fn new(device: &wgpu::Device, enabled: bool) -> Self {
        Self(if enabled {
            [
                wgpu::ErrorFilter::OutOfMemory,
                wgpu::ErrorFilter::Internal,
                wgpu::ErrorFilter::Validation,
            ]
            .into_iter()
            .map(|filter| device.push_error_scope(filter))
            .collect()
        } else {
            Vec::new()
        })
    }
    pub(crate) fn check(mut self) -> Result<()> {
        let mut error = None;
        while let Some(scope) = self.0.pop() {
            if let Some(found) = gpui::block_on(scope.pop()) {
                error = Some(found);
            }
        }
        if let Some(error) = error {
            anyhow::bail!("{error}");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{LayerFrame, LayerKey, Scene, SceneLayers, TileCoord, rgba};
    #[test]
    fn namespace_scope_restores_after_nested_panic() {
        let outer = FlowLayerNamespace::new();
        let inner = FlowLayerNamespace::new();
        assert_ne!(outer, inner);
        with_flow_layer_namespace(outer, || {
            assert_eq!(NAMESPACE.get(), Some(outer));
            let _ =
                std::panic::catch_unwind(|| with_flow_layer_namespace(inner, || panic!("test")));
            assert_eq!(NAMESPACE.get(), Some(outer));
        });
        assert_eq!(NAMESPACE.get(), None);
    }
    #[test]
    fn namespace_change_clears_layers_and_same_identity_retains_them() {
        let mut state = FrameState::default();
        let token = FlowLayerNamespace::new();
        let key = LayerKey(1);
        let tile = TileCoord { x: 0, y: 0 };
        let layers = SceneLayers {
            frames: vec![LayerFrame {
                key,
                generation: 1,
                background: rgba(0xffffffff),
                tile_size: 16,
                content: Scene::default().into(),
                dirty_tiles: vec![tile],
            }],
        };
        with_flow_layer_namespace(token, || {
            begin_frame(&mut state);
            state.layers.begin_frame(&layers, [(key, tile)].into_iter());
            begin_frame(&mut state);
            assert!(state.layers.holds(key, tile));
            with_flow_layer_namespace(FlowLayerNamespace::new(), || begin_frame(&mut state));
            assert!(state.layers.is_empty());
        });
        begin_frame(&mut state);
        assert_eq!(state.flow.namespace, None);
        assert!(state.flow.receipt.is_none());
    }
    #[test]
    #[cfg(not(target_family = "wasm"))]
    fn failure_resets_pending_and_published_counts() {
        let mut state = FrameState::default();
        with_flow_layer_namespace(FlowLayerNamespace::new(), || begin_frame(&mut state));
        state.flow.upload(256);
        state.flow.tile_pass();
        state.flow.submitted();
        assert!(state.flow.receipt.is_some());
        state.flow.fail(FlowFailureStage::Submission);
        assert_eq!(state.flow.pending.get(), FlowExecutionReceipt::default());
        assert!(state.flow.receipt.is_none());
    }
    #[test]
    #[cfg(not(target_family = "wasm"))]
    fn receipt_capture_is_single_draw_scoped_and_panic_safe() {
        let namespace = FlowLayerNamespace::new();
        let mut state = FrameState::default();
        let (_, empty) = with_flow_layer_namespace_receipt(namespace, || {});
        assert!(empty.is_none());
        let (_, one) = with_flow_layer_namespace_receipt(namespace, || {
            begin_draw(&mut state);
            state.flow.upload(256);
            state.flow.submitted();
        });
        let one = one.expect("one checked submission");
        assert_eq!(one.namespace, Some(namespace));
        assert!(!one.readback_completed);
        let (_, multiple) = with_flow_layer_namespace_receipt(namespace, || {
            begin_draw(&mut state);
            state.flow.submitted();
            begin_draw(&mut state); // The second draw never submits.
        });
        assert!(multiple.is_none());
        let (_, failed) = with_flow_layer_namespace_receipt(namespace, || {
            begin_draw(&mut state);
            state.flow.submitted();
            state.flow.fail(FlowFailureStage::Readback);
        });
        assert!(failed.is_none());
        let (_, nested) = with_flow_layer_namespace_receipt(namespace, || {
            begin_draw(&mut state);
            state.flow.submitted();
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                with_flow_layer_namespace_receipt(namespace, || {
                    begin_draw(&mut state);
                    panic!("nested draw failed");
                })
            }));
            assert!(CAPTURE.with(|capture| capture.borrow().is_some()));
        });
        assert!(nested.is_none());
        assert!(CAPTURE.with(|capture| capture.borrow().is_none()));
    }
}
