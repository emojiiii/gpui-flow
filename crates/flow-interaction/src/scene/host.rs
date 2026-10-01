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
}
/// Plugin host deliberately does not expose mutable editor state to plugins.
/// A trusted application can still access its own Editor directly; read-only
/// guarantees apply to all commands/events routed through this host.
pub struct ScenePluginHost {
    plugins: Vec<Entry>,
    policy: AccessPolicy,
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
}
impl Default for ScenePluginHost {
    fn default() -> Self {
        Self {
            plugins: Vec::new(),
            policy: AccessPolicy::default(),
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
        if self.capture.is_some() || self.gesture_owner.is_some() {
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
        if self.owns_active(id) {
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
    pub fn dispatch(&mut self, editor: &mut SceneEditor, event: CanvasEvent) -> DispatchReport {
        let mut report = DispatchReport::default();
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
        self.event_button = event.pointer().and_then(|p| p.button);
        if let Some(pointer) = event.pointer() {
            if !pointer.position.is_finite() {
                self.cancel_active(editor, &mut report);
                report.errors.push(InteractionError::InvalidInput(
                    "pointer coordinates must be finite".into(),
                ));
                return report;
            }
            if self
                .capture
                .as_ref()
                .is_some_and(|c| c.pointer_id != pointer.pointer_id)
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
                    connection: &self.connection,
                    capture_owner: self.capture_owner(),
                };
                // Split fields explicitly so plugin state is mutable while the
                // context remains a read-only view of editor and overlays.
                let capture_owner = context.capture_owner.map(str::to_owned);
                let context = PluginContext {
                    editor,
                    policy: self.policy,
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
        let session_identity = match &command {
            SceneCommand::SetSessionProperty {
                token, handle, key, ..
            } => Some((*token, *handle, key.clone())),
            _ => None,
        };
        let resize_identity = match &command {
            SceneCommand::SetResizeTransform { target, .. } => Some(*target),
            _ => None,
        };
        let resize_batch_identity = match &command {
            SceneCommand::SetResizeTransforms { transforms } => Some(
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
        if let Some((token, handle, key)) = session_identity
            && !matches!(&command, SceneCommand::SetSessionProperty { token: actual_token, handle: actual_handle, key: actual_key, .. }
                if *actual_token == token && *actual_handle == handle && *actual_key == key)
        {
            return Err(InteractionError::InvalidInput(
                "middleware cannot retarget an owned edit command".into(),
            ));
        }
        if let Some(target) = resize_identity
            && !matches!(&command, SceneCommand::SetResizeTransform { target: actual, .. } if *actual == target)
        {
            return Err(InteractionError::InvalidInput(
                "middleware cannot retarget a resize command".into(),
            ));
        }
        if let Some(targets) = &resize_batch_identity
            && !matches!(&command, SceneCommand::SetResizeTransforms { transforms }
                if transforms.iter().map(|(target, _)| target).eq(targets.iter()))
        {
            return Err(InteractionError::InvalidInput(
                "middleware cannot change resize batch membership".into(),
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
                if editor.selection.ids.len() < 2
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
                    button: self.event_button,
                    selection: editor.selection.clone(),
                    viewport: editor.viewport,
                    target: None,
                    selection_guard: Some(SelectionCapture {
                        targets: guarded,
                        engine: editor.engine.engine_id(),
                        document: editor.engine.document_epoch(),
                        accepted_revision: editor.engine.revision(),
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
                editor.selection = if let Some(bounds) = editor.scene().visible_bounds()? {
                    SceneSelection::in_rect(editor.scene(), bounds)?
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
                let selected = SceneSelection::in_rect(editor.scene(), rect)?;
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
                if !anchor.is_finite() {
                    return Err(InteractionError::InvalidInput("invalid move anchor".into()));
                }
                for id in editor.selection.roots(editor.scene()) {
                    require_editable(&id)?;
                }
                if let Some(bounds) = selection_bounds(editor)? {
                    editor.translate_selection(anchor - bounds.origin)?;
                }
            }
            SceneCommand::SetTransform { id, transform } => {
                require_editable(&id)?;
                editor.transaction(|scene| {
                    scene.update_object(&id, |object| object.transform = transform)
                })?;
            }
            SceneCommand::SetResizeTransform { target, transform } => {
                let instance = editor
                    .engine
                    .instance(target)
                    .map_err(|error| InteractionError::InvalidInput(error.to_string()))?;
                if instance.state != InstanceState::Attached {
                    return Err(InteractionError::InvalidInput(
                        "resize target is detached".into(),
                    ));
                }
                let id = instance.id.clone();
                require_editable(&id)?;
                if super::SceneResizeGeometry::for_target(editor, &id).is_none()
                    || !super::SceneResizeGeometry::accepts_transform(editor, &id, transform)
                {
                    return Err(InteractionError::InvalidInput(
                        "unsupported resize transform or subtree".into(),
                    ));
                }
                editor.transaction(|scene| {
                    scene.update_object(&id, |object| object.transform = transform)
                })?;
            }
            SceneCommand::SetResizeTransforms { transforms } => {
                if self.capture_target_stale(editor) {
                    return Err(InteractionError::InvalidInput(
                        "stale resize selection".into(),
                    ));
                }
                let expected = editor.selection.roots(editor.scene());
                if editor.selection.ids.len() < 2
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
                        || !super::SceneResizeGeometry::accepts_scene_transform(
                            editor.scene(),
                            id,
                            *transform,
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
                                || !super::SceneResizeGeometry::accepts_scene_transform(
                                    scene,
                                    id,
                                    node.object.transform,
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
            SceneCommand::Pan { delta } => {
                if !editor.viewport.pan_by(delta) {
                    return Err(InteractionError::InvalidInput("invalid pan".into()));
                }
            }
            SceneCommand::Zoom { position, zoom } => {
                if !editor.viewport.zoom_at(position, zoom) {
                    return Err(InteractionError::InvalidInput("invalid zoom".into()));
                }
            }
            SceneCommand::Fit { canvas, padding } => {
                if let Some(bounds) = editor.scene().visible_bounds()?
                    && !editor.viewport.fit(bounds, canvas, padding)
                {
                    return Err(InteractionError::InvalidInput(
                        "invalid fit viewport".into(),
                    ));
                }
            }
        }
        Ok(())
    }
}
