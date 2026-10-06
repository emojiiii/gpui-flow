use crate::*;
use gpui_flow_core::*;
use std::{collections::BTreeSet, fmt};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InteractionError {
    CapabilityDenied(Capability),
    UnknownPlugin(String),
    DuplicatePlugin(String),
    InvalidPluginId,
    GestureOwned(String),
    InvalidInput(String),
    PluginRejected { plugin: String, reason: String },
    Graph(GraphError),
}
impl fmt::Display for InteractionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CapabilityDenied(c) => write!(f, "canvas capability denied: {c:?}"),
            Self::UnknownPlugin(id) => write!(f, "plugin not installed: {id}"),
            Self::DuplicatePlugin(id) => write!(f, "plugin already installed: {id}"),
            Self::InvalidPluginId => write!(f, "plugin id cannot be empty"),
            Self::GestureOwned(id) => write!(f, "active gesture belongs to {id}"),
            Self::InvalidInput(message) => message.fmt(f),
            Self::PluginRejected { plugin, reason } => write!(f, "plugin {plugin}: {reason}"),
            Self::Graph(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for InteractionError {}
impl From<GraphError> for InteractionError {
    fn from(value: GraphError) -> Self {
        Self::Graph(value)
    }
}
#[derive(Debug, Default, Clone)]
pub struct DispatchReport {
    pub effects: Vec<CanvasEffect>,
    pub errors: Vec<InteractionError>,
    pub handled: bool,
    pub document_changed: bool,
    pub selection_changed: bool,
    pub viewport_changed: bool,
    pub default_prevented: bool,
    pub propagation_stopped: bool,
}
struct Entry {
    id: String,
    enabled: bool,
    order: u64,
    plugin: Box<dyn CanvasPlugin>,
}
struct Capture {
    owner: String,
    pointer_id: u64,
    device: crate::PointerDevice,
    button: Option<PointerButton>,
    selection: Selection,
    viewport: Viewport,
}
/// Plugin host deliberately does not expose mutable editor state to plugins.
/// A trusted application can still access its own Editor directly; read-only
/// guarantees apply to all commands/events routed through this host.
pub struct PluginHost {
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
    event_device: crate::PointerDevice,
}
impl Default for PluginHost {
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
            event_device: crate::PointerDevice::Mouse,
        }
    }
}
impl PluginHost {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn with_defaults() -> Self {
        let mut host = Self::default();
        plugins::install_defaults(&mut host).expect("unique builtin plugin IDs");
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
        mut plugin: impl CanvasPlugin + 'static,
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
        plugin: impl CanvasPlugin + 'static,
        editor: &mut Editor,
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
        editor: &mut Editor,
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
        editor: &mut Editor,
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
    pub fn set_policy(&mut self, policy: AccessPolicy, editor: &mut Editor) -> DispatchReport {
        let mut report = DispatchReport::default();
        if self.policy != policy {
            self.cancel_active(editor, &mut report);
            self.policy = policy;
        }
        report
    }
    pub fn set_editable(&mut self, editable: bool, editor: &mut Editor) -> DispatchReport {
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
    fn cancel_active(&mut self, editor: &mut Editor, report: &mut DispatchReport) {
        let before = editor.graph().clone();
        let selection = editor.selection.clone();
        let viewport = editor.viewport;
        if self.gesture_owner.take().is_some()
            && editor.transaction_active()
            && let Err(error) = editor.cancel_transaction()
        {
            report.errors.push(error.into());
        }
        if let Some(capture) = self.capture.take() {
            editor.selection = capture.selection;
            let graph = editor.graph().clone();
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
        if &before != editor.graph() {
            self.effect(CanvasEffect::DocumentChanged, report);
        }
        if selection != editor.selection {
            self.effect(CanvasEffect::SelectionChanged, report);
        }
        if viewport != editor.viewport {
            self.effect(CanvasEffect::ViewportChanged, report);
        }
        self.effect(CanvasEffect::ConnectionChanged, report);
        self.effect(CanvasEffect::Marquee(None), report);
        self.effect(CanvasEffect::Guides(vec![]), report);
        self.effect(CanvasEffect::Cursor(CanvasCursor::Default), report);
        report.handled = true;
    }
    pub fn cancel(&mut self, editor: &mut Editor) -> DispatchReport {
        let mut report = DispatchReport::default();
        self.cancel_active(editor, &mut report);
        report
    }
    pub fn dispatch(&mut self, editor: &mut Editor, mut event: CanvasEvent) -> DispatchReport {
        let mut report = DispatchReport::default();
        if let CanvasEvent::PointerCancelFor(identity) = &event {
            if self.capture.as_ref().is_none_or(|capture| {
                capture.pointer_id != identity.pointer_id || capture.device != identity.device
            }) {
                return report;
            }
            event = CanvasEvent::PointerCancel;
        }

        if !editor.viewport.is_valid() {
            self.cancel_active(editor, &mut report);
            report.errors.push(InteractionError::InvalidInput(
                "invalid editor viewport".into(),
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
            let mut output = PluginOutput::default();
            {
                let context = PluginContext {
                    editor,
                    policy: self.policy,
                    transforms: TransformCapabilities::default(),
                    connection: &self.connection,
                    capture_owner: self.capture_owner(),
                };
                // Split fields explicitly so plugin state is mutable while the
                // context remains a read-only view of editor and overlays.
                let capture_owner = context.capture_owner.map(str::to_owned);
                let context = PluginContext {
                    editor,
                    policy: self.policy,
                    transforms: TransformCapabilities::default(),
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
        editor: &mut Editor,
        command: CanvasCommand,
    ) -> Result<DispatchReport, InteractionError> {
        let mut report = DispatchReport::default();
        self.process(editor, "host", command, &mut report)?;
        Ok(report)
    }
    fn process(
        &mut self,
        editor: &mut Editor,
        owner: &str,
        mut command: CanvasCommand,
        report: &mut DispatchReport,
    ) -> Result<(), InteractionError> {
        if let Some(capability) = command.capability()
            && !self.allows(capability)
        {
            return Err(InteractionError::CapabilityDenied(capability));
        }
        let mut effects = vec![];
        let mut participants = vec![];
        let cleanup = matches!(
            command,
            CanvasCommand::CancelGesture
                | CanvasCommand::ReleasePointer
                | CanvasCommand::ClearConnection
        );
        for entry in &mut self.plugins {
            if !entry.enabled || cleanup {
                continue;
            }
            let before = command.clone();
            let context = PluginContext {
                editor,
                policy: self.policy,
                transforms: TransformCapabilities::default(),
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
        let graph = editor.graph().clone();
        let selection = editor.selection.clone();
        let viewport = editor.viewport;
        self.apply(editor, owner, command, report)?;
        if self.capture.is_some() || self.gesture_owner.is_some() {
            self.participants.extend(participants);
        }
        for effect in effects {
            self.effect(effect, report);
        }
        if &graph != editor.graph() {
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
        editor: &mut Editor,
        owner: &str,
        command: CanvasCommand,
        report: &mut DispatchReport,
    ) -> Result<(), InteractionError> {
        match command {
            CanvasCommand::CapturePointer { pointer_id } => {
                if let Some(active) = self.capture_owner() {
                    if active != owner {
                        return Err(InteractionError::GestureOwned(active.into()));
                    }
                } else {
                    self.capture = Some(Capture {
                        owner: owner.into(),
                        pointer_id,
                        device: self.event_device,
                        button: self.event_button,
                        selection: editor.selection.clone(),
                        viewport: editor.viewport,
                    });
                }
            }
            CanvasCommand::ReleasePointer => {
                if self.capture_owner() == Some(owner) {
                    if self.gesture_owner.as_deref() == Some(owner) {
                        self.cancel_active(editor, report);
                        return Ok(());
                    }
                    self.capture = None;
                    self.participants.clear();
                }
            }
            CanvasCommand::BeginGesture { label } => {
                if self.gesture_owner.as_deref() != Some(owner) {
                    if editor.transaction_active() {
                        return Err(InteractionError::GestureOwned(
                            self.gesture_owner
                                .clone()
                                .unwrap_or_else(|| "external editor transaction".into()),
                        ));
                    }
                    editor.begin_transaction(label)?;
                    self.gesture_owner = Some(owner.into());
                }
            }
            CanvasCommand::CommitGesture => {
                if self.gesture_owner.as_deref() == Some(owner) {
                    editor.commit_transaction()?;
                    self.gesture_owner = None;
                    self.participants.clear();
                }
            }
            CanvasCommand::CancelGesture => self.cancel_active(editor, report),
            CanvasCommand::SelectNode { node, mode } => {
                if !editor.graph().nodes.contains_key(&node) {
                    return Err(GraphError::MissingNode(node).into());
                }
                if !gpui_flow_core::node_is_visible(editor.graph(), &node) {
                    return Err(InteractionError::InvalidInput("node is hidden".into()));
                }
                match mode {
                    SelectionMode::Replace => editor.selection.select_node(node, false),
                    SelectionMode::Add => editor.selection.select_node(node, true),
                    SelectionMode::Toggle => editor.selection.toggle_node(node),
                }
            }
            CanvasCommand::SelectEdge { edge, mode } => {
                if !editor.graph().edges.contains_key(&edge) {
                    return Err(GraphError::MissingEdge(edge).into());
                }
                if editor
                    .graph()
                    .edges
                    .get(&edge)
                    .is_some_and(|e| !gpui_flow_core::edge_is_visible(editor.graph(), e))
                {
                    return Err(InteractionError::InvalidInput("edge is hidden".into()));
                }
                if mode == SelectionMode::Replace {
                    editor.selection.clear();
                }
                if mode != SelectionMode::Toggle || !editor.selection.edges.remove(&edge) {
                    editor.selection.edges.insert(edge);
                }
            }
            CanvasCommand::SelectGroup { group, mode } => {
                if !editor.graph().groups.contains_key(&group) {
                    return Err(GraphError::MissingGroup(group).into());
                }
                if mode == SelectionMode::Replace {
                    editor.selection.clear();
                }
                if mode != SelectionMode::Toggle || !editor.selection.groups.remove(&group) {
                    editor.selection.groups.insert(group);
                }
            }
            CanvasCommand::SelectAll => editor.select_all(),
            CanvasCommand::ClearSelection => editor.selection.clear(),
            CanvasCommand::SelectRect {
                rect,
                additive,
                fully_contained,
            } => editor.select_rect(rect, additive, fully_contained),
            CanvasCommand::SetPositions { nodes, groups } => {
                editor.transaction("Move selection", move |graph| {
                    for (id, position) in nodes {
                        let node = graph
                            .nodes
                            .get_mut(&id)
                            .ok_or_else(|| GraphError::MissingNode(id.clone()))?;
                        if node.locked {
                            return Err(GraphError::Locked(id.to_string()));
                        }
                        node.position = position;
                    }
                    for (id, position) in groups {
                        let group = graph
                            .groups
                            .get_mut(&id)
                            .ok_or_else(|| GraphError::MissingGroup(id.clone()))?;
                        if group.locked {
                            return Err(GraphError::Locked(id.to_string()));
                        }
                        group.bounds.origin = position;
                    }
                    Ok(())
                })?
            }
            CanvasCommand::ResizeNode { node, size } => editor.resize_node(&node, size)?,
            CanvasCommand::Pan { delta } => {
                if !editor.viewport.pan_by(delta) {
                    return Err(InteractionError::InvalidInput("invalid pan delta".into()));
                }
            }
            CanvasCommand::Zoom { position, zoom } => {
                if !editor.viewport.zoom_at(position, zoom) {
                    return Err(InteractionError::InvalidInput("invalid zoom".into()));
                }
            }
            CanvasCommand::Fit { canvas, padding } => {
                editor.fit_to_content(canvas, padding);
            }
            CanvasCommand::PreviewConnection {
                from,
                position,
                hovered,
            } => {
                if !gpui_flow_core::node_is_visible(editor.graph(), &from.node) {
                    return Err(InteractionError::InvalidInput(
                        "connection source is hidden".into(),
                    ));
                }
                let hovered =
                    hovered.filter(|p| gpui_flow_core::node_is_visible(editor.graph(), &p.node));
                if self.connection.source() != Some(&from) {
                    self.connection.begin(editor.graph(), from, position)?;
                }
                self.connection
                    .move_to_exact(editor.graph(), position, hovered, true);
                self.connection_owner = Some(owner.into());
                self.effect(CanvasEffect::ConnectionChanged, report);
                let cursor =
                    if self.connection.hovered().is_some() && self.connection.target().is_none() {
                        CanvasCursor::NotAllowed
                    } else {
                        CanvasCursor::Crosshair
                    };
                self.effect(CanvasEffect::Cursor(cursor), report);
            }
            CanvasCommand::CompleteConnection { from } => {
                if self.connection.source() != Some(&from) {
                    return Err(InteractionError::InvalidInput(
                        "connection source changed during gesture".into(),
                    ));
                }
                let pair = self.connection.release_exact(editor.graph());
                self.connection_owner = None;
                self.effect(CanvasEffect::ConnectionChanged, report);
                match pair {
                    Ok(Some(pair)) => {
                        if !gpui_flow_core::node_is_visible(editor.graph(), &pair.source.node)
                            || !gpui_flow_core::node_is_visible(editor.graph(), &pair.target.node)
                        {
                            return Err(InteractionError::InvalidInput(
                                "connection endpoint is hidden".into(),
                            ));
                        }
                        let id = editor.graph().fresh_id("edge");
                        editor.transaction("Connect ports", move |graph| {
                            graph.connect(id, pair.source, pair.target)
                        })?;
                    }
                    Ok(None) => {}
                    Err(error) => {
                        self.effect(CanvasEffect::ConnectionRejected(error.to_string()), report);
                        return Err(error.into());
                    }
                }
            }
            CanvasCommand::ClearConnection => {
                self.connection.cancel();
                self.connection_owner = None;
                self.effect(CanvasEffect::ConnectionChanged, report);
            }
            CanvasCommand::AddNode(node) => {
                editor.transaction("Add node", move |graph| graph.add_node(node))?
            }
            CanvasCommand::DeleteSelection => editor.delete_selection()?,
            CanvasCommand::GroupSelection { title, padding } => {
                editor.group_selection(title, padding)?;
            }
            CanvasCommand::UngroupSelection => editor.ungroup_selection()?,
            CanvasCommand::CopySelection => {
                self.effect(CanvasEffect::Clipboard(editor.copy_selection()), report)
            }
            CanvasCommand::Paste { clipboard, offset } => {
                editor.paste(&clipboard, offset)?;
            }
            CanvasCommand::Duplicate { offset } => {
                editor.duplicate_selection(offset)?;
            }
            CanvasCommand::Undo => {
                editor.undo();
            }
            CanvasCommand::Redo => {
                editor.redo();
            }
            CanvasCommand::SetNodeData { node, data } => {
                editor.transaction("Edit node", move |graph| {
                    let target = graph
                        .nodes
                        .get_mut(&node)
                        .ok_or_else(|| GraphError::MissingNode(node.clone()))?;
                    if target.locked {
                        return Err(GraphError::Locked(node.to_string()));
                    }
                    target.data = data;
                    Ok(())
                })?
            }
            CanvasCommand::LoadDocument(document) => editor.load_document(document)?,
            CanvasCommand::ImportGraph(graph) => {
                editor.transaction("Import graph", move |target| {
                    *target = graph;
                    Ok(())
                })?
            }
        }
        Ok(())
    }
}
