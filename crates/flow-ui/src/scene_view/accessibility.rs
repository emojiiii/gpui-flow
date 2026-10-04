//! Optional real AccessKit nodes over the accepted presentation. Proxy Divs own
//! no mouse handlers/hitboxes or keyboard focus; the canvas is one native stop.
use super::*;
use crate::display::composite::RequestKey;
use gpui_flow_engine::{ElementHandle, InstanceState, SceneEngine};
use gpui_flow_interaction::{Capability, scene::object_editable};
use gpui_flow_scene::{
    Affine, Bounds as SceneBounds, EraserMode, MaskMode, SemanticAction, SemanticSource, Shape,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

actions!(
    flow_scene_semantics,
    [NextTarget, PreviousTarget, ActivateTarget]
);
struct SemanticBindings;
impl Global for SemanticBindings {}
fn register_bindings(cx: &mut App) {
    if cx.has_global::<SemanticBindings>() {
        return;
    }
    cx.set_global(SemanticBindings);
    cx.bind_keys([
        KeyBinding::new("tab", NextTarget, Some("FlowSceneSemantics")),
        KeyBinding::new("shift-tab", PreviousTarget, Some("FlowSceneSemantics")),
        KeyBinding::new("enter", ActivateTarget, Some("FlowSceneSemantics")),
    ]);
}

const MAX_NODES: usize = 2048;
const MAX_LABEL_BYTES: usize = 64 * 1024;

/// Opt-in scene semantics. Labels come from object name, Text content, then ID.
/// No document data or pointer behavior is changed by this configuration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SceneAccessibility {
    pub label: String,
}
impl SceneAccessibility {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SceneAccessibilityIssue {
    /// The entire projection is withheld, never silently truncated.
    BudgetExceeded,
    /// Logical bounds cannot establish this target's supported visibility.
    UnsupportedVisibility(DisplayId),
    UnsupportedNative(DisplayId),
    ManagedLayer,
    IdentityExhausted,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Identity {
    handle: ElementHandle,
    attachment: u64,
    document: u64,
}
impl Identity {
    fn new(engine: &SceneEngine, handle: ElementHandle) -> Option<Self> {
        let instance = engine.instance(handle).ok()?;
        (instance.state == InstanceState::Attached).then_some(Self {
            handle,
            attachment: instance.attachment_epoch,
            document: engine.document_epoch(),
        })
    }
    fn current(self, engine: &SceneEngine) -> bool {
        Self::new(engine, self.handle) == Some(self)
    }
}
#[derive(Clone)]
struct SemanticNode {
    identity: Identity,
    serial: u64,
    label: String,
    role: Role,
    bounds: SceneBounds,
    disabled: bool,
    read_only: bool,
    selected: bool,
    text: bool,
}
#[derive(Default)]
pub(super) struct AccessibilityState {
    config: Option<SceneAccessibility>,
    generation: Arc<()>,
    next_serial: u64,
    bindings: BTreeMap<Identity, u64>,
    nodes: Vec<SemanticNode>,
    key: Option<Arc<RequestKey>>,
    issues: Vec<SceneAccessibilityIssue>,
    resume: Option<Identity>,
    entry_pending: bool,
}
#[derive(Clone)]
struct Activation {
    identity: Identity,
    generation: Arc<()>,
    key: Arc<RequestKey>,
}
fn navigation_key_allowed(
    key: &str,
    modifiers: Modifiers,
    inner: bool,
    crop: bool,
    space: bool,
) -> bool {
    !inner
        && !crop
        && !space
        && !modifiers.control
        && !modifiers.alt
        && !modifiers.platform
        && (key == "tab" || (key == "enter" && !modifiers.shift))
}
impl Activation {
    fn matches_projection(&self, state: &AccessibilityState) -> bool {
        state.config.is_some()
            && Arc::ptr_eq(&self.generation, &state.generation)
            && state.key.as_ref() == Some(&self.key)
            && state
                .nodes
                .iter()
                .any(|node| node.identity == self.identity && !node.disabled)
    }
}
#[derive(Debug, PartialEq, Eq)]
enum Step {
    Target(usize),
    Exit,
}
fn next_target(nodes: &[SemanticNode], focus: Option<ElementHandle>, reverse: bool) -> Step {
    let current = focus.and_then(|h| nodes.iter().position(|n| n.identity.handle == h));
    let target = if reverse {
        let end = current.unwrap_or(nodes.len());
        (0..end).rev().find(|i| !nodes[*i].disabled)
    } else {
        let start = current.map_or(0, |i| i + 1);
        (start..nodes.len()).find(|i| !nodes[*i].disabled)
    };
    target.map_or(Step::Exit, Step::Target)
}
fn intersection(a: SceneBounds, b: SceneBounds) -> Option<SceneBounds> {
    let x = a.origin.x.max(b.origin.x);
    let y = a.origin.y.max(b.origin.y);
    let right = (a.origin.x + a.size.width).min(b.origin.x + b.size.width);
    let bottom = (a.origin.y + a.size.height).min(b.origin.y + b.size.height);
    let result = SceneBounds::new(WorldPoint::new(x, y), WorldSize::new(right - x, bottom - y));
    (result.is_valid() && result.size.is_positive()).then_some(result)
}
fn axis_aligned(m: Affine) -> bool {
    (m.b == 0. && m.c == 0.) || (m.a == 0. && m.d == 0.)
}
fn live_policy(
    editor: &SceneEditor,
    host: &gpui_flow_interaction::scene::ScenePluginHost,
    identity: Identity,
    action: SemanticAction,
) -> bool {
    if !identity.current(&editor.engine) || !host.allows(Capability::Navigation) {
        return false;
    }
    let Ok(instance) = editor.engine.instance(identity.handle) else {
        return false;
    };
    let visible = editor
        .scene()
        .ancestor_path(&instance.id)
        .is_ok_and(|path| {
            path.iter().all(|id| {
                editor.scene().get(id).is_some_and(|n| {
                    n.object.visible && n.object.style.opacity > 0. && !n.object.locked
                })
            })
        });
    visible
        && object_editable(editor.scene(), &instance.id)
        && (action == SemanticAction::Focus
            || (host.allows(Capability::Edit)
                && editor
                    .engine
                    .object(identity.handle)
                    .is_ok_and(|o| matches!(o.shape, Shape::Text(_)))))
}

impl AccessibilityState {
    pub(super) fn configure(&mut self, config: Option<SceneAccessibility>) -> bool {
        if self.config == config {
            return false;
        }
        self.generation = Arc::new(());
        self.config = config;
        self.clear_projection();
        self.bindings.clear();
        self.issues.clear();
        self.resume = None;
        self.entry_pending = false;
        true
    }
    fn active_serial(&self, focused: bool, handle: Option<ElementHandle>) -> Option<u64> {
        focused.then_some(handle).flatten().and_then(|handle| {
            self.nodes
                .iter()
                .find(|node| node.identity.handle == handle && !node.disabled)
                .map(|node| node.serial)
        })
    }
    #[cfg(test)]
    pub(super) fn inspect(
        &self,
        focus: Option<ElementHandle>,
    ) -> Vec<(ElementHandle, SceneBounds, bool)> {
        let active = self.active_serial(true, focus);
        self.nodes
            .iter()
            .map(|node| {
                (
                    node.identity.handle,
                    node.bounds,
                    active == Some(node.serial),
                )
            })
            .collect()
    }
    pub(super) fn clear_projection(&mut self) {
        self.nodes.clear();
        self.key = None;
        // Preserve IDs while the same attached objects wait for a CPU upload.
    }
    fn reconcile(&mut self, engine: &SceneEngine) {
        self.bindings.retain(|identity, _| identity.current(engine));
        if self
            .resume
            .is_some_and(|identity| !identity.current(engine))
        {
            self.resume = None;
        }
    }
    pub(super) fn project(
        &mut self,
        frame: &render::StateFrame,
        geometry: &Scene,
        editor: &SceneEditor,
        host: &gpui_flow_interaction::scene::ScenePluginHost,
        key: Arc<RequestKey>,
    ) {
        self.clear_projection();
        self.issues.clear();
        self.reconcile(&editor.engine);
        let Some(config) = &self.config else { return };
        let mut bytes = config.label.len();
        if bytes > MAX_LABEL_BYTES || frame.objects.len() > MAX_NODES {
            self.issues.push(SceneAccessibilityIssue::BudgetExceeded);
            self.bindings.clear();
            return;
        }
        let mut admitted = BTreeSet::new();
        let mut unsupported = BTreeSet::new();
        for unit in &frame.units {
            match unit {
                gpui_flow_composite::SceneUnit::Native(id)
                | gpui_flow_composite::SceneUnit::NativePhase { id, .. } => {
                    if frame
                        .objects
                        .iter()
                        .any(|o| &o.object.id == id && o.native_renderable())
                    {
                        admitted.insert(id.clone());
                    } else {
                        self.issues
                            .push(SceneAccessibilityIssue::UnsupportedNative(id.clone()));
                    }
                }
                gpui_flow_composite::SceneUnit::Domain(domain) => {
                    let ids = domain.ids();
                    if ids
                        .len()
                        .saturating_add(admitted.len())
                        .saturating_add(unsupported.len())
                        > MAX_NODES
                    {
                        self.issues.clear();
                        self.issues.push(SceneAccessibilityIssue::BudgetExceeded);
                        self.bindings.clear();
                        return;
                    }
                    // Masks/erasers can make otherwise nonempty AABBs entirely
                    // invisible. Do not infer operability from those bounds.
                    let complex = ids.iter().any(|id| {
                        geometry.get(id).is_some_and(|n| {
                            n.object.mask != MaskMode::None || n.object.eraser != EraserMode::None
                        })
                    });
                    for id in ids {
                        if complex {
                            unsupported.insert(id.clone());
                        } else {
                            admitted.insert(id.clone());
                        }
                    }
                }
                gpui_flow_composite::SceneUnit::Rejected(_) => {}
            }
        }
        let Some(canvas) = intersection(key.viewport.canvas_bounds, key.viewport.content_mask)
        else {
            self.key = Some(key);
            return;
        };
        let clip = SceneBounds::new(
            canvas.origin - key.viewport.canvas_bounds.origin,
            canvas.size,
        );
        let mut nodes = Vec::new();
        for object in &frame.objects {
            let id = &object.object.id;
            if !object.object.visible || object.opacity <= 0. {
                continue;
            }
            if unsupported.contains(id) {
                self.issues
                    .push(SceneAccessibilityIssue::UnsupportedVisibility(id.clone()));
                continue;
            }
            if !admitted.contains(id) {
                continue;
            }
            let Some(handle) = editor.engine.handle(id) else {
                continue;
            };
            let Some(identity) = Identity::new(&editor.engine, handle) else {
                continue;
            };
            let local = if matches!(object.object.shape, Shape::Group(_)) {
                geometry.local_bounds(id).ok().flatten()
            } else {
                object.object.paint_bounds()
            };
            let Some(local) = local else { continue };
            let full_bounds = object.screen_transform.transform_bounds(local);
            let Some(mut bounds) = intersection(full_bounds, clip) else {
                continue;
            };
            let mut complex = !axis_aligned(object.screen_transform) && bounds != full_bounds;
            let mut outside = false;
            for region in &object.clips {
                let screen = object.viewport_transform * region.world_transform;
                match intersection(bounds, screen.transform_bounds(region.local_bounds)) {
                    Some(clipped) => bounds = clipped,
                    None => {
                        outside = true;
                        break;
                    }
                }
                complex |= !region.is_rectangular()
                    || !axis_aligned(screen)
                    || !axis_aligned(object.screen_transform);
            }
            if outside {
                continue;
            }
            if complex || matches!(object.object.shape, Shape::Custom(_)) {
                self.issues
                    .push(SceneAccessibilityIssue::UnsupportedVisibility(id.clone()));
                continue;
            }
            let text = matches!(object.object.shape, Shape::Text(_));
            let label = if !object.object.name.is_empty() {
                object.object.name.as_str()
            } else if let Shape::Text(text) = &object.object.shape {
                text.content.as_str()
            } else {
                id.as_str()
            };
            bytes = bytes.saturating_add(label.len());
            if bytes > MAX_LABEL_BYTES {
                self.issues.clear();
                self.issues.push(SceneAccessibilityIssue::BudgetExceeded);
                self.bindings.clear();
                return;
            }
            let serial = match self.bindings.get(&identity) {
                Some(serial) => *serial,
                None => {
                    if self.bindings.len() >= MAX_NODES {
                        self.issues.clear();
                        self.issues.push(SceneAccessibilityIssue::BudgetExceeded);
                        self.bindings.clear();
                        return;
                    }
                    let Some(serial) = self.next_serial.checked_add(1) else {
                        self.issues.clear();
                        self.issues.push(SceneAccessibilityIssue::IdentityExhausted);
                        self.bindings.clear();
                        return;
                    };
                    self.next_serial = serial;
                    self.bindings.insert(identity, serial);
                    serial
                }
            };
            nodes.push(SemanticNode {
                identity,
                serial,
                label: label.to_owned(),
                role: if text {
                    Role::Label
                } else if object.object.shape.allows_children() {
                    Role::Group
                } else {
                    Role::GraphicsObject
                },
                bounds,
                disabled: !live_policy(editor, host, identity, SemanticAction::Focus),
                read_only: !live_policy(editor, host, identity, SemanticAction::OpenText),
                selected: editor.selection.ids.contains(id),
                text,
            });
        }
        self.nodes = nodes;
        self.key = Some(key);
    }
    fn token(&self, node: &SemanticNode) -> Option<Activation> {
        Some(Activation {
            identity: node.identity,
            generation: self.generation.clone(),
            key: self.key.clone()?,
        })
    }
}

impl SceneView {
    pub fn set_accessibility(
        &mut self,
        config: Option<SceneAccessibility>,
        cx: &mut Context<Self>,
    ) {
        if self.accessibility.configure(config) {
            // Div's tab_index/stop builders only configure an internally
            // created handle. We explicitly track this existing handle, so
            // configure that same focus identity before GPUI registers it.
            self.focus = self
                .focus
                .clone()
                .tab_index(0)
                .tab_stop(self.accessibility_enabled());
            if self.accessibility_enabled() {
                register_bindings(cx);
            }
            cx.notify();
        }
    }
    pub fn accessibility_enabled(&self) -> bool {
        self.accessibility.config.is_some()
    }
    pub fn accessibility_issues(&self) -> &[SceneAccessibilityIssue] {
        &self.accessibility.issues
    }
    pub(super) fn project_accessibility(
        &mut self,
        frame: &render::StateFrame,
        key: Option<Arc<RequestKey>>,
    ) {
        if !self.accessibility_enabled() {
            return;
        }
        if self.is_layer_managed() {
            self.accessibility.clear_projection();
            self.accessibility.issues = vec![SceneAccessibilityIssue::ManagedLayer];
            return;
        }
        let Some(key) = key.filter(|key| self.presentation_draw_current(key)) else {
            self.accessibility.clear_projection();
            self.accessibility.reconcile(&self.editor.engine);
            return;
        };
        let mut accessibility = std::mem::take(&mut self.accessibility);
        if let Some(geometry) = self.presentation_hit_geometry() {
            accessibility.project(
                frame,
                geometry.scene(),
                &self.editor,
                &self.interactions,
                key,
            );
        } else {
            accessibility.clear_projection();
        }
        self.accessibility = accessibility;
    }
    fn semantic_current(
        &self,
        token: &Activation,
        action: SemanticAction,
        window: &Window,
    ) -> bool {
        if !self.accessibility_enabled()
            || !window.is_visible()
            || self.is_layer_managed()
            || self.inner_tool_active()
            || self.image_crop_editor_active()
            || !token.matches_projection(&self.accessibility)
            || !self.presentation_draw_current(&token.key)
            || !live_policy(&self.editor, &self.interactions, token.identity, action)
        {
            return false;
        }
        let Some(mut actual) = self.composite_geometry else {
            return false;
        };
        actual.world_to_screen =
            Affine::translation(actual.canvas_bounds.origin.x, actual.canvas_bounds.origin.y)
                * Affine::translation(self.editor.viewport.pan.x, self.editor.viewport.pan.y)
                * Affine::scale(self.editor.viewport.zoom, self.editor.viewport.zoom);
        actual.device_scale = window.scale_factor();
        presentation::view_matches(
            &token.key,
            actual,
            window.window_handle().window_id().as_u64(),
        )
    }
    fn semantic_activate(
        &mut self,
        token: Activation,
        action: SemanticAction,
        source: SemanticSource,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.semantic_current(&token, action, window) {
            return;
        }
        let dispatch = match self.router.dispatch_semantic(
            &mut self.editor,
            &mut self.interactions,
            token.identity.handle,
            action,
            source,
        ) {
            Ok(report) => report,
            Err(error) => {
                self.interaction_error = Some(error.to_string());
                cx.notify();
                return;
            }
        };
        self.apply_routed_report(dispatch.report, cx);
        let Some(permit) = dispatch.default else {
            return;
        };
        // Listener queues can remove/recreate/mount, load, lock or change policy.
        if !self.semantic_current(&token, action, window) {
            return;
        }
        match self.focus_element_with_semantic(token.identity.handle, Some(permit), window, cx) {
            Ok(true) if self.semantic_current(&token, action, window) => {
                if action == SemanticAction::OpenText {
                    self.accessibility.resume = Some(token.identity);
                    if let Err(error) = self.open_text_editor(token.identity.handle, window, cx) {
                        self.accessibility.resume = None;
                        self.interaction_error = Some(error.to_string());
                    }
                }
            }
            Err(error) => self.interaction_error = Some(error.to_string()),
            _ => {}
        }
        cx.notify();
    }
    pub(super) fn semantic_text_closed(&mut self, restored_to_canvas: bool) {
        if !restored_to_canvas {
            self.accessibility.resume = None;
        }
    }
    pub(super) fn semantic_enter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.focus.is_focused(window) {
            return;
        }
        self.accessibility.entry_pending = self.accessibility_enabled();
        self.complete_semantic_entry(window, cx);
    }
    fn repaint_semantic_entry(window: &mut Window, cx: &mut Context<Self>) {
        // Focus notifications can arrive while GPUI is drawing, after this
        // view's active-descendant/outline was built. notify/refresh in that
        // phase can be absorbed by the current frame. One weak next-frame
        // notification publishes the new focus; consumed entry intent means
        // that repaint cannot schedule another one by itself.
        window.refresh();
        let view = cx.weak_entity();
        window.on_next_frame(move |window, cx| {
            if view.update(cx, |_, cx| cx.notify()).is_ok() {
                window.refresh();
            }
        });
    }
    pub(super) fn complete_semantic_entry(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.accessibility.entry_pending
            || !self.focus.is_focused(window)
            || self.inner_tool_active()
        {
            return;
        }
        if self.focused_element().is_some() {
            self.accessibility.entry_pending = false;
            self.accessibility.resume = None;
            Self::repaint_semantic_entry(window, cx);
            return;
        }
        // Done/Cancel can return native focus before its changed document has
        // published. Retain entry intent until a fresh displayed key exists.
        let Some(key) = &self.accessibility.key else {
            return;
        };
        if !self.presentation_draw_current(key) {
            return;
        }
        self.accessibility.entry_pending = false;
        let resume = self.accessibility.resume.take();
        let target = resume
            .and_then(|id| {
                self.accessibility
                    .nodes
                    .iter()
                    .position(|n| n.identity == id)
            })
            .map(Step::Target)
            .unwrap_or_else(|| {
                next_target(&self.accessibility.nodes, None, window.modifiers().shift)
            });
        if let Step::Target(index) = target
            && let Some(token) = self.accessibility.token(&self.accessibility.nodes[index])
        {
            self.semantic_activate(
                token,
                SemanticAction::Focus,
                SemanticSource::NativeFocus,
                window,
                cx,
            );
            if self.focused_element().is_some() {
                Self::repaint_semantic_entry(window, cx);
            }
        }
    }
    fn semantic_key_action(
        &mut self,
        key: &str,
        reverse: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut modifiers = window.modifiers();
        if key == "tab" {
            modifiers.shift = reverse;
        }
        if !self.focus.is_focused(window)
            || !self.accessibility_enabled()
            || !navigation_key_allowed(
                key,
                modifiers,
                self.inner_tool_active(),
                self.image_crop_editor_active(),
                self.router.space_held(),
            )
        {
            cx.propagate();
            return;
        }
        // GPUI consumes matched actions before raw KeyDown listeners. Normalize
        // the action's actual key into the same scene KeyDown route first; a
        // handled action never reaches the raw listener a second time.
        let allowed = self.plugin_event(
            gpui_flow_interaction::CanvasEvent::KeyDown {
                key: gpui_flow_interaction::KeyCode::Other(key.into()),
                modifiers: gpui_flow_interaction::Modifiers {
                    shift: modifiers.shift,
                    control: modifiers.control,
                    alt: modifiers.alt,
                    meta: modifiers.platform,
                    space: false,
                },
            },
            cx,
        );
        if allowed {
            self.semantic_key_default(key, modifiers, window, cx);
        }
        cx.stop_propagation();
    }
    /// Called only after the existing KeyDown tree/default/queue route permits it.
    pub(super) fn semantic_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let modifiers = event.keystroke.modifiers;
        if !self.accessibility_enabled()
            || !navigation_key_allowed(
                event.keystroke.key.as_str(),
                modifiers,
                self.inner_tool_active(),
                self.image_crop_editor_active(),
                self.router.space_held(),
            )
        {
            return;
        }
        self.semantic_key_default(event.keystroke.key.as_str(), modifiers, window, cx);
    }
    fn semantic_key_default(
        &mut self,
        key: &str,
        modifiers: Modifiers,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match key {
            "tab" => {
                match next_target(
                    &self.accessibility.nodes,
                    self.focused_element(),
                    modifiers.shift,
                ) {
                    Step::Target(index) => {
                        if let Some(token) =
                            self.accessibility.token(&self.accessibility.nodes[index])
                        {
                            self.semantic_activate(
                                token,
                                SemanticAction::Focus,
                                SemanticSource::Keyboard,
                                window,
                                cx,
                            );
                        }
                    }
                    Step::Exit => {
                        let report = self
                            .router
                            .clear_focus(&mut self.editor, &mut self.interactions);
                        self.apply_routed_report(report, cx);
                        self.pending_native_focus = false;
                        if modifiers.shift {
                            window.focus_prev(cx);
                        } else {
                            window.focus_next(cx);
                        }
                    }
                }
                cx.stop_propagation();
            }
            "enter" if !modifiers.shift => {
                if let Some(node) = self
                    .accessibility
                    .nodes
                    .iter()
                    .find(|n| Some(n.identity.handle) == self.focused_element())
                    && let Some(token) = self.accessibility.token(node)
                {
                    let action = if node.text && !node.read_only {
                        SemanticAction::OpenText
                    } else {
                        SemanticAction::Focus
                    };
                    self.semantic_activate(token, action, SemanticSource::Keyboard, window, cx);
                    cx.stop_propagation();
                }
            }
            _ => {}
        }
    }
    pub(super) fn semantic_root(
        &self,
        view: Stateful<Div>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        match &self.accessibility.config {
            Some(config) if !self.is_layer_managed() => view
                .role(Role::GraphicsDocument)
                .aria_label(if config.label.len() <= MAX_LABEL_BYTES {
                    config.label.clone()
                } else {
                    "Scene".into()
                })
                .accessibility_id(format!("flow-scene-{}", self.editor.engine.engine_id()))
                // Stay in the host's tab scope. A new nested group sorts after
                // same-index sibling controls regardless of tree order.
                .when(
                    !self.inner_tool_active() && !self.image_crop_editor_active(),
                    |view| {
                        view.key_context("FlowSceneSemantics")
                            .on_action(cx.listener(|this, _: &NextTarget, window, cx| {
                                this.semantic_key_action("tab", false, window, cx)
                            }))
                            .on_action(cx.listener(|this, _: &PreviousTarget, window, cx| {
                                this.semantic_key_action("tab", true, window, cx)
                            }))
                            .on_action(cx.listener(|this, _: &ActivateTarget, window, cx| {
                                this.semantic_key_action("enter", false, window, cx)
                            }))
                    },
                ),
            _ => view,
        }
    }
    pub(super) fn semantic_elements(&self, cx: &Context<Self>, focused: bool) -> Vec<AnyElement> {
        let mut result = Vec::new();
        let active_serial = self
            .accessibility
            .active_serial(focused, self.focused_element());
        for node in &self.accessibility.nodes {
            let bounds = node.bounds;
            let active = active_serial == Some(node.serial);
            let mut proxy = div()
                .id(("scene-semantic", node.serial))
                .absolute()
                .left(px(bounds.origin.x))
                .top(px(bounds.origin.y))
                .w(px(bounds.size.width))
                .h(px(bounds.size.height))
                .role(node.role)
                .aria_label(node.label.clone())
                .aria_selected(node.selected)
                .accessibility_id(format!(
                    "flow-scene-{}-object-{}",
                    node.identity.handle.engine_id(),
                    node.serial
                ))
                .aria_description(if node.text {
                    "Retained text; Enter opens content editing when editable"
                } else {
                    "Scene object; conservative logical geometry bounds"
                })
                .when(active, |element| element.aria_active_descendant());
            if !node.disabled
                && let Some(token) = self.accessibility.token(node)
            {
                let entity = cx.entity().downgrade();
                proxy = proxy.on_a11y_action(AccessibleAction::Focus, move |_, window, cx| {
                    let _ = entity.update(cx, |this, cx| {
                        this.semantic_activate(
                            token.clone(),
                            SemanticAction::Focus,
                            SemanticSource::AssistiveTechnology,
                            window,
                            cx,
                        )
                    });
                });
                if node.text
                    && !node.read_only
                    && let Some(token) = self.accessibility.token(node)
                {
                    let entity = cx.entity().downgrade();
                    proxy = proxy.on_a11y_action(AccessibleAction::Click, move |_, window, cx| {
                        let _ = entity.update(cx, |this, cx| {
                            this.semantic_activate(
                                token.clone(),
                                SemanticAction::OpenText,
                                SemanticSource::AssistiveTechnology,
                                window,
                                cx,
                            )
                        });
                    });
                }
            }
            result.push(
                SemanticElement {
                    proxy,
                    disabled: node.disabled,
                    read_only: node.text && node.read_only,
                }
                .into_any_element(),
            );
            if active {
                let color = self.theme.accent;
                result.push(
                    canvas(
                        |_, _, _| (),
                        move |bounds, _, window, _| {
                            let mut path = PathBuilder::stroke(px(2.));
                            path.move_to(bounds.origin);
                            path.line_to(bounds.origin + point(bounds.size.width, px(0.)));
                            path.line_to(
                                bounds.origin + point(bounds.size.width, bounds.size.height),
                            );
                            path.line_to(bounds.origin + point(px(0.), bounds.size.height));
                            path.close();
                            if let Ok(path) = path.build() {
                                window.paint_path(path, rgb(color));
                            }
                        },
                    )
                    .absolute()
                    .left(px(bounds.origin.x))
                    .top(px(bounds.origin.y))
                    .w(px(bounds.size.width))
                    .h(px(bounds.size.height))
                    .into_any_element(),
                );
            }
        }
        result
    }
}

/// Delegate lifecycle directly so the proxy's stable ID and a11y action
/// registration use the same actual node. No synthetic nodes or mouse listeners.
struct SemanticElement {
    proxy: Stateful<Div>,
    disabled: bool,
    read_only: bool,
}
impl IntoElement for SemanticElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for SemanticElement {
    type RequestLayoutState = <Stateful<Div> as Element>::RequestLayoutState;
    type PrepaintState = <Stateful<Div> as Element>::PrepaintState;
    fn id(&self) -> Option<ElementId> {
        Element::id(&self.proxy)
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    fn a11y_role(&self) -> Option<Role> {
        self.proxy.a11y_role()
    }
    fn write_a11y_info(&self, node: &mut accesskit::Node) {
        self.proxy.write_a11y_info(node);
        if self.disabled {
            node.set_disabled();
        }
        if self.read_only {
            node.set_read_only();
        }
    }
    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.proxy.request_layout(id, inspector, window, cx)
    }
    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.proxy
            .prepaint(id, inspector, bounds, layout, window, cx)
    }
    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.proxy
            .paint(id, inspector, bounds, layout, prepaint, window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use gpui_flow_composite::{RasterViewport, SceneUnit};
    use gpui_flow_interaction::{
        AccessPolicy,
        scene::{SceneEventRouter, ScenePluginHost, SceneStateStyles},
    };
    use gpui_flow_scene::{Color, DisplayObject, Style, display};

    fn fixture() -> (SceneEditor, ScenePluginHost) {
        let mut scene = Scene::new();
        for (id, x) in [("a", 10.), ("b", 100.)] {
            scene
                .insert(
                    DisplayObject::new(id, display::Rect::new(50., 40.))
                        .with_transform(Affine::translation(x, 20.))
                        .with_style(Style::fill(Color::WHITE)),
                    None,
                    None,
                )
                .unwrap();
        }
        scene
            .insert(
                DisplayObject::new(
                    "text",
                    display::Text::new("Hello", WorldSize::new(90., 30.)),
                )
                .with_transform(Affine::translation(20., 100.))
                .with_style(Style::fill(Color::WHITE)),
                None,
                None,
            )
            .unwrap();
        (
            SceneEditor::new(scene).unwrap(),
            ScenePluginHost::with_defaults(),
        )
    }
    fn frame(editor: &SceneEditor) -> render::StateFrame {
        let mut styles = SceneStateStyles::new(&editor.engine);
        let (frame, error) = render::state_render_frame_with_canvas(
            editor,
            &SceneEventRouter::new(),
            &mut styles,
            std::time::Duration::ZERO,
            None,
            None,
        );
        assert!(error.is_none());
        frame
    }
    fn key(editor: &SceneEditor, frame: &render::StateFrame) -> Arc<RequestKey> {
        let domains = frame
            .units
            .iter()
            .filter_map(|unit| match unit {
                SceneUnit::Domain(domain) => Some((**domain).clone()),
                _ => None,
            })
            .collect();
        Arc::new(RequestKey::new(
            domains,
            &editor.engine,
            RasterViewport {
                world_to_screen: Affine::IDENTITY,
                canvas_bounds: SceneBounds::new(WorldPoint::ZERO, WorldSize::new(300., 200.)),
                content_mask: SceneBounds::new(WorldPoint::ZERO, WorldSize::new(300., 200.)),
                device_scale: 1.,
            },
            7,
        ))
    }
    fn state(editor: &SceneEditor, host: &ScenePluginHost) -> AccessibilityState {
        let mut state = AccessibilityState {
            config: Some(SceneAccessibility::new("Drawing")),
            ..Default::default()
        };
        project(&mut state, editor, host);
        state
    }
    fn project(state: &mut AccessibilityState, editor: &SceneEditor, host: &ScenePluginHost) {
        let frame = frame(editor);
        state.project(&frame, editor.scene(), editor, host, key(editor, &frame));
    }
    #[test]
    fn stable_identity_survives_frames_and_offscreen_roundtrip_but_not_remount_replace_or_reset() {
        let (mut e, h) = fixture();
        let mut s = state(&e, &h);
        let old = s.nodes[0].identity;
        let serial = s.nodes[0].serial;
        project(&mut s, &e, &h);
        assert_eq!(s.nodes[0].serial, serial);
        e.viewport.pan.x = 1000.;
        project(&mut s, &e, &h);
        assert!(s.nodes.is_empty());
        e.viewport.pan.x = 0.;
        project(&mut s, &e, &h);
        assert_eq!(s.nodes[0].serial, serial);
        let (tree, _) = e.engine.detach(old.handle).unwrap();
        e.engine.attach(&tree, None, Some(0)).unwrap();
        project(&mut s, &e, &h);
        assert!(!old.current(&e.engine));
        assert_ne!(s.nodes[0].serial, serial);
        let mounted = s.nodes[0].identity;
        e.engine.destroy(mounted.handle).unwrap();
        e.engine
            .transaction(|scene| {
                scene.insert(
                    DisplayObject::new("a", display::Rect::new(50., 40.)),
                    None,
                    Some(0),
                )
            })
            .unwrap();
        project(&mut s, &e, &h);
        assert!(!mounted.current(&e.engine));
        assert_ne!(s.nodes[0].identity.handle, mounted.handle);
        let before_reset = s.nodes[0].identity;
        e.engine.reset_scene(e.scene().clone()).unwrap();
        project(&mut s, &e, &h);
        assert!(!before_reset.current(&e.engine));
        assert!(
            s.bindings
                .keys()
                .all(|identity| identity.current(&e.engine))
        );
    }
    #[test]
    fn actual_labels_bounds_selected_disabled_readonly_and_bidirectional_edges() {
        let (mut e, mut h) = fixture();
        e.selection.ids.insert("b".into());
        e.engine
            .transaction(|scene| {
                scene.update_object(&"b".into(), |o| {
                    o.name = "Locked rectangle".into();
                    o.locked = true;
                })
            })
            .unwrap();
        h.set_policy(AccessPolicy::read_only(), &mut e);
        let s = state(&e, &h);
        assert_eq!(
            s.nodes.iter().map(|n| n.label.as_str()).collect::<Vec<_>>(),
            ["a", "Locked rectangle", "Hello"]
        );
        assert_eq!(
            s.nodes[0].bounds,
            SceneBounds::new(WorldPoint::new(10., 20.), WorldSize::new(50., 40.))
        );
        assert!(s.nodes[1].selected && s.nodes[1].disabled);
        assert!(s.nodes[2].read_only && !s.nodes[2].disabled);
        assert_eq!(next_target(&s.nodes, None, false), Step::Target(0));
        assert_eq!(
            next_target(&s.nodes, Some(s.nodes[0].identity.handle), false),
            Step::Target(2)
        );
        assert_eq!(
            next_target(&s.nodes, Some(s.nodes[2].identity.handle), false),
            Step::Exit
        );
        assert_eq!(next_target(&s.nodes, None, true), Step::Target(2));
        assert_eq!(
            next_target(&s.nodes, Some(s.nodes[2].identity.handle), true),
            Step::Target(0)
        );
        assert_eq!(
            next_target(&s.nodes, Some(s.nodes[0].identity.handle), true),
            Step::Exit
        );
        h.set_policy(AccessPolicy::locked(), &mut e);
        assert!(!live_policy(
            &e,
            &h,
            s.nodes[0].identity,
            SemanticAction::Focus
        ));
        assert!(!live_policy(
            &e,
            &h,
            s.nodes[2].identity,
            SemanticAction::OpenText
        ));
    }
    #[test]
    fn hidden_zero_opacity_offscreen_and_fully_clipped_objects_are_absent() {
        let (mut e, h) = fixture();
        e.engine
            .transaction(|scene| {
                scene.update_object(&"a".into(), |o| o.visible = false)?;
                scene.update_object(&"b".into(), |o| o.style.opacity = 0.)?;
                scene.update_object(&"text".into(), |o| o.transform.tx = 400.)
            })
            .unwrap();
        assert!(state(&e, &h).nodes.is_empty());
        let (e, h) = fixture();
        let mut f = frame(&e);
        for object in &mut f.objects {
            object.clips.push(gpui_flow_scene::ClipRegion {
                local_bounds: SceneBounds::new(
                    WorldPoint::new(250., 150.),
                    WorldSize::new(20., 20.),
                ),
                corner_radius: 0.,
                corner_radii: None,
                world_transform: Affine::IDENTITY,
            });
        }
        let mut s = state(&e, &h);
        s.project(&f, e.scene(), &e, &h, key(&e, &f));
        assert!(s.nodes.is_empty());
    }
    #[test]
    fn rejected_native_and_complex_clip_targets_never_become_actionable() {
        let (e, h) = fixture();
        let mut f = frame(&e);
        f.units
            .retain(|unit| !matches!(unit, SceneUnit::Native(id) if id.as_str() == "a"));
        f.units
            .push(SceneUnit::Rejected(gpui_flow_composite::CompositeIssue {
                object: "a".into(),
                error: gpui_flow_composite::CompositeError::StaleFrame,
            }));
        let b = f
            .objects
            .iter_mut()
            .find(|o| o.object.id.as_str() == "b")
            .unwrap();
        b.clips.push(gpui_flow_scene::ClipRegion {
            local_bounds: SceneBounds::new(WorldPoint::ZERO, WorldSize::new(200., 200.)),
            corner_radius: 20.,
            corner_radii: None,
            world_transform: Affine::IDENTITY,
        });
        let text = f
            .objects
            .iter_mut()
            .find(|o| o.object.id.as_str() == "text")
            .unwrap();
        text.screen_transform = Affine::rotation(30.);
        let mut s = state(&e, &h);
        s.project(&f, e.scene(), &e, &h, key(&e, &f));
        assert!(s.nodes.is_empty());
        assert!(
            s.issues
                .contains(&SceneAccessibilityIssue::UnsupportedVisibility("b".into()))
        );
        assert!(
            s.issues
                .contains(&SceneAccessibilityIssue::UnsupportedNative("text".into()))
        );
    }
    #[test]
    fn budgets_withhold_the_whole_projection_and_revoke_old_configuration_tokens() {
        let (mut e, h) = fixture();
        let mut s = state(&e, &h);
        let old_generation = s.generation.clone();
        s.generation = Arc::new(());
        assert!(!Arc::ptr_eq(&old_generation, &s.generation));
        // Even zero-sized Arc payloads retain unique allocation identity.
        let next = Arc::new(());
        assert!(!Arc::ptr_eq(&s.generation, &next));
        e.engine
            .transaction(|scene| {
                scene.update_object(&"text".into(), |o| o.name = "x".repeat(MAX_LABEL_BYTES))
            })
            .unwrap();
        project(&mut s, &e, &h);
        assert!(s.nodes.is_empty() && s.key.is_none());
        assert_eq!(s.issues, [SceneAccessibilityIssue::BudgetExceeded]);
        let mut f = frame(&e);
        f.objects = vec![f.objects[0].clone(); MAX_NODES + 1];
        s.project(&f, e.scene(), &e, &h, key(&e, &f));
        assert!(s.nodes.is_empty());
        assert_eq!(s.issues, [SceneAccessibilityIssue::BudgetExceeded]);
    }
    #[test]
    fn ordinary_domain_members_are_projected_but_mask_domain_is_diagnostic_only() {
        let (mut e, h) = fixture();
        e.engine
            .transaction(|scene| {
                scene.update_object(&"a".into(), |o| {
                    o.style.blend_mode = gpui_flow_scene::BlendMode::Normal
                })
            })
            .unwrap();
        let s = state(&e, &h);
        assert_eq!(s.nodes.len(), 3);
        let mut scene = Scene::new();
        scene
            .insert(
                DisplayObject::new("masked-group", display::Group::new()),
                None,
                None,
            )
            .unwrap();
        let mut mask = DisplayObject::new("a", display::Rect::new(50., 40.))
            .with_style(Style::fill(Color::WHITE));
        mask.mask = MaskMode::Path;
        scene
            .insert(mask, Some("masked-group".into()), None)
            .unwrap();
        scene
            .insert(
                DisplayObject::new("masked-content", display::Rect::new(30., 20.))
                    .with_style(Style::fill(Color::WHITE)),
                Some("masked-group".into()),
                None,
            )
            .unwrap();
        e.engine.replace_scene(scene).unwrap();
        let s = state(&e, &h);
        assert!(s.nodes.is_empty());
        assert!(
            s.issues
                .contains(&SceneAccessibilityIssue::UnsupportedVisibility("a".into()))
        );
        assert!(
            s.issues
                .contains(&SceneAccessibilityIssue::UnsupportedVisibility(
                    "masked-content".into()
                ))
        );
        // DomainSource is the accepted membership authority, not all canonical IDs.
        let f = render::StateFrame {
            objects: frame(&e).objects,
            units: vec![],
            resolved_scene: None,
        };
        let mut s = s;
        s.project(&f, e.scene(), &e, &h, key(&e, &f));
        assert!(s.nodes.is_empty());
    }
    #[test]
    fn key_gate_yields_to_text_pan_and_platform_shortcuts() {
        let plain = Modifiers::default();
        assert!(navigation_key_allowed("tab", plain, false, false, false));
        assert!(navigation_key_allowed(
            "tab",
            Modifiers {
                shift: true,
                ..plain
            },
            false,
            false,
            false
        ));
        assert!(navigation_key_allowed("enter", plain, false, false, false));
        for key in ["space", " ", "a", "escape"] {
            assert!(!navigation_key_allowed(key, plain, false, false, false));
        }
        for modifier in [
            Modifiers {
                control: true,
                ..plain
            },
            Modifiers { alt: true, ..plain },
            Modifiers {
                platform: true,
                ..plain
            },
        ] {
            assert!(!navigation_key_allowed(
                "tab", modifier, false, false, false
            ));
            assert!(!navigation_key_allowed(
                "enter", modifier, false, false, false
            ));
        }
        for (inner, crop, space) in [
            (true, false, false),
            (false, true, false),
            (false, false, true),
        ] {
            assert!(!navigation_key_allowed("tab", plain, inner, crop, space));
            assert!(!navigation_key_allowed("enter", plain, inner, crop, space));
        }
    }
    #[test]
    fn disabled_reenabled_projection_and_queued_lock_cannot_reuse_an_old_activation() {
        let (mut e, h) = fixture();
        let mut s = state(&e, &h);
        let token = s.token(&s.nodes[0]).unwrap();
        assert!(token.matches_projection(&s));
        s.configure(None);
        assert!(!token.matches_projection(&s));
        s.configure(Some(SceneAccessibility::new("Drawing")));
        project(&mut s, &e, &h);
        assert!(!token.matches_projection(&s));
        let token = s.token(&s.nodes[0]).unwrap();
        let mut router = SceneEventRouter::new();
        let sender = router.action_sender();
        e.engine
            .on(
                token.identity.handle,
                EventKind::SemanticAction,
                gpui_flow_scene::ListenerOptions::default(),
                move |_| {
                    sender
                        .enqueue(
                            token.identity.handle,
                            gpui_flow_interaction::scene::SceneCommand::SetProperty {
                                id: "a".into(),
                                key: gpui_flow_engine::PropertyKey::Locked,
                                value: serde_json::json!(true),
                            },
                        )
                        .unwrap();
                },
            )
            .unwrap();
        let mut h = h;
        let dispatch = router
            .dispatch_semantic(
                &mut e,
                &mut h,
                token.identity.handle,
                SemanticAction::Focus,
                SemanticSource::AssistiveTechnology,
            )
            .unwrap();
        assert!(dispatch.report.errors.is_empty());
        assert!(!live_policy(&e, &h, token.identity, SemanticAction::Focus));
        // The view checks this policy and presentation revision before using
        // the continuation. Focus and selection remain unchanged here.
        assert_eq!(router.focused(&e.engine), None);
        assert!(e.selection.ids.is_empty());
    }
}
