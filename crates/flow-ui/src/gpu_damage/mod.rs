//! Opt-in, bounded Linux GPU tile reuse over the normal Flow paint stream.
//! Unsupported inputs retain the native full-paint path. This does not reduce
//! CPU scene construction or promise a hard bound on total driver memory.
mod cache;
#[cfg(all(test, feature = "gpu-damage-oracle"))]
mod native_tests;
mod platform;
mod surface;
#[cfg(test)]
mod tests;
use cache::{Admission, Cache, Damage, FrameKey, MAX_TILES, Prepared};
use gpui::{self as g, *};
use gpui_flow_engine::{ObserverId, SceneEngine};
use gpui_wgpu::{FlowExecutionReceipt, FlowLayerNamespace, with_flow_layer_namespace_receipt};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::{Rc, Weak},
};

/// Last window submission. Counts come from the backend, not analytic damage.
#[derive(Clone, Default, Debug)]
pub struct GpuDamageStatus {
    pub receipt: Option<FlowExecutionReceipt>,
    pub admitted_tiles: usize,
    pub fallback: Vec<String>,
}
/// Creates windows that retain ordinary input/IME/AT behavior and permit only
/// this adapter's inline GPUI surfaces (no native/external composition surfaces).
#[derive(Clone, Default)]
pub struct GpuDamageHost {
    windows: Rc<RefCell<HashMap<u64, Weak<RefCell<WindowState>>>>>,
}
impl GpuDamageHost {
    pub fn application(&self) -> Application {
        Application::with_platform(Rc::new(GpuPlatform {
            inner: gpui_platform::current_platform(false),
            host: self.clone(),
        }))
    }
    pub fn status(&self, window: &Window) -> Option<GpuDamageStatus> {
        self.record(window)
            .map(|state| state.borrow().status.clone())
    }
    fn record(&self, window: &Window) -> Option<Rc<RefCell<WindowState>>> {
        self.windows
            .borrow()
            .get(&window.window_handle().window_id().as_u64())
            .and_then(Weak::upgrade)
    }
    pub(crate) fn bind(
        &self,
        engine: &mut SceneEngine,
        window: &mut Window,
    ) -> anyhow::Result<Binding> {
        let record = self.record(window).ok_or_else(|| {
            anyhow::anyhow!("create this window with GpuDamageHost::application first")
        })?;
        record
            .borrow_mut()
            .bindings
            .retain(|_, input| input.strong_count() > 0);
        anyhow::ensure!(
            record.borrow().bindings.len() < 32,
            "at most 32 GPU views per window"
        );
        let id = {
            let mut r = record.borrow_mut();
            let id = r.next_key;
            r.next_key = r
                .next_key
                .checked_add(1)
                .filter(|n| *n < 0x0100_0000)
                .ok_or_else(|| anyhow::anyhow!("GPU layer identity exhausted"))?;
            id
        };
        let editing = Editing::new(record.borrow().editing.clone());
        let surface = (|| {
            let composition = window.enable_window_composition()?;
            composition.create_gpui_surface(None)
        })();
        drop(editing);
        let surface = surface?;
        let input = Rc::new(RefCell::new(Input {
            layer: LayerKey(id),
            damage: Damage::new(engine.revision()),
            cache: Cache::default(),
            request: None,
        }));
        record
            .borrow_mut()
            .bindings
            .insert(surface.id(), Rc::downgrade(&input));
        let weak = Rc::downgrade(&input);
        let observer = match engine.on_change(None, move |change| {
            if let Some(input) = weak.upgrade() {
                input.borrow_mut().damage.observe(change);
            }
        }) {
            Ok(observer) => observer,
            Err(error) => {
                record.borrow_mut().bindings.remove(&surface.id());
                let _editing = Editing::new(record.borrow().editing.clone());
                if let Ok(composition) = window.enable_window_composition() {
                    let _ = composition.remove_surface(surface.id());
                }
                return Err(error.into());
            }
        };
        Ok(Binding {
            surface,
            window: window.window_handle(),
            record,
            input,
            observer,
            engine: engine.engine_id(),
        })
    }
}
pub(crate) struct Binding {
    surface: WindowCompositionSurface,
    window: AnyWindowHandle,
    record: Rc<RefCell<WindowState>>,
    input: Rc<RefCell<Input>>,
    observer: ObserverId,
    engine: u64,
}
impl Binding {
    pub fn dispose(self, engine: &mut SceneEngine, cx: &mut App) {
        engine.off_change(self.observer);
        let id = self.surface.id();
        self.record.borrow_mut().bindings.remove(&id);
        let record = self.record.clone();
        cx.defer(move |cx| {
            let _ = self.window.update(cx, |_, window, _| {
                let editing = Editing::new(record.borrow().editing.clone());
                if let Ok(composition) = window.enable_window_composition() {
                    let _ = composition.remove_surface(id);
                }
                drop(editing);
            });
        });
    }
    pub fn matches(&self, engine: &SceneEngine) -> bool {
        self.engine == engine.engine_id()
    }
}
struct Input {
    layer: LayerKey,
    damage: Damage,
    cache: Cache,
    request: Option<Request>,
}
#[derive(Clone)]
struct Request {
    key: FrameKey,
    eligible: bool,
}
struct WindowState {
    namespace: FlowLayerNamespace,
    bindings: HashMap<CompositionSurfaceId, Weak<RefCell<Input>>>,
    next_key: u32,
    editing: Rc<Cell<bool>>,
    order: Vec<CompositionSurfaceId>,
    status: GpuDamageStatus,
    #[cfg(all(test, feature = "gpu-damage-oracle"))]
    oracle: Option<OracleProbe>,
}
struct GpuPlatform {
    inner: Rc<dyn Platform>,
    host: GpuDamageHost,
}
struct GpuWindow {
    inner: Box<dyn PlatformWindow>,
    host: GpuDamageHost,
    id: u64,
    state: Rc<RefCell<WindowState>>,
}
impl GpuWindow {
    fn new(inner: Box<dyn PlatformWindow>, host: GpuDamageHost, window: AnyWindowHandle) -> Self {
        let id = window.window_id().as_u64();
        let state = Rc::new(RefCell::new(WindowState {
            namespace: FlowLayerNamespace::new_window_space(),
            bindings: HashMap::new(),
            next_key: 1,
            editing: Rc::new(Cell::new(false)),
            order: Vec::new(),
            status: Default::default(),
            #[cfg(all(test, feature = "gpu-damage-oracle"))]
            oracle: None,
        }));
        host.windows.borrow_mut().insert(id, Rc::downgrade(&state));
        Self {
            inner,
            host,
            id,
            state,
        }
    }
    fn accept_order(&self, surfaces: &[PlatformCompositionSurface]) -> anyhow::Result<()> {
        anyhow::ensure!(
            surfaces
                .iter()
                .all(|s| matches!(s.content, PlatformCompositionSurfaceContent::Gpui)),
            "native/external composition is unsupported by Flow GPU windows"
        );
        let next: Vec<_> = surfaces.iter().map(|s| s.id).collect();
        let mut state = self.state.borrow_mut();
        anyhow::ensure!(
            state.order.is_empty() || state.editing.get() || next == state.order,
            "only Flow owns GPU surface creation/reordering in this window"
        );
        state.order = next;
        Ok(())
    }
    fn draw_direct(&self, scene: &g::Scene) {
        let mut state = self.state.borrow_mut();
        state.status = Default::default();
        for input in state.bindings.values().filter_map(Weak::upgrade) {
            input.borrow_mut().cache.invalidate();
        }
        drop(state);
        self.inner.draw(scene);
    }
    fn draw_flow(&self, scene: ComposedScene<'_>) {
        // Host layer keys belong to an independent allocator. Draw the exact
        // original frame unscoped rather than copying/merging its layer table.
        if !scene.scene().layers.frames.is_empty() {
            self.draw_direct(scene.scene());
            self.state
                .borrow_mut()
                .status
                .fallback
                .push("host scroll layers use the unchanged native backend".into());
            return;
        }
        // Sort spans back into authored paint order. Surfaces only identify a
        // Flow slice; a later ordinary sibling must still cover an earlier one.
        let mut spans: Vec<_> = scene
            .layers()
            .iter()
            .flat_map(|layer| {
                layer
                    .ranges
                    .iter()
                    .cloned()
                    .map(move |range| (range, layer.surface))
            })
            .collect();
        spans.sort_by_key(|(range, _)| range.start);
        let valid = spans.first().is_none_or(|(r, _)| r.start == 0)
            && spans
                .last()
                .is_none_or(|(r, _)| r.end == scene.scene().len())
            && spans
                .windows(2)
                .all(|pair| pair[0].0.end == pair[1].0.start);
        if !valid {
            self.draw_direct(scene.scene());
            return;
        }
        let mut output = g::Scene::default();
        let mut acks = Vec::new();
        let mut status = GpuDamageStatus::default();
        let mut remaining = Admission::budget(MAX_TILES);
        #[allow(unused_mut)]
        let mut state = self.state.borrow_mut();
        let namespace = state.namespace;
        for (range, surface) in &spans {
            let mut part = g::Scene::default();
            if output.len() > 8192 || part.replay_balanced(range.clone(), scene.scene()).is_err() {
                drop(state);
                self.draw_direct(scene.scene());
                return;
            }
            part.finish();
            let input = state.bindings.get(surface).and_then(Weak::upgrade);
            #[cfg(all(test, feature = "gpu-damage-oracle"))]
            let oracle_source = input
                .as_ref()
                .and_then(|input| input.borrow().request.clone())
                .and_then(|request| {
                    let mut direct = g::Scene::default();
                    direct.replay_balanced(0..part.len(), &part).ok()?;
                    direct.finish();
                    Some((direct, request.key))
                });
            let Prepared {
                scene: part,
                key,
                admission,
                fallback,
            } = if let Some(input) = &input {
                let mut input = input.borrow_mut();
                if spans.iter().filter(|(_, id)| id == surface).count() != 1 {
                    input.cache.invalidate();
                    Prepared {
                        scene: part,
                        key: None,
                        admission: Admission::default(),
                        fallback: Some("split Flow span uses full painting"),
                    }
                } else if let Some(request) = input.request.clone() {
                    let damage = input.damage.clone();
                    let layer = input.layer;
                    input.cache.prepare(
                        part,
                        request.key,
                        &damage,
                        layer,
                        request.eligible,
                        remaining,
                    )
                } else {
                    input.cache.invalidate();
                    Prepared {
                        scene: part,
                        key: None,
                        admission: Admission::default(),
                        fallback: Some("no current Flow paint metadata"),
                    }
                }
            } else {
                Prepared {
                    scene: part,
                    key: None,
                    admission: Admission::default(),
                    fallback: None,
                }
            };
            #[cfg(all(test, feature = "gpu-damage-oracle"))]
            if let (Some(probe), Some((direct, key))) = (state.oracle.as_mut(), oracle_source) {
                probe(&direct, &part, &key, fallback);
            }
            if let Some(reason) = fallback
                && status.fallback.len() < 32
            {
                status.fallback.push(reason.into());
            }
            status.admitted_tiles += admission.tiles;
            remaining.consume(admission);
            if let (Some(input), Some(key)) = (input, key) {
                acks.push((input, key));
            }
            if output.replay_balanced(0..part.len(), &part).is_err() {
                drop(state);
                self.draw_direct(scene.scene());
                return;
            }
        }
        drop(state);
        output.finish();
        if status.admitted_tiles > 0 {
            let (_, receipt) =
                with_flow_layer_namespace_receipt(namespace, || self.inner.draw(&output));
            if let Some(receipt) = receipt.filter(|r| r.submitted && r.namespace == Some(namespace))
            {
                for (input, key) in acks {
                    let mut input = input.borrow_mut();
                    input.cache.acknowledge(&key);
                    input.damage.acknowledge(key.revision);
                }
                status.receipt = Some(receipt);
            } else {
                // A small-device limit or unrelated large host scene can still
                // exceed backend resources. Retry the original native frame
                // once, never an already transformed tile scene; no cache ack.
                self.draw_direct(scene.scene());
                self.state
                    .borrow_mut()
                    .status
                    .fallback
                    .push("GPU submission unconfirmed; original native frame retried".into());
                return;
            }
        } else {
            self.inner.draw(&output);
        }
        self.state.borrow_mut().status = status;
    }
}
impl Drop for GpuWindow {
    fn drop(&mut self) {
        self.host.windows.borrow_mut().remove(&self.id);
    }
}

struct Editing {
    flag: Rc<Cell<bool>>,
    previous: bool,
}
impl Editing {
    fn new(flag: Rc<Cell<bool>>) -> Self {
        let previous = flag.replace(true);
        Self { flag, previous }
    }
}
impl Drop for Editing {
    fn drop(&mut self) {
        self.flag.set(self.previous);
    }
}

#[cfg(all(test, feature = "gpu-damage-oracle"))]
type OracleProbe = Box<dyn FnMut(&g::Scene, &g::Scene, &FrameKey, Option<&'static str>)>;
