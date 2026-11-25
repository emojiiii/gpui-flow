//! True overlapping scene layers. Each layer retains its engine, editor, history,
//! viewport and resources; the app owns stacking and a single native input surface.
//! See `docs/layered-app.md` for the bounded contract and deliberate exclusions.
mod native;
mod routing;
#[cfg(test)]
mod tests;
use crate::SceneView;
use gpui::{prelude::*, *};
use gpui_flow_core::{Point as ScenePoint, Viewport};
use gpui_flow_engine::{ElementHandle, EngineError, InstanceState, SceneEngine};
use gpui_flow_interaction::{CanvasEvent, HitTarget};
use gpui_flow_scene::Scene;
use routing::{LayerAccess, LayerRouting};
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicU64, Ordering},
};

/// Runtime identity scoped to one app. Removed IDs are never reused on reattach.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LayerId {
    app: u64,
    serial: u64,
}
impl LayerId {
    pub fn serial(self) -> u64 {
        self.serial
    }
}
/// These flags affect the surface, not the objects' own visibility/hit policies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayerOptions {
    pub visible: bool,
    pub interactive: bool,
    /// Catch empty surface input. False lets the next lower layer receive it.
    /// Usually only the ground layer enables this for marquee/pan gestures.
    pub hit_background: bool,
}
impl Default for LayerOptions {
    fn default() -> Self {
        Self {
            visible: true,
            interactive: true,
            hit_background: false,
        }
    }
}
#[derive(Debug)]
pub enum LayeredAppError {
    InvalidLayer,
    AlreadyMountedOrEditing,
    InvalidIndex,
    InvalidCoordinates,
    Exhausted,
    Engine(EngineError),
}
impl std::fmt::Display for LayeredAppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLayer => write!(f, "stale, removed, or foreign layer ID"),
            Self::AlreadyMountedOrEditing => write!(
                f,
                "view is owned, destroyed, uses OwnedCanvas, or has active inner/frame edits"
            ),
            Self::InvalidIndex => write!(f, "layer index is outside the stack"),
            Self::InvalidCoordinates => write!(f, "invalid viewport or non-finite coordinates"),
            Self::Exhausted => write!(f, "layer identifier space exhausted"),
            Self::Engine(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for LayeredAppError {}
impl From<EngineError> for LayeredAppError {
    fn from(error: EngineError) -> Self {
        Self::Engine(error)
    }
}
#[derive(Clone, Debug)]
pub enum LayeredAppEvent {
    LayersChanged,
    /// Target is the actual receiving layer. The input is in app-local pixels;
    /// caller-supplied hit IDs are ignored and recomputed by each SceneView.
    Input {
        target: Option<LayerId>,
        input: CanvasEvent,
    },
}
impl EventEmitter<LayeredAppEvent> for LayeredApp {}
struct Layer {
    view: Entity<SceneView>,
    _observer: Subscription,
}
/// A single GPUI surface with bottom-to-top scene layers. Its own background is
/// painted once. Child SceneView chrome/grid/backdrops are suppressed; explicit
/// scene background objects and all regular scene content remain intact.
pub struct LayeredApp {
    owner: u64,
    next_id: u64,
    layers: BTreeMap<LayerId, Layer>,
    routing: LayerRouting,
    pub background: u32,
    focus: FocusHandle,
    origin: ScenePoint,
    native_pointer: crate::native_pointer::NativePointerState,
    subscriptions: Vec<Subscription>,
    input_window: Option<u64>,
}
impl LayeredApp {
    pub fn new(cx: &mut Context<Self>) -> Self {
        static NEXT_APP: AtomicU64 = AtomicU64::new(1);
        let owner = NEXT_APP
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .expect("app identity space exhausted");
        cx.on_release(|this: &mut Self, cx| {
            for layer in this.layers.values() {
                layer
                    .view
                    .update(cx, |view, cx| view.layer_destroy(this.owner, cx));
            }
        })
        .detach();
        Self {
            owner,
            next_id: 1,
            layers: BTreeMap::new(),
            routing: Default::default(),
            background: 0x171b24,
            focus: cx.focus_handle(),
            origin: ScenePoint::ZERO,
            native_pointer: Default::default(),
            subscriptions: vec![],
            input_window: None,
        }
    }
    pub fn add_layer(
        &mut self,
        scene: Scene,
        options: LayerOptions,
        cx: &mut Context<Self>,
    ) -> Result<LayerId, LayeredAppError> {
        self.add_engine(SceneEngine::new(scene)?, options, cx)
    }
    pub fn add_engine(
        &mut self,
        engine: SceneEngine,
        options: LayerOptions,
        cx: &mut Context<Self>,
    ) -> Result<LayerId, LayeredAppError> {
        let view = cx.new(|cx| SceneView::from_engine(engine, cx));
        self.attach_layer(view, options, cx)
    }
    /// Mount an unowned view, or one returned by detach_layer. Never render the
    /// same Entity elsewhere while mounted. Reattachment allocates a fresh ID.
    pub fn attach_layer(
        &mut self,
        view: Entity<SceneView>,
        options: LayerOptions,
        cx: &mut Context<Self>,
    ) -> Result<LayerId, LayeredAppError> {
        let next = self
            .next_id
            .checked_add(1)
            .ok_or(LayeredAppError::Exhausted)?;
        if !view.update(cx, |v, cx| v.layer_mount(self.owner, cx)) {
            return Err(LayeredAppError::AlreadyMountedOrEditing);
        }
        let id = LayerId {
            app: self.owner,
            serial: self.next_id,
        };
        self.next_id = next;
        let observer = cx.observe(&view, |_, _, cx| cx.notify());
        self.layers.insert(
            id,
            Layer {
                view,
                _observer: observer,
            },
        );
        self.routing.entries.push((id, options));
        self.changed(cx);
        Ok(id)
    }
    /// Trusted host access for scene edits, listeners, plugins and resources.
    /// Native focus and input belong to this app; use focus_element/dispatch_input
    /// here instead of invoking standalone SceneView native focus/inner editors.
    pub fn layer(&self, id: LayerId) -> Option<&Entity<SceneView>> {
        self.layers.get(&id).map(|l| &l.view)
    }
    pub fn layer_ids(&self) -> impl DoubleEndedIterator<Item = LayerId> + '_ {
        self.routing.entries.iter().map(|e| e.0)
    }
    pub fn layer_options(&self, id: LayerId) -> Option<LayerOptions> {
        self.routing.entries.iter().find(|e| e.0 == id).map(|e| e.1)
    }
    pub fn focused_layer(&self) -> Option<LayerId> {
        self.routing.focused
    }
    /// The owner of the accepted pointer stream, including listener-only presses.
    pub fn pointer_layer(&self) -> Option<LayerId> {
        self.routing.pressed_layer()
    }
    fn changed(&self, cx: &mut Context<Self>) {
        cx.emit(LayeredAppEvent::LayersChanged);
        cx.notify();
    }
    pub fn set_layer_options(
        &mut self,
        id: LayerId,
        options: LayerOptions,
        cx: &mut Context<Self>,
    ) -> Result<(), LayeredAppError> {
        let index = self
            .routing
            .entries
            .iter()
            .position(|e| e.0 == id)
            .ok_or(LayeredAppError::InvalidLayer)?;
        if self.routing.entries[index].1 != options {
            self.routing.invalidate(
                id,
                &mut NativeAccess {
                    layers: &self.layers,
                    cx,
                },
            );
            self.routing.entries[index].1 = options;
            self.changed(cx);
        }
        Ok(())
    }
    /// Zero is bottom. Reordering cancels a current gesture and suppresses its
    /// eventual release, so a newly exposed layer never receives a stale Up.
    pub fn reorder_layer(
        &mut self,
        id: LayerId,
        index: usize,
        cx: &mut Context<Self>,
    ) -> Result<(), LayeredAppError> {
        let previous = self
            .routing
            .entries
            .iter()
            .position(|e| e.0 == id)
            .ok_or(LayeredAppError::InvalidLayer)?;
        if index >= self.routing.entries.len() {
            return Err(LayeredAppError::InvalidIndex);
        }
        if previous != index {
            self.routing.reorder(&mut NativeAccess {
                layers: &self.layers,
                cx,
            });
            let layer = self.routing.entries.remove(previous);
            self.routing.entries.insert(index, layer);
            self.changed(cx);
        }
        Ok(())
    }
    /// Remove only app ownership. Scene identity/history/resources/listeners are
    /// preserved; capture/focus/hover and queued action senders are terminated.
    pub fn detach_layer(
        &mut self,
        id: LayerId,
        cx: &mut Context<Self>,
    ) -> Result<Entity<SceneView>, LayeredAppError> {
        if !self.layers.contains_key(&id) {
            return Err(LayeredAppError::InvalidLayer);
        }
        self.routing.invalidate(
            id,
            &mut NativeAccess {
                layers: &self.layers,
                cx,
            },
        );
        self.routing.entries.retain(|e| e.0 != id);
        let layer = self.layers.remove(&id).unwrap();
        layer
            .view
            .update(cx, |v, cx| v.layer_unmount(self.owner, cx));
        self.changed(cx);
        Ok(layer.view)
    }
    /// Terminally dispose only this layer, including detached engine instances
    /// and resource jobs. Other layers' history, viewport and listeners survive.
    pub fn destroy_layer(
        &mut self,
        id: LayerId,
        cx: &mut Context<Self>,
    ) -> Result<(), LayeredAppError> {
        if !self.layers.contains_key(&id) {
            return Err(LayeredAppError::InvalidLayer);
        }
        self.routing.invalidate(
            id,
            &mut NativeAccess {
                layers: &self.layers,
                cx,
            },
        );
        self.routing.entries.retain(|e| e.0 != id);
        let layer = self.layers.remove(&id).unwrap();
        layer
            .view
            .update(cx, |v, cx| v.layer_destroy(self.owner, cx));
        self.changed(cx);
        Ok(())
    }
    pub fn set_layer_viewport(
        &mut self,
        id: LayerId,
        viewport: Viewport,
        cx: &mut Context<Self>,
    ) -> Result<(), LayeredAppError> {
        if !viewport.is_valid() {
            return Err(LayeredAppError::InvalidCoordinates);
        }
        let layer = self.layers.get(&id).ok_or(LayeredAppError::InvalidLayer)?;
        self.routing.invalidate(
            id,
            &mut NativeAccess {
                layers: &self.layers,
                cx,
            },
        );
        layer.view.update(cx, |v, cx| {
            v.editor.viewport = viewport;
            cx.notify();
        });
        Ok(())
    }
    pub fn layer_to_app(
        &self,
        id: LayerId,
        world: ScenePoint,
        cx: &App,
    ) -> Result<ScenePoint, LayeredAppError> {
        let viewport = self
            .layer(id)
            .ok_or(LayeredAppError::InvalidLayer)?
            .read(cx)
            .editor
            .viewport;
        map_coordinate(viewport, world, false)
    }
    pub fn app_to_layer(
        &self,
        id: LayerId,
        local: ScenePoint,
        cx: &App,
    ) -> Result<ScenePoint, LayeredAppError> {
        let viewport = self
            .layer(id)
            .ok_or(LayeredAppError::InvalidLayer)?
            .read(cx)
            .editor
            .viewport;
        map_coordinate(viewport, local, true)
    }
    pub fn map_between_layers(
        &self,
        from: LayerId,
        to: LayerId,
        world: ScenePoint,
        cx: &App,
    ) -> Result<ScenePoint, LayeredAppError> {
        self.app_to_layer(to, self.layer_to_app(from, world, cx)?, cx)
    }
    /// Synchronously forward app-local input. Hit payloads are never trusted;
    /// the exact SceneView picker supplies each layer's target. Native adapters
    /// also call this method, so headless routing tests exercise the same logic.
    pub fn dispatch_input(
        &mut self,
        input: CanvasEvent,
        cx: &mut Context<Self>,
    ) -> Option<LayerId> {
        if matches!(input, CanvasEvent::PointerCancel | CanvasEvent::FocusLost) {
            self.native_pointer.reset();
        }
        let target = self.routing.route(
            input.clone(),
            &mut NativeAccess {
                layers: &self.layers,
                cx,
            },
        );
        cx.emit(LayeredAppEvent::Input { target, input });
        cx.notify();
        target
    }
    pub fn focus_element(
        &mut self,
        id: LayerId,
        handle: ElementHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<bool, LayeredAppError> {
        let options = self
            .layer_options(id)
            .ok_or(LayeredAppError::InvalidLayer)?;
        if !options.visible || !options.interactive {
            return Err(LayeredAppError::InvalidLayer);
        }
        let view = self.layer(id).unwrap().clone();
        let engine = &view.read(cx).editor.engine;
        engine.object(handle)?;
        if engine.instance(handle)?.state != InstanceState::Attached {
            return Err(EngineError::DetachedHandle.into());
        }
        window.focus(&self.focus, cx);
        if !self.focus.is_focused(window) {
            return Ok(false);
        }
        self.routing.reorder(&mut NativeAccess {
            layers: &self.layers,
            cx,
        });
        self.routing.activate(
            Some(id),
            &mut NativeAccess {
                layers: &self.layers,
                cx,
            },
        );
        Ok(view.update(cx, |v, cx| v.layer_focus(handle, cx))?)
    }
}
fn map_coordinate(
    viewport: Viewport,
    point: ScenePoint,
    inverse: bool,
) -> Result<ScenePoint, LayeredAppError> {
    if !viewport.is_valid() || !point.is_finite() {
        return Err(LayeredAppError::InvalidCoordinates);
    }
    let result = if inverse {
        viewport.screen_to_world(point)
    } else {
        viewport.world_to_screen(point)
    };
    result
        .is_finite()
        .then_some(result)
        .ok_or(LayeredAppError::InvalidCoordinates)
}
struct NativeAccess<'a> {
    layers: &'a BTreeMap<LayerId, Layer>,
    cx: &'a mut App,
}
impl LayerAccess for NativeAccess<'_> {
    fn hit(&mut self, id: LayerId, position: ScenePoint) -> HitTarget {
        self.layers.get(&id).map_or(HitTarget::Background, |l| {
            l.view.read(self.cx).layer_hit(position)
        })
    }
    fn send(&mut self, id: LayerId, event: CanvasEvent) {
        if let Some(layer) = self.layers.get(&id) {
            layer
                .view
                .update(self.cx, |v, cx| v.layer_dispatch(event, cx));
        }
    }
}
impl Focusable for LayeredApp {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
