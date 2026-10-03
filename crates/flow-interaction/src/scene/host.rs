mod geometry_tools;
mod image_crop;
mod intrinsic_resize;
mod text_resize;
use super::{SceneCommand, object_editable, selection_bounds};
use crate::*;
use gpui_flow_core::{ConnectionDrag, Rect, Viewport};
use gpui_flow_editor::{SceneEditor, SceneSelection};
use gpui_flow_engine::{ElementHandle, InstanceState};
use gpui_flow_scene::{Affine, DisplayId, SceneError};
use std::collections::BTreeSet;
impl From<SceneError> for InteractionError {
    fn from(error: SceneError) -> Self {
        Self::InvalidInput(format!("scene: {error}"))
    }
}
struct Entry {
    id: String,
    enabled: bool,
    order: u64,
    plugin: Box<dyn CanvasPlugin<SceneEditor, SceneCommand>>,
}
struct Capture {
    owner: String,
    pointer_id: u64,
    device: crate::PointerDevice,
    button: Option<PointerButton>,
    selection: SceneSelection,
    viewport: Viewport,
    target: Option<(ElementHandle, u64, u64)>,
    selection_guard: Option<SelectionCapture>,
}
struct SelectionCapture {
    targets: Vec<(ElementHandle, u64)>,
    engine: u64,
    document: u64,
    accepted_revision: u64,
    // Frozen at pointer-down; short existing lines may grow and return exactly.
    minimum_line_length: Option<f64>,
    geometry_baseline: Option<(Affine, gpui_flow_core::Point)>,
}
/// Plugin host deliberately does not expose mutable editor state to plugins.
/// A trusted application can still access its own Editor directly; read-only
/// guarantees apply to all commands/events routed through this host.
pub struct ScenePluginHost {
    plugins: Vec<Entry>,
    policy: AccessPolicy,
    transforms: TransformCapabilities,
    sequence: u64,
    capture: Option<Capture>,
    gesture_owner: Option<String>,
    participants: BTreeSet<String>,
    connection: ConnectionDrag,
    connection_owner: Option<String>,
    marquee: Option<Rect>,
    guides: Vec<SnapGuide>,
    cursor: CanvasCursor,
    event_button: Option<PointerButton>,
    event_device: crate::PointerDevice,
    navigation_time: std::time::Duration,
    navigation_frame: bool,
}
impl Default for ScenePluginHost {
    fn default() -> Self {
        Self {
            plugins: Vec::new(),
            policy: AccessPolicy::default(),
            transforms: TransformCapabilities::default(),
            sequence: 0,
            capture: None,
            gesture_owner: None,
            participants: BTreeSet::new(),
            connection: ConnectionDrag::default(),
            connection_owner: None,
            marquee: None,
            guides: Vec::new(),
            cursor: CanvasCursor::Default,
            event_button: None,
            event_device: crate::PointerDevice::Mouse,
            navigation_time: std::time::Duration::ZERO,
            navigation_frame: false,
        }
    }
}
impl ScenePluginHost {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn with_defaults() -> Self {
        let mut host = Self::default();
        super::plugins::install_defaults(&mut host).expect("unique builtin plugin IDs");
        host
    }
    pub fn transform_capabilities(&self) -> TransformCapabilities {
        self.transforms
    }
    pub(super) fn enabled_geometry_tool_mode(&self) -> Option<super::GeometryToolMode> {
        self.plugins
            .iter()
            .find(|p| p.enabled && p.id == super::GEOMETRY_TOOLS_PLUGIN)?
            .plugin
            .geometry_tool_mode()
    }
    /// Shared resize overlay/input geometry, including the installed plugin's
    /// intrinsic sizing box. Geometry is taken from the accepted presentation.
    pub fn resize_geometry(
        &self,
        editor: &SceneEditor,
        id: &DisplayId,
        presented: &gpui_flow_engine::SceneEngine,
    ) -> Option<super::SceneResizeGeometry> {
        let geometry = super::SceneResizeGeometry::for_target_with_geometry(
            editor,
            id,
            self.transforms,
            presented,
        )?;
        let plugin = self
            .plugins
            .iter()
            .find(|entry| entry.id == "resize" && entry.enabled)?;
        geometry.with_bounds(
            plugin
                .plugin
                .resize_bounds(editor, &id.0, geometry.bounds)?,
        )
    }
    /// Cancels an active gesture before changing presentation constraints.
    pub fn set_transform_capabilities(
        &mut self,
        capabilities: TransformCapabilities,
        editor: &mut SceneEditor,
    ) -> DispatchReport {
        let mut report = DispatchReport::default();
        if self.transforms != capabilities {
            self.cancel_active(editor, &mut report);
            self.transforms = capabilities;
        }
        report
    }
    pub fn policy(&self) -> AccessPolicy {
        self.policy
    }
    pub fn allows(&self, capability: Capability) -> bool {
        self.policy.allows(capability)
    }
    pub fn connection(&self) -> &ConnectionDrag {
        &self.connection
    }
    pub fn marquee(&self) -> Option<Rect> {
        self.marquee
    }
    pub fn guides(&self) -> &[SnapGuide] {
        &self.guides
    }
    pub fn cursor(&self) -> CanvasCursor {
        self.cursor
    }
    pub fn has_capture(&self) -> bool {
        self.capture.is_some()
    }
    pub fn capture_owner(&self) -> Option<&str> {
        self.capture.as_ref().map(|c| c.owner.as_str())
    }
    pub fn is_enabled(&self, id: &str) -> bool {
        self.plugins.iter().any(|p| p.id == id && p.enabled)
    }
    pub fn plugin_ids(&self) -> Vec<&str> {
        self.plugins.iter().map(|p| p.id.as_str()).collect()
    }
    pub fn install(
        &mut self,
        mut plugin: impl CanvasPlugin<SceneEditor, SceneCommand> + 'static,
    ) -> Result<(), InteractionError> {
        if let Some(owner) = self.capture_owner().or(self.gesture_owner.as_deref()) {
            return Err(InteractionError::GestureOwned(owner.into()));
        }
        let id = plugin.id().to_string();
        if id.trim().is_empty() {
            return Err(InteractionError::InvalidPluginId);
        }
        if self.plugins.iter().any(|p| p.id == id) {
            return Err(InteractionError::DuplicatePlugin(id));
        }
        plugin.on_lifecycle(PluginLifecycle::Installed);
        self.sequence += 1;
        self.plugins.push(Entry {
            id,
            enabled: true,
            order: self.sequence,
            plugin: Box::new(plugin),
        });
        self.sort();
        Ok(())
    }
    fn sort(&mut self) {
        self.plugins.sort_by_key(|p| (p.plugin.priority(), p.order));
    }
    pub fn replace(
        &mut self,
        plugin: impl CanvasPlugin<SceneEditor, SceneCommand> + 'static,
        editor: &mut SceneEditor,
    ) -> Result<DispatchReport, InteractionError> {
        let id = plugin.id().to_string();
        if id.trim().is_empty() {
            return Err(InteractionError::InvalidPluginId);
        }
        let mut cancellation = DispatchReport::default();
        if self.capture.is_some() || self.gesture_owner.is_some() || self.needs_navigation_frame() {
            self.cancel_active(editor, &mut cancellation);
        }
        let report = if self.plugins.iter().any(|p| p.id == id) {
            self.uninstall(&id, editor)?
        } else {
            DispatchReport::default()
        };
        self.install(plugin)?;
        cancellation.effects.extend(report.effects);
        cancellation.errors.extend(report.errors);
        cancellation.handled |= report.handled;
        cancellation.document_changed |= report.document_changed;
        cancellation.selection_changed |= report.selection_changed;
        cancellation.viewport_changed |= report.viewport_changed;
        Ok(cancellation)
    }
    pub fn uninstall(
        &mut self,
        id: &str,
        editor: &mut SceneEditor,
    ) -> Result<DispatchReport, InteractionError> {
        let index = self
            .plugins
            .iter()
            .position(|p| p.id == id)
            .ok_or_else(|| InteractionError::UnknownPlugin(id.into()))?;
        let mut report = DispatchReport::default();
        if self.owns_active(id) || self.needs_navigation_frame() {
            self.cancel_active(editor, &mut report);
        }
        self.plugins[index]
            .plugin
            .on_lifecycle(PluginLifecycle::Uninstalled);
        self.plugins.remove(index);
        Ok(report)
    }
    pub fn set_enabled(
        &mut self,
        id: &str,
        enabled: bool,
        editor: &mut SceneEditor,
    ) -> Result<DispatchReport, InteractionError> {
        let index = self
            .plugins
            .iter()
            .position(|p| p.id == id)
            .ok_or_else(|| InteractionError::UnknownPlugin(id.into()))?;
        let mut report = DispatchReport::default();
        if self.plugins[index].enabled == enabled {
            return Ok(report);
        }
        if self.capture.is_some()
            || self.gesture_owner.is_some()
            || self.needs_navigation_frame()
            || (!enabled && self.owns_active(id))
        {
            self.cancel_active(editor, &mut report);
        }
        self.plugins[index].enabled = enabled;
        self.plugins[index].plugin.on_lifecycle(if enabled {
            PluginLifecycle::Enabled
        } else {
            PluginLifecycle::Disabled
        });
        Ok(report)
    }
    fn owns_active(&self, id: &str) -> bool {
        self.capture_owner() == Some(id)
            || self.gesture_owner.as_deref() == Some(id)
            || self.connection_owner.as_deref() == Some(id)
            || self.participants.contains(id)
    }
    pub fn set_policy(&mut self, policy: AccessPolicy, editor: &mut SceneEditor) -> DispatchReport {
        let mut report = DispatchReport::default();
        if self.policy != policy {
            self.cancel_active(editor, &mut report);
            self.policy = policy;
        }
        report
    }
    pub fn set_editable(&mut self, editable: bool, editor: &mut SceneEditor) -> DispatchReport {
        self.set_policy(
            AccessPolicy {
                editable,
                ..self.policy
            },
            editor,
        )
    }
    fn effect(&mut self, effect: CanvasEffect, report: &mut DispatchReport) {
        match &effect {
            CanvasEffect::DocumentChanged => report.document_changed = true,
            CanvasEffect::SelectionChanged => report.selection_changed = true,
            CanvasEffect::ViewportChanged => report.viewport_changed = true,
            CanvasEffect::Marquee(rect) => self.marquee = *rect,
            CanvasEffect::Guides(guides) => self.guides = guides.clone(),
            CanvasEffect::Cursor(cursor) => self.cursor = *cursor,
            _ => {}
        }
        report.effects.push(effect);
    }
    fn cancel_active(&mut self, editor: &mut SceneEditor, report: &mut DispatchReport) {
        let before = editor.scene().clone();
        let selection = editor.selection.clone();
        let viewport = editor.viewport;
        let stale_target = self.capture_target_stale(editor);
        if stale_target && self.gesture_owner.is_some() {
            let same_document = self.capture.as_ref().is_some_and(|capture| {
                capture.selection_guard.as_ref().map_or_else(
                    || {
                        capture.target.is_some_and(|(target, _, document)| {
                            target.engine_id() == editor.engine.engine_id()
                                && document == editor.engine.document_epoch()
                        })
                    },
                    |guard| {
                        guard.engine == editor.engine.engine_id()
                            && guard.document == editor.engine.document_epoch()
                    },
                )
            });
            if same_document {
                editor.commit_edit();
            } else {
                editor.abandon_edit();
            }
        }
        if self.gesture_owner.take().is_some() && editor.editing() {
            editor.cancel_edit();
        }
        if let Some(capture) = self.capture.take().filter(|_| !stale_target) {
            editor.selection = capture.selection;
            let graph = editor.scene().clone();
            editor.selection.retain_existing(&graph);
            editor.viewport = capture.viewport;
        }
        self.connection.cancel();
        self.connection_owner = None;
        self.participants.clear();
        self.marquee = None;
        self.guides.clear();
        self.cursor = CanvasCursor::Default;
        for entry in &mut self.plugins {
            entry.plugin.on_lifecycle(PluginLifecycle::Cancelled);
        }
        if before != *editor.scene() {
            self.effect(CanvasEffect::DocumentChanged, report);
        }
        if selection != editor.selection {
            self.effect(CanvasEffect::SelectionChanged, report);
        }
        if viewport != editor.viewport {
            self.effect(CanvasEffect::ViewportChanged, report);
        }

        self.effect(CanvasEffect::Marquee(None), report);
        self.effect(CanvasEffect::Guides(vec![]), report);
        self.effect(CanvasEffect::Cursor(CanvasCursor::Default), report);
        report.handled = true;
    }
    pub fn cancel(&mut self, editor: &mut SceneEditor) -> DispatchReport {
        let mut report = DispatchReport::default();
        self.cancel_active(editor, &mut report);
        report
    }
    fn capture_target_stale(&self, editor: &SceneEditor) -> bool {
        // Engine revisions cover every descendant, ancestor transform and
        // topology dependency. No full-tree scan on pointer motion; only our
        // own successful batch advances this accepted-state marker.
        if self.capture.as_ref().is_some_and(|capture| {
            capture.selection_guard.as_ref().is_some_and(|guard| {
                guard.engine != editor.engine.engine_id()
                    || guard.document != editor.engine.document_epoch()
                    || guard.accepted_revision != editor.engine.revision()
                    || capture.selection != editor.selection
            })
        }) {
            return true;
        }
        self.capture
            .as_ref()
            .and_then(|c| c.target)
            .is_some_and(|(target, epoch, document)| {
                editor.engine.document_epoch() != document
                    || !editor.engine.instance(target).is_ok_and(|instance| {
                        instance.state == InstanceState::Attached
                            && instance.attachment_epoch == epoch
                    })
            })
    }
    /// Synchronize event timestamps without moving the displayed viewport.
    /// Use this before input; reserve advance_navigation for actual frames.
    /// Backwards time cancels all active work and reports an error.
    pub fn synchronize_navigation_time(
        &mut self,
        editor: &mut SceneEditor,
        now: std::time::Duration,
    ) -> DispatchReport {
        let mut report = DispatchReport::default();
        if now < self.navigation_time || !editor.viewport.is_valid() {
            self.cancel_active(editor, &mut report);
            report.errors.push(InteractionError::InvalidInput(
                "navigation clock moved backwards or viewport is invalid".into(),
            ));
            return report;
        }
        self.navigation_time = now;
        for entry in &mut self.plugins {
            if entry.enabled {
                entry.plugin.on_time(now);
            }
        }
        report
    }
    /// Advance only on explicit application frames. Time is monotonic since a
    /// fixed origin. Stop scheduling as soon as needs_navigation_frame is false.
    pub fn advance_navigation(
        &mut self,
        editor: &mut SceneEditor,
        now: std::time::Duration,
    ) -> DispatchReport {
        let mut report = self.synchronize_navigation_time(editor, now);
        if !report.errors.is_empty() {
            return report;
        }
        if self.capture_target_stale(editor) {
            self.cancel_active(editor, &mut report);
            return report;
        }
        for index in 0..self.plugins.len() {
            if !self.plugins[index].enabled {
                continue;
            }
            let id = self.plugins[index].id.clone();
            let owner = self.capture_owner().map(str::to_owned);
            let context = PluginContext {
                editor,
                policy: self.policy,
                transforms: self.transforms,
                connection: &self.connection,
                capture_owner: owner.as_deref(),
            };
            let mut output = PluginOutput::default();
            self.plugins[index]
                .plugin
                .on_frame(now, &context, &mut output);
            for command in output.commands {
                // Frames only animate navigation, never document edits or capture.
                if command.capability() != Some(Capability::Navigation) {
                    report.errors.push(InteractionError::InvalidInput(
                        "navigation frames may only emit navigation commands".into(),
                    ));
                    self.cancel_active(editor, &mut report);
                    return report;
                }
                self.navigation_frame = true;
                let result = self.process(editor, &id, command, &mut report);
                self.navigation_frame = false;
                if let Err(error) = result {
                    report.errors.push(error);
                    self.cancel_active(editor, &mut report);
                    return report;
                }
            }
            // No clipboard, fit, or other executable side effect from a frame.
            for effect in output.effects {
                if matches!(
                    effect,
                    CanvasEffect::Cursor(_)
                        | CanvasEffect::Guides(_)
                        | CanvasEffect::ViewportChanged
                ) {
                    self.effect(effect, &mut report);
                }
            }
        }
        report
    }
    pub fn navigation_time(&self) -> std::time::Duration {
        self.navigation_time
    }
    /// Mandatory input cleanup for a completed navigation tail. It never
    /// restores snapshots or cancels an unrelated document/capture owner.
    pub fn stop_navigation_tail(&mut self) -> bool {
        let mut stopped = false;
        for entry in &mut self.plugins {
            if entry.enabled
                && entry.plugin.needs_frame()
                && self
                    .capture
                    .as_ref()
                    .is_none_or(|capture| capture.owner != entry.id)
                && self
                    .gesture_owner
                    .as_ref()
                    .is_none_or(|owner| owner != &entry.id)
            {
                entry.plugin.on_lifecycle(PluginLifecycle::Cancelled);
                stopped = true;
            }
        }
        stopped
    }
    pub fn needs_navigation_frame(&self) -> bool {
        self.plugins
            .iter()
            .any(|entry| entry.enabled && entry.plugin.needs_frame())
    }
    pub fn dispatch(&mut self, editor: &mut SceneEditor, mut event: CanvasEvent) -> DispatchReport {
        let mut report = DispatchReport::default();
        if let CanvasEvent::PointerCancelFor(identity) = &event {
            if self.capture.as_ref().is_none_or(|capture| {
                capture.pointer_id != identity.pointer_id || capture.device != identity.device
            }) {
                return report;
            }
            event = CanvasEvent::PointerCancel;
        }

        if self.needs_navigation_frame()
            && matches!(
                event,
                CanvasEvent::PointerDown(_)
                    | CanvasEvent::Wheel { .. }
                    | CanvasEvent::KeyDown { .. }
            )
        {
            report.handled |= self.stop_navigation_tail();
        }
        if self.capture_target_stale(editor) {
            self.cancel_active(editor, &mut report);
            return report;
        }
        if !editor.viewport.is_valid() {
            self.cancel_active(editor, &mut report);
            report.errors.push(InteractionError::InvalidInput(
                "invalid editor viewport".into(),
            ));
            return report;
        }
        if let CanvasEvent::Wheel {
            position, delta, ..
        } = &event
            && (!position.is_finite() || !delta.is_finite())
        {
            self.cancel_active(editor, &mut report);
            report.errors.push(InteractionError::InvalidInput(
                "wheel coordinates and deltas must be finite".into(),
            ));
            return report;
        }
        self.event_button = event.pointer().and_then(|p| p.button);
        self.event_device = event
            .pointer()
            .map_or(crate::PointerDevice::Mouse, |p| p.device);
        if let Some(pointer) = event.pointer() {
            if !pointer.position.is_finite()
                || pointer
                    .pressure
                    .is_some_and(|p| !p.is_finite() || !(0.0..=1.0).contains(&p))
            {
                self.cancel_active(editor, &mut report);
                report.errors.push(InteractionError::InvalidInput(
                    "pointer coordinates and pressure must be finite; pressure must be in 0..=1"
                        .into(),
                ));
                return report;
            }
            if self
                .capture
                .as_ref()
                .is_some_and(|c| c.pointer_id != pointer.pointer_id || c.device != pointer.device)
            {
                return report;
            }
        }
        let explicit_cancel = matches!(
            event,
            CanvasEvent::PointerCancel
                | CanvasEvent::FocusLost
                | CanvasEvent::KeyDown {
                    key: KeyCode::Escape,
                    ..
                }
        );
        let interrupted = match &event {
            CanvasEvent::PointerMove(p) | CanvasEvent::PointerUp(p) => {
                self.capture.as_ref().is_some_and(|c| p.button != c.button)
            }
            CanvasEvent::PointerDown(_) | CanvasEvent::Wheel { .. } => self.capture.is_some(),
            _ => false,
        };
        if explicit_cancel || interrupted {
            self.cancel_active(editor, &mut report);
            if explicit_cancel
                || matches!(
                    event,
                    CanvasEvent::PointerMove(_) | CanvasEvent::PointerUp(_)
                )
            {
                return report;
            }
        }
        for index in 0..self.plugins.len() {
            if !self.plugins[index].enabled {
                continue;
            }
            let id = self.plugins[index].id.clone();
            if event.pointer().is_some()
                && self.capture_owner().is_some_and(|owner| owner != id)
                && self.plugins[index].plugin.is_default_action()
            {
                continue;
            }
            if report.default_prevented && self.plugins[index].plugin.is_default_action() {
                continue;
            }
            let mut output = PluginOutput::<SceneCommand>::default();
            {
                let context = PluginContext {
                    editor,
                    policy: self.policy,
                    transforms: self.transforms,
                    connection: &self.connection,
                    capture_owner: self.capture_owner(),
                };
                // Split fields explicitly so plugin state is mutable while the
                // context remains a read-only view of editor and overlays.
                let capture_owner = context.capture_owner.map(str::to_owned);
                let context = PluginContext {
                    editor,
                    policy: self.policy,
                    transforms: self.transforms,
                    connection: &self.connection,
                    capture_owner: capture_owner.as_deref(),
                };
                self.plugins[index]
                    .plugin
                    .on_event(&event, &context, &mut output);
            }
            report.default_prevented |= output.prevent_default;
            report.propagation_stopped |= output.stop_propagation;
            let mut failed = false;
            for command in output.commands {
                if let Err(error) = self.process(editor, &id, command, &mut report) {
                    report.errors.push(error);
                    self.cancel_active(editor, &mut report);
                    failed = true;
                    break;
                }
            }
            if failed {
                break;
            }
            for effect in output.effects {
                let allowed = match effect {
                    CanvasEffect::RequestPaste => self.allows(Capability::Edit),
                    CanvasEffect::RequestFit | CanvasEffect::ToggleGrid => {
                        self.allows(Capability::Navigation)
                    }
                    CanvasEffect::RequestCopyDocument | CanvasEffect::RequestSave => {
                        self.allows(Capability::ClipboardRead)
                    }
                    _ => true,
                };
                if allowed {
                    self.effect(effect, &mut report);
                }
            }
            if report.propagation_stopped {
                break;
            }
        }
        if matches!(event, CanvasEvent::PointerUp(_)) && self.capture.is_some() {
            self.cancel_active(editor, &mut report);
        }
        report
    }
    pub fn execute(
        &mut self,
        editor: &mut SceneEditor,
        command: SceneCommand,
    ) -> Result<DispatchReport, InteractionError> {
        let mut report = DispatchReport::default();
        self.process(editor, "host", command, &mut report)?;
        Ok(report)
    }
    fn process(
        &mut self,
        editor: &mut SceneEditor,
        owner: &str,
        mut command: SceneCommand,
        report: &mut DispatchReport,
    ) -> Result<(), InteractionError> {
        if let Some(capability) = command.capability()
            && !self.allows(capability)
        {
            return Err(InteractionError::CapabilityDenied(capability));
        }
        let move_selection = matches!(command, SceneCommand::MoveSelectionTo { .. });
        let session_identity = match &command {
            SceneCommand::SetSessionProperty {
                token, handle, key, ..
            } => Some((*token, *handle, key.clone())),
            _ => None,
        };
        let custom_resize_identity = match &command {
            SceneCommand::ResizeCustomSession { token, handle, .. } => Some((*token, *handle)),
            _ => None,
        };
        let image_crop_identity = match &command {
            SceneCommand::SetImageCrop { target, .. } => Some(*target),
            _ => None,
        };
        let line_identity = match &command {
            SceneCommand::SetLineEndpoint {
                target, endpoint, ..
            } => Some((*target, *endpoint)),
            _ => None,
        };
        let geometry_identity = match &command {
            SceneCommand::SetGeometry { target, edit } => Some((*target, edit.control())),
            _ => None,
        };
        let text_box_identity = match &command {
            SceneCommand::SetTextBoxResize { target, .. } => Some(*target),
            _ => None,
        };
        let text_font_identity = match &command {
            SceneCommand::SetTextFontResize { target, .. } => Some(*target),
            _ => None,
        };
        let resize_identity = match &command {
            SceneCommand::SetResizeTransform { target, .. } => Some(*target),
            _ => None,
        };
        let builtin_resize_identity = match &command {
            SceneCommand::SetBuiltinResize { target, resize } => Some((
                *target,
                resize
                    .child_transforms()
                    .iter()
                    .map(|(id, _)| id.clone())
                    .collect::<Vec<_>>(),
            )),
            _ => None,
        };
        let rotation_batch = matches!(command, SceneCommand::SetRotationTransforms { .. });
        let resize_batch_identity = match &command {
            SceneCommand::SetResizeTransforms { transforms }
            | SceneCommand::SetRotationTransforms { transforms } => Some(
                transforms
                    .iter()
                    .map(|(target, _)| *target)
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        };
        let capture_selection_identity = match &command {
            SceneCommand::CapturePointerForSelection {
                pointer_id,
                targets,
            } => Some((*pointer_id, targets.clone())),
            _ => None,
        };
        let mut effects = vec![];
        let mut participants = vec![];
        let cleanup = matches!(
            command,
            SceneCommand::CancelGesture | SceneCommand::ReleasePointer
        );
        for entry in &mut self.plugins {
            if !entry.enabled || cleanup {
                continue;
            }
            let before = command.clone();
            let context = PluginContext {
                editor,
                policy: self.policy,
                transforms: self.transforms,
                connection: &self.connection,
                capture_owner: self.capture.as_ref().map(|c| c.owner.as_str()),
            };
            entry
                .plugin
                .filter_command(&mut command, &context, &mut effects)
                .map_err(|reason| InteractionError::PluginRejected {
                    plugin: entry.id.clone(),
                    reason,
                })?;
            if command != before {
                participants.push(entry.id.clone());
            }
        }
        if let Some(capability) = command.capability()
            && !self.allows(capability)
        {
            return Err(InteractionError::CapabilityDenied(capability));
        }
        if move_selection && !matches!(command, SceneCommand::MoveSelectionTo { .. }) {
            return Err(InteractionError::InvalidInput(
                "middleware cannot change a selection move command kind".into(),
            ));
        }
        if self.navigation_frame && command.capability() != Some(Capability::Navigation) {
            return Err(InteractionError::InvalidInput(
                "navigation frame middleware cannot emit editing commands".into(),
            ));
        }
        if let Some((token, handle, key)) = session_identity
            && !matches!(&command, SceneCommand::SetSessionProperty { token: actual_token, handle: actual_handle, key: actual_key, .. }
                if *actual_token == token && *actual_handle == handle && *actual_key == key)
        {
            return Err(InteractionError::InvalidInput(
                "middleware cannot retarget an owned edit command".into(),
            ));
        }
        if let Some((token, handle)) = custom_resize_identity
            && !matches!(&command, SceneCommand::ResizeCustomSession { token: actual_token, handle: actual_handle, .. } if *actual_token == token && *actual_handle == handle)
        {
            return Err(InteractionError::InvalidInput(
                "middleware cannot retarget a custom resize session".into(),
            ));
        }
        if let Some(target) = image_crop_identity
            && !matches!(&command,SceneCommand::SetImageCrop {target:actual,..} if *actual==target)
        {
            return Err(InteractionError::InvalidInput(
                "middleware cannot retarget an image crop command".into(),
            ));
        }
        if let Some((target, endpoint)) = line_identity
            && !matches!(&command, SceneCommand::SetLineEndpoint { target: actual, endpoint: actual_endpoint, .. } if *actual == target && *actual_endpoint == endpoint)
        {
            return Err(InteractionError::InvalidInput(
                "middleware cannot retarget a line endpoint command".into(),
            ));
        }
        if let Some((target, control)) = geometry_identity
            && !matches!(&command, SceneCommand::SetGeometry { target: actual, edit } if *actual == target && edit.control() == control)
        {
            return Err(InteractionError::InvalidInput(
                "middleware cannot retarget a geometry control".into(),
            ));
        }
        if let Some(target) = text_box_identity
            && !matches!(&command, SceneCommand::SetTextBoxResize { target: actual, .. } if *actual == target)
        {
            return Err(InteractionError::InvalidInput(
                "middleware cannot retarget a Text box command".into(),
            ));
        }
        if let Some(target) = text_font_identity
            && !matches!(&command, SceneCommand::SetTextFontResize { target: actual, .. } if *actual == target)
        {
            return Err(InteractionError::InvalidInput(
                "middleware cannot retarget a Text font command".into(),
            ));
        }
        if let Some(target) = resize_identity
            && !matches!(&command, SceneCommand::SetResizeTransform { target: actual, .. } if *actual == target)
        {
            return Err(InteractionError::InvalidInput(
                "middleware cannot retarget a resize command".into(),
            ));
        }
        if let Some((target, children)) = &builtin_resize_identity
            && !matches!(&command, SceneCommand::SetBuiltinResize { target: actual, resize } if actual == target && resize.child_transforms().iter().map(|(id, _)| id).eq(children.iter()))
        {
            return Err(InteractionError::InvalidInput(
                "middleware cannot retarget intrinsic resize or its content".into(),
            ));
        }
        if let Some(targets) = &resize_batch_identity
            && !(matches!(&command, SceneCommand::SetResizeTransforms { transforms }
                | SceneCommand::SetRotationTransforms { transforms }
                if transforms.iter().map(|(target, _)| target).eq(targets.iter()))
                && rotation_batch == matches!(command, SceneCommand::SetRotationTransforms { .. }))
        {
            return Err(InteractionError::InvalidInput(
                "middleware cannot change transform batch kind or membership".into(),
            ));
        }
        if let Some((pointer, targets)) = &capture_selection_identity
            && !matches!(&command, SceneCommand::CapturePointerForSelection { pointer_id, targets: actual }
                if pointer_id == pointer && actual == targets)
        {
            return Err(InteractionError::InvalidInput(
                "middleware cannot change selection capture membership".into(),
            ));
        }
        if command.capability() == Some(Capability::Edit)
            && self
                .gesture_owner
                .as_ref()
                .is_some_and(|active| active != owner)
        {
            return Err(InteractionError::GestureOwned(
                self.gesture_owner.clone().unwrap(),
            ));
        }
        if command.capability() == Some(Capability::Edit)
            && self.capture_owner().is_some_and(|active| active != owner)
        {
            return Err(InteractionError::GestureOwned(
                self.capture_owner().unwrap().into(),
            ));
        }
        let graph = editor.scene().clone();
        let selection = editor.selection.clone();
        let viewport = editor.viewport;
        self.apply(editor, owner, command, report)?;
        if self.capture.is_some() || self.gesture_owner.is_some() {
            self.participants.extend(participants);
        }
        for effect in effects {
            self.effect(effect, report);
        }
        if graph != *editor.scene() {
            self.effect(CanvasEffect::DocumentChanged, report);
        }
        if selection != editor.selection {
            self.effect(CanvasEffect::SelectionChanged, report);
        }
        if viewport != editor.viewport {
            self.effect(CanvasEffect::ViewportChanged, report);
        }
        report.handled = true;
        Ok(())
    }
    fn apply(
        &mut self,
        editor: &mut SceneEditor,
        owner: &str,
        command: SceneCommand,
        report: &mut DispatchReport,
    ) -> Result<(), InteractionError> {
        let require_editable = |id: &DisplayId| {
            if object_editable(editor.scene(), id) {
                Ok(())
            } else {
                Err(InteractionError::InvalidInput(format!(
                    "locked, missing, or non-invertible object: {id}"
                )))
            }
        };
        let capabilities = self.transforms;
        let rotation_batch = matches!(command, SceneCommand::SetRotationTransforms { .. });
        match command {
            SceneCommand::CapturePointer { pointer_id }
            | SceneCommand::CapturePointerForTarget { pointer_id, .. } => {
                if let Some(active) = self.capture_owner() {
                    if active != owner {
                        return Err(InteractionError::GestureOwned(active.into()));
                    }
                } else {
                    let target =
                        if let SceneCommand::CapturePointerForTarget { target, .. } = command {
                            let instance = editor.engine.instance(target).map_err(|error| {
                                InteractionError::InvalidInput(error.to_string())
                            })?;
                            if instance.state != InstanceState::Attached {
                                return Err(InteractionError::InvalidInput(
                                    "capture target is detached".into(),
                                ));
                            }
                            Some((
                                target,
                                instance.attachment_epoch,
                                editor.engine.document_epoch(),
                            ))
                        } else {
                            None
                        };
                    self.capture = Some(Capture {
                        owner: owner.into(),
                        pointer_id,
                        device: self.event_device,
                        button: self.event_button,
                        selection: editor.selection.clone(),
                        viewport: editor.viewport,
                        target,
                        selection_guard: None,
                    });
                }
            }
            SceneCommand::CapturePointerForSelection {
                pointer_id,
                targets,
            } => {
                if let Some(active) = self.capture_owner() {
                    return Err(InteractionError::GestureOwned(active.into()));
                }
                let expected: Vec<_> = editor
                    .selection
                    .roots(editor.scene())
                    .iter()
                    .filter_map(|id| editor.engine.handle(id))
                    .collect();
                if editor.selection.ids.is_empty()
                    || targets.is_empty()
                    || targets != expected
                    || editor
                        .selection
                        .ids
                        .iter()
                        .any(|id| editor.scene().get(id).is_none())
                {
                    return Err(InteractionError::InvalidInput(
                        "invalid selection capture".into(),
                    ));
                }
                let mut guarded = Vec::with_capacity(targets.len());
                for target in targets {
                    let instance = editor
                        .engine
                        .instance(target)
                        .map_err(|error| InteractionError::InvalidInput(error.to_string()))?;
                    if instance.state != InstanceState::Attached {
                        return Err(InteractionError::InvalidInput(
                            "detached resize participant".into(),
                        ));
                    }
                    guarded.push((target, instance.attachment_epoch));
                }
                self.capture = Some(Capture {
                    owner: owner.into(),
                    pointer_id,
                    device: self.event_device,
                    button: self.event_button,
                    selection: editor.selection.clone(),
                    viewport: editor.viewport,
                    target: None,
                    selection_guard: Some(SelectionCapture {
                        targets: guarded,
                        engine: editor.engine.engine_id(),
                        document: editor.engine.document_epoch(),
                        accepted_revision: editor.engine.revision(),
                        minimum_line_length: super::SceneLineGeometry::for_selection(editor)
                            .map(|geometry| geometry.minimum_length),
                        geometry_baseline: super::SceneGeometryTools::for_selection(
                            editor,
                            super::GeometryToolMode::Skew,
                            capabilities,
                        )
                        .map(|geometry| (geometry.original, geometry.pivot)),
                    }),
                });
            }
            SceneCommand::ReleasePointer => {
                if self.capture_owner() == Some(owner) {
                    if self.gesture_owner.as_deref() == Some(owner) {
                        self.cancel_active(editor, report);
                    } else {
                        self.capture = None;
                        self.participants.clear();
                    }
                }
            }
            SceneCommand::BeginGesture => {
                if self.gesture_owner.as_deref() != Some(owner) {
                    if !editor.begin_edit() {
                        return Err(InteractionError::GestureOwned("external scene edit".into()));
                    }
                    self.gesture_owner = Some(owner.into());
                }
            }
            SceneCommand::CommitGesture => {
                if self.gesture_owner.as_deref() == Some(owner) {
                    editor.commit_edit();
                    self.gesture_owner = None;
                    self.participants.clear();
                }
            }
            SceneCommand::CancelGesture => self.cancel_active(editor, report),
            SceneCommand::Select { id, mode } => {
                if editor.scene().get(&id).is_none() {
                    return Err(SceneError::MissingNode(id).into());
                }
                match mode {
                    SelectionMode::Replace => editor.selection.select(id, false),
                    SelectionMode::Add => editor.selection.select(id, true),
                    SelectionMode::Toggle => editor.selection.toggle(id),
                }
            }
            SceneCommand::ClearSelection => editor.selection.clear(),
            SceneCommand::SelectAll => {
                let presented = editor.scene().resolved_for_view(editor.viewport.zoom)?;
                editor.selection = if let Some(bounds) = presented.visible_bounds()? {
                    SceneSelection::in_rect(&presented, bounds)?
                } else {
                    SceneSelection::default()
                };
            }
            SceneCommand::SelectRect { rect, additive } => {
                if !rect.is_valid() {
                    return Err(InteractionError::InvalidInput(
                        "invalid marquee bounds".into(),
                    ));
                }
                let presented = editor.scene().resolved_for_view(editor.viewport.zoom)?;
                let selected = SceneSelection::in_rect(&presented, rect)?;
                if additive {
                    editor.selection.ids.extend(selected.ids);
                } else {
                    editor.selection = selected;
                }
            }
            SceneCommand::TranslateSelection { delta } => {
                for id in editor.selection.roots(editor.scene()) {
                    require_editable(&id)?;
                }
                editor.translate_selection(delta)?;
            }
            SceneCommand::MoveSelectionTo { anchor } => {
                if self.capture_target_stale(editor) {
                    return Err(InteractionError::InvalidInput(
                        "stale selection drag".into(),
                    ));
                }
                if !anchor.is_finite() {
                    return Err(InteractionError::InvalidInput("invalid move anchor".into()));
                }
                for id in editor.selection.roots(editor.scene()) {
                    require_editable(&id)?;
                }
                if let Some(bounds) = selection_bounds(editor)? {
                    editor.translate_selection(anchor - bounds.origin)?;
                }
                if let Some(guard) = self
                    .capture
                    .as_mut()
                    .and_then(|c| c.selection_guard.as_mut())
                {
                    guard.accepted_revision = editor.engine.revision();
                }
            }
            SceneCommand::SetTransform { id, transform } => {
                require_editable(&id)?;
                editor.transaction(|scene| {
                    scene.update_object(&id, |object| object.transform = transform)
                })?;
            }
            SceneCommand::SetTextBoxResize {
                target,
                size,
                transform,
            } => {
                self.apply_text_box_resize(editor, target, size, transform)?;
            }
            SceneCommand::SetTextFontResize { target, resize } => {
                self.apply_text_font_resize(editor, target, resize)?;
            }
            SceneCommand::SetBuiltinResize { target, resize } => {
                self.apply_builtin_resize(editor, target, resize)?;
            }
            SceneCommand::SetResizeTransform { target, transform } => {
                if self.capture_target_stale(editor) {
                    return Err(InteractionError::InvalidInput("stale resize target".into()));
                }
                let instance = editor
                    .engine
                    .instance(target)
                    .map_err(|error| InteractionError::InvalidInput(error.to_string()))?;
                if instance.state != InstanceState::Attached {
                    return Err(InteractionError::InvalidInput(
                        "resize target is detached".into(),
                    ));
                }
                if let Some(guard) = self
                    .capture
                    .as_ref()
                    .and_then(|c| c.selection_guard.as_ref())
                    && guard.targets.as_slice() != [(target, instance.attachment_epoch)]
                {
                    return Err(InteractionError::InvalidInput(
                        "resize capture membership changed".into(),
                    ));
                }
                let id = instance.id.clone();
                require_editable(&id)?;
                if super::SceneResizeGeometry::for_target_with_capabilities(
                    editor,
                    &id,
                    capabilities,
                )
                .is_none()
                    || !super::SceneResizeGeometry::accepts_transform_with_capabilities(
                        editor,
                        &id,
                        transform,
                        capabilities,
                    )
                {
                    return Err(InteractionError::InvalidInput(
                        "unsupported resize transform or subtree".into(),
                    ));
                }
                editor.transaction_validated(
                    |scene| scene.update_object(&id, |object| object.transform = transform),
                    |scene| {
                        let node = scene
                            .get(&id)
                            .ok_or_else(|| SceneError::MissingNode(id.clone()))?;
                        if !object_editable(scene, &id)
                            || !super::SceneResizeGeometry::accepts_scene_transform(
                                scene,
                                &id,
                                node.object.transform,
                                capabilities,
                            )
                        {
                            return Err(SceneError::InvalidGeometry(
                                "unsupported derived resize subtree".into(),
                            ));
                        }
                        Ok(())
                    },
                )?;
                if let Some(guard) = self
                    .capture
                    .as_mut()
                    .and_then(|c| c.selection_guard.as_mut())
                {
                    guard.accepted_revision = editor.engine.revision();
                }
            }
            SceneCommand::SetResizeTransforms { transforms }
            | SceneCommand::SetRotationTransforms { transforms } => {
                if rotation_batch
                    && super::SceneRotationGeometry::for_selection_with_capabilities(
                        editor,
                        capabilities,
                    )
                    .is_none()
                {
                    return Err(InteractionError::InvalidInput(
                        "unsupported rotation selection".into(),
                    ));
                }
                if self.capture_target_stale(editor) {
                    return Err(InteractionError::InvalidInput(
                        "stale resize selection".into(),
                    ));
                }
                let expected = editor.selection.roots(editor.scene());
                if editor.selection.ids.len() < if rotation_batch { 1 } else { 2 }
                    || transforms.len() != expected.len()
                    || transforms.is_empty()
                    || editor
                        .selection
                        .ids
                        .iter()
                        .any(|id| editor.scene().get(id).is_none())
                {
                    return Err(InteractionError::InvalidInput(
                        "invalid resize batch membership".into(),
                    ));
                }
                let mut edits = Vec::with_capacity(transforms.len());
                for (index, ((target, transform), id)) in
                    transforms.iter().zip(&expected).enumerate()
                {
                    let instance = editor
                        .engine
                        .instance(*target)
                        .map_err(|error| InteractionError::InvalidInput(error.to_string()))?;
                    if instance.state != InstanceState::Attached || &instance.id != id {
                        return Err(InteractionError::InvalidInput(
                            "stale resize participant".into(),
                        ));
                    }
                    if let Some(guard) = self
                        .capture
                        .as_ref()
                        .and_then(|c| c.selection_guard.as_ref())
                        && guard.targets.get(index) != Some(&(*target, instance.attachment_epoch))
                    {
                        return Err(InteractionError::InvalidInput(
                            "resize attachment changed".into(),
                        ));
                    }
                    require_editable(id)?;
                    if !editor
                        .engine
                        .local_bounds(id)
                        .ok()
                        .flatten()
                        .is_some_and(|bounds| bounds.size.is_positive())
                        || (rotation_batch
                            && !super::SceneRotationGeometry::accepts_subtree(
                                editor.scene(),
                                id,
                                capabilities,
                            ))
                        || !super::SceneResizeGeometry::accepts_scene_transform(
                            editor.scene(),
                            id,
                            *transform,
                            capabilities,
                        )
                    {
                        return Err(InteractionError::InvalidInput(
                            "unsupported resize participant".into(),
                        ));
                    }
                    edits.push((id.clone(), *transform));
                }
                editor.transaction_validated(
                    |scene| {
                        for (id, transform) in &edits {
                            scene.update_object(id, |object| object.transform = *transform)?;
                        }
                        Ok(())
                    },
                    |scene| {
                        // Providers/normalizers run inside this same transaction.
                        // Their final matrices must obey the native subtree
                        // contract too, before any observer sees the candidate.
                        for id in &expected {
                            let node = scene
                                .get(id)
                                .ok_or_else(|| SceneError::MissingNode(id.clone()))?;
                            if !object_editable(scene, id)
                                || (rotation_batch
                                    && !super::SceneRotationGeometry::accepts_subtree(
                                        scene,
                                        id,
                                        capabilities,
                                    ))
                                || !super::SceneResizeGeometry::accepts_scene_transform(
                                    scene,
                                    id,
                                    node.object.transform,
                                    capabilities,
                                )
                            {
                                return Err(SceneError::InvalidGeometry(
                                    "unsupported derived resize subtree".into(),
                                ));
                            }
                        }
                        Ok(())
                    },
                )?;
                if let Some(guard) = self
                    .capture
                    .as_mut()
                    .and_then(|c| c.selection_guard.as_mut())
                {
                    guard.accepted_revision = editor.engine.revision();
                }
            }
            SceneCommand::SetImageCrop { target, crop } => {
                self.apply_image_crop(editor, target, crop)?
            }
            SceneCommand::SetGeometry { target, edit } => {
                self.apply_geometry(editor, target, edit)?
            }
            SceneCommand::SetLineEndpoint {
                target,
                endpoint,
                position,
            } => {
                if self.capture_target_stale(editor) {
                    return Err(InteractionError::InvalidInput("stale line capture".into()));
                }
                let geometry = super::SceneLineGeometry::for_selection(editor)
                    .filter(|geometry| geometry.target == target)
                    .ok_or_else(|| {
                        InteractionError::InvalidInput("unsupported line target".into())
                    })?;
                let instance = editor
                    .engine
                    .instance(target)
                    .map_err(|error| InteractionError::InvalidInput(error.to_string()))?;
                let id = instance.id.clone();
                let guard = self
                    .capture
                    .as_ref()
                    .and_then(|c| c.selection_guard.as_ref());
                if guard.is_some_and(|guard| {
                    guard.targets.as_slice() != [(target, instance.attachment_epoch)]
                }) {
                    return Err(InteractionError::InvalidInput(
                        "line attachment changed".into(),
                    ));
                }
                let minimum = guard
                    .and_then(|guard| guard.minimum_line_length)
                    .unwrap_or(geometry.minimum_length);
                let mut candidate = geometry.line.clone();
                endpoint.set(&mut candidate, position);
                if !super::SceneLineGeometry::world_length(&candidate, geometry.world)
                    .is_some_and(|length| length >= minimum)
                {
                    return Err(InteractionError::InvalidInput(
                        "line endpoint is nonfinite or too short".into(),
                    ));
                }
                let opposite = endpoint.opposite().position(&geometry.line);
                let transform = editor
                    .engine
                    .object(target)
                    .map_err(|error| InteractionError::InvalidInput(error.to_string()))?
                    .transform;
                editor.transaction_validated(
                    |scene| {
                        scene.update_object(&id, |object| {
                            // Never replay a cached whole Shape: only the requested
                            // field changes; style, the opposite end and metadata stay live.
                            if let gpui_flow_scene::Shape::Line(line) = &mut object.shape {
                                endpoint.set(line, position);
                            }
                        })
                    },
                    |scene| {
                        let (line, world) = super::SceneLineGeometry::scene_line(scene, &id)
                            .ok_or_else(|| {
                                SceneError::InvalidGeometry("unsupported derived line".into())
                            })?;
                        if world != geometry.world
                            || scene
                                .get(&id)
                                .is_none_or(|node| node.object.transform != transform)
                            || endpoint.opposite().position(&line) != opposite
                            || endpoint.position(&line) != position
                            || !super::SceneLineGeometry::world_length(&line, world)
                                .is_some_and(|length| length >= minimum)
                        {
                            return Err(SceneError::InvalidGeometry(
                                "derived line changed the endpoint contract".into(),
                            ));
                        }
                        Ok(())
                    },
                )?;
                if let Some(guard) = self
                    .capture
                    .as_mut()
                    .and_then(|c| c.selection_guard.as_mut())
                {
                    guard.accepted_revision = editor.engine.revision();
                }
            }
            SceneCommand::RotateSelection { degrees } => {
                for id in editor.selection.roots(editor.scene()) {
                    require_editable(&id)?;
                }
                editor.rotate_selection(degrees)?;
            }
            SceneCommand::ScaleSelection { factor } => {
                for id in editor.selection.roots(editor.scene()) {
                    require_editable(&id)?;
                }
                editor.scale_selection(factor)?;
            }
            SceneCommand::DeleteSelection => {
                for id in editor.selection.roots(editor.scene()) {
                    require_editable(&id)?;
                }
                editor.delete_selection()?;
            }
            SceneCommand::SetProperty { id, key, value } => {
                require_editable(&id)?;
                let handle = editor
                    .engine
                    .handle(&id)
                    .ok_or(SceneError::MissingNode(id))?;
                editor.set_property(handle, key, value)?;
            }
            SceneCommand::SetSessionProperty {
                token,
                handle,
                key,
                value,
            } => {
                let id = editor
                    .engine
                    .object(handle)
                    .map_err(|error| InteractionError::InvalidInput(error.to_string()))?
                    .id
                    .clone();
                require_editable(&id)?;
                editor
                    .set_edit_session_property(token, handle, key, value)
                    .map_err(|error| InteractionError::InvalidInput(error.to_string()))?;
            }
            SceneCommand::ResizeCustomSession {
                token,
                handle,
                size,
                anchor,
            } => {
                let id = editor
                    .engine
                    .object(handle)
                    .map_err(|e| InteractionError::InvalidInput(e.to_string()))?
                    .id
                    .clone();
                require_editable(&id)?;
                if !editor.is_edit_session_active(token) {
                    return Err(InteractionError::InvalidInput(
                        "custom resize session is stale".into(),
                    ));
                }
                let patch = editor
                    .engine
                    .preview_custom_resize(handle, size, anchor)
                    .map_err(|e| InteractionError::InvalidInput(e.to_string()))?;
                editor
                    .set_edit_session_properties(
                        token,
                        handle,
                        [
                            (
                                gpui_flow_engine::PropertyKey::Shape,
                                serde_json::to_value(patch.shape)
                                    .map_err(|e| InteractionError::InvalidInput(e.to_string()))?,
                            ),
                            (
                                gpui_flow_engine::PropertyKey::Transform,
                                serde_json::to_value(patch.transform)
                                    .map_err(|e| InteractionError::InvalidInput(e.to_string()))?,
                            ),
                        ],
                    )
                    .map_err(|e| InteractionError::InvalidInput(e.to_string()))?;
            }
            SceneCommand::SetObjectData { id, data } => {
                require_editable(&id)?;
                editor.transaction(move |scene| {
                    scene.update_object(&id, |object| object.data = data)
                })?;
            }
            SceneCommand::AddObject {
                object,
                parent,
                index,
            } => {
                if let Some(parent) = &parent {
                    require_editable(parent)?;
                }
                editor.transaction(move |scene| scene.insert(object, parent, index))?;
            }
            SceneCommand::ImportScene(scene) => editor.transaction(move |target| {
                *target = scene;
                Ok(())
            })?,
            SceneCommand::LoadScene(scene) => {
                if editor.gesture_editing() {
                    return Err(InteractionError::GestureOwned("active scene edit".into()));
                }
                editor.load_scene(scene)?;
            }
            SceneCommand::PasteScene { scene, offset } => {
                scene.validate()?;
                if !offset.is_finite() {
                    return Err(InteractionError::InvalidInput(
                        "paste offset must be finite".into(),
                    ));
                }
                let mut inserted = vec![];
                editor.transaction(|target| {
                    let mut stack: Vec<_> = scene
                        .roots()
                        .iter()
                        .rev()
                        .map(|id| (id.clone(), None))
                        .collect();
                    let mut sequence = 1u64;
                    while let Some((id, parent)) = stack.pop() {
                        let node = scene
                            .get(&id)
                            .ok_or_else(|| SceneError::MissingNode(id.clone()))?;
                        let mut object = node.object.clone();
                        let new_id = loop {
                            let id = DisplayId::from(format!("object-{sequence}"));
                            sequence += 1;
                            if target.get(&id).is_none() {
                                break id;
                            }
                        };
                        object.id = new_id.clone();
                        if parent.is_none() {
                            object.transform =
                                Affine::translation(offset.x, offset.y) * object.transform;
                            inserted.push(new_id.clone());
                        }
                        target.insert(object, parent, None)?;
                        for child in node.children.iter().rev() {
                            stack.push((child.clone(), Some(new_id.clone())));
                        }
                    }
                    Ok(())
                })?;
                editor.selection.ids = inserted.into_iter().collect();
            }
            SceneCommand::Undo => {
                editor.undo();
            }
            SceneCommand::Redo => {
                editor.redo();
            }
            SceneCommand::SetViewport { viewport } => {
                if !viewport.is_valid() {
                    return Err(InteractionError::InvalidInput("invalid viewport".into()));
                }
                editor.scene().validate_view(viewport.zoom)?;
                editor.engine.set_stroke_view_scale(viewport.zoom);
                editor.viewport = viewport;
            }
            SceneCommand::Pan { delta } => {
                let mut viewport = editor.viewport;
                if !viewport.pan_by(delta) {
                    return Err(InteractionError::InvalidInput("invalid pan".into()));
                }
                editor.scene().validate_view(viewport.zoom)?;
                editor.engine.set_stroke_view_scale(viewport.zoom);
                editor.viewport = viewport;
            }
            SceneCommand::Zoom { position, zoom } => {
                let mut viewport = editor.viewport;
                if !viewport.zoom_at(position, zoom) {
                    return Err(InteractionError::InvalidInput("invalid zoom".into()));
                }
                editor.scene().validate_view(viewport.zoom)?;
                editor.engine.set_stroke_view_scale(viewport.zoom);
                editor.viewport = viewport;
            }
            SceneCommand::Fit { canvas, padding } => {
                let viewport =
                    super::fit_scene_viewport(editor.scene(), editor.viewport, canvas, padding)?;
                editor.scene().validate_view(viewport.zoom)?;
                editor.engine.set_stroke_view_scale(viewport.zoom);
                editor.viewport = viewport;
            }
        }
        Ok(())
    }
}
