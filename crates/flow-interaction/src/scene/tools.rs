//! Per-editor tool registration and one token-owned inner editing session.
//!
//! Platform-neutral registrations are paired with widget factories by native adapters.
//! Applications implement outer-tool behavior and native text input themselves.
//! No text buffer, caret model, shaping engine, global registration, or keyboard
//! listener lives here. Call `reconcile` before input/render and `dispose` before
//! releasing a controller; dropping Rust state alone cannot access its editor.
use super::{SceneCommand, ScenePluginHost, object_editable};
use crate::{Capability, InteractionError};
use gpui_flow_editor::{EditSessionToken, SceneEditor};
use gpui_flow_engine::{ElementHandle, InstanceState, PropertyKey};
use gpui_flow_scene::Shape;
use std::collections::BTreeMap;

pub const DEFAULT_OUTER_TOOL: &str = "selection";
pub const DEFAULT_TEXT_TOOL: &str = "text";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneToolKind {
    Outer,
    PlainText,
    CustomInner,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneToolRegistration {
    pub id: String,
    pub kind: SceneToolKind,
    /// Inner dispatch uses Shape::tag(); custom element kinds share the `custom` tag.
    pub shape_tag: Option<String>,
}
/// Isolated registry. Duplicate IDs and ambiguous shape registrations error;
/// registration never silently replaces another tool.
#[derive(Debug, Clone)]
pub struct SceneToolRegistry {
    entries: BTreeMap<String, SceneToolRegistration>,
    default_outer: String,
}
impl Default for SceneToolRegistry {
    fn default() -> Self {
        let mut registry = Self {
            entries: BTreeMap::new(),
            default_outer: DEFAULT_OUTER_TOOL.into(),
        };
        registry
            .register_outer(DEFAULT_OUTER_TOOL)
            .expect("unique default");
        registry
    }
}
impl SceneToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn register_outer(&mut self, id: impl Into<String>) -> Result<(), InteractionError> {
        self.insert(SceneToolRegistration {
            id: id.into(),
            kind: SceneToolKind::Outer,
            shape_tag: None,
        })
    }
    /// Plain Text is the currently supported inner editor. A custom element
    /// whose kind happens to be `text` does not qualify.
    pub fn register_text(&mut self, id: impl Into<String>) -> Result<(), InteractionError> {
        self.insert(SceneToolRegistration {
            id: id.into(),
            kind: SceneToolKind::PlainText,
            shape_tag: Some("text".into()),
        })
    }
    /// Register a host-defined inner editor for a canonical shape tag.
    /// A Custom element uses the explicit `custom` tag, never its arbitrary kind.
    pub fn register_inner(
        &mut self,
        id: impl Into<String>,
        shape_tag: impl Into<String>,
    ) -> Result<(), InteractionError> {
        let tag = shape_tag.into();
        if !matches!(
            tag.as_str(),
            "rect"
                | "ellipse"
                | "line"
                | "polygon"
                | "star"
                | "path"
                | "pen"
                | "text"
                | "image"
                | "canvas"
                | "group"
                | "pen_group"
                | "box"
                | "frame"
                | "custom"
        ) {
            return Err(invalid("unknown canonical tool shape tag"));
        }
        self.insert(SceneToolRegistration {
            id: id.into(),
            kind: SceneToolKind::CustomInner,
            shape_tag: Some(tag),
        })
    }
    fn insert(&mut self, entry: SceneToolRegistration) -> Result<(), InteractionError> {
        if entry.id.trim().is_empty() || self.entries.contains_key(&entry.id) {
            return Err(invalid("tool ID is empty or already registered"));
        }
        if entry.shape_tag.as_ref().is_some_and(|tag| {
            self.entries
                .values()
                .any(|other| other.shape_tag.as_ref() == Some(tag))
        }) {
            return Err(invalid(
                "an inner tool is already registered for that shape tag",
            ));
        }
        self.entries.insert(entry.id.clone(), entry);
        Ok(())
    }
    pub fn set_default_outer(&mut self, id: &str) -> Result<(), InteractionError> {
        if !self
            .entries
            .get(id)
            .is_some_and(|tool| tool.kind == SceneToolKind::Outer)
        {
            return Err(invalid("default tool must be a registered outer tool"));
        }
        self.default_outer = id.into();
        Ok(())
    }
    pub fn default_outer(&self) -> &str {
        &self.default_outer
    }
    pub fn get(&self, id: &str) -> Option<&SceneToolRegistration> {
        self.entries.get(id)
    }
    pub fn registrations(&self) -> impl Iterator<Item = &SceneToolRegistration> {
        self.entries.values()
    }
    pub fn inner_for_shape(&self, tag: &str) -> Option<&SceneToolRegistration> {
        self.entries
            .values()
            .find(|tool| tool.shape_tag.as_deref() == Some(tag))
    }
}

/// Opaque runtime ownership. Late callbacks must keep their original token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextSessionToken(EditSessionToken);
/// Shared opaque ownership token; the Text name remains source-compatible.
pub type ToolSessionToken = TextSessionToken;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCloseReason {
    Done,
    Blur,
    TargetSwitch,
    Cancelled,
    Invalidated,
    Disabled,
    Disposed,
    OuterToolSwitch,
    InnerOpened,
    Unregistered,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolLifecycleKind {
    Created,
    Loaded,
    Updated,
    Unloaded(ToolCloseReason),
    Destroyed,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneToolEvent {
    pub tool_id: String,
    pub target: Option<ElementHandle>,
    pub token: Option<TextSessionToken>,
    pub kind: ToolLifecycleKind,
}
struct ToolSession {
    kind: SceneToolKind,
    tool_id: String,
    token: TextSessionToken,
    target: ElementHandle,
    document_epoch: u64,
    attachment_epoch: u64,
    text: String,
}
/// Optional controller; construct one per SceneEditor and explicitly dispose it.
/// Ordinary editor/engine edits revoke the session and retain its last owned
/// change as one undo step, separately from the external change. Escape rolls
/// back only a still-owned baseline (intentionally unlike Leafer's close).
pub struct SceneToolController {
    registry: SceneToolRegistry,
    outer: String,
    outer_loaded: bool,
    bound_engine: Option<u64>,
    active: Option<ToolSession>,
    events: Vec<SceneToolEvent>,
    enabled: bool,
    disposed: bool,
}
impl Default for SceneToolController {
    fn default() -> Self {
        Self::new()
    }
}
impl SceneToolController {
    /// Default outer selection, with no inner editing installed.
    pub fn new() -> Self {
        Self::with_registry(SceneToolRegistry::new())
    }
    pub fn with_text() -> Self {
        let mut registry = SceneToolRegistry::new();
        registry
            .register_text(DEFAULT_TEXT_TOOL)
            .expect("unique text tool");
        Self::with_registry(registry)
    }
    pub fn with_registry(registry: SceneToolRegistry) -> Self {
        let outer = registry.default_outer.clone();
        let mut result = Self {
            registry,
            outer,
            outer_loaded: false,
            bound_engine: None,
            active: None,
            events: vec![],
            enabled: true,
            disposed: false,
        };
        for tool in result.registry.registrations() {
            result.events.push(SceneToolEvent {
                tool_id: tool.id.clone(),
                target: None,
                token: None,
                kind: ToolLifecycleKind::Created,
            });
        }
        result.load_outer();
        result
    }
    pub fn registry(&self) -> &SceneToolRegistry {
        &self.registry
    }
    pub fn outer_tool(&self) -> &str {
        &self.outer
    }
    /// Native adapters may show the built-in handles only while this outer
    /// identity is loaded. Inner editing, disabling and disposal suspend it.
    pub fn outer_tool_active(&self, id: &str) -> bool {
        self.is_enabled() && self.outer_loaded && self.active.is_none() && self.outer == id
    }
    pub fn is_enabled(&self) -> bool {
        self.enabled && !self.disposed
    }
    pub fn active_token(&self) -> Option<TextSessionToken> {
        self.active.as_ref().map(|s| s.token)
    }
    pub fn active_target(&self) -> Option<ElementHandle> {
        self.active.as_ref().map(|s| s.target)
    }
    pub fn active_text(&self) -> Option<&str> {
        self.active
            .as_ref()
            .filter(|s| s.kind == SceneToolKind::PlainText)
            .map(|s| s.text.as_str())
    }
    pub fn active_kind(&self) -> Option<SceneToolKind> {
        self.active.as_ref().map(|s| s.kind)
    }
    pub fn active_tool_id(&self) -> Option<&str> {
        self.active.as_ref().map(|s| s.tool_id.as_str())
    }
    pub fn drain_events(&mut self) -> Vec<SceneToolEvent> {
        std::mem::take(&mut self.events)
    }
    fn outer_event(&mut self, kind: ToolLifecycleKind) {
        self.events.push(SceneToolEvent {
            tool_id: self.outer.clone(),
            target: None,
            token: None,
            kind,
        });
    }
    fn load_outer(&mut self) {
        if self.is_enabled() && !self.outer_loaded && self.active.is_none() {
            self.outer_loaded = true;
            self.outer_event(ToolLifecycleKind::Loaded);
        }
    }
    fn unload_outer(&mut self, reason: ToolCloseReason) {
        if self.outer_loaded {
            self.outer_loaded = false;
            self.outer_event(ToolLifecycleKind::Unloaded(reason));
        }
    }
    fn session_event(&mut self, kind: ToolLifecycleKind) {
        if let Some(s) = &self.active {
            self.events.push(SceneToolEvent {
                tool_id: s.tool_id.clone(),
                target: Some(s.target),
                token: Some(s.token),
                kind,
            });
        }
    }
    fn bind(&mut self, editor: &SceneEditor) -> Result<(), InteractionError> {
        if self.disposed {
            return Err(invalid("tool controller is disposed"));
        }
        let id = editor.engine.engine_id();
        if self.bound_engine.is_some_and(|bound| bound != id) {
            return Err(invalid("tool controller belongs to another editor engine"));
        }
        self.bound_engine = Some(id);
        Ok(())
    }
    fn target_epoch(
        editor: &SceneEditor,
        host: &ScenePluginHost,
        target: ElementHandle,
    ) -> Result<u64, InteractionError> {
        if !host.allows(Capability::Edit) {
            return Err(InteractionError::CapabilityDenied(Capability::Edit));
        }
        let instance = editor
            .engine
            .instance(target)
            .map_err(|e| invalid(e.to_string()))?;
        if instance.state != InstanceState::Attached
            || !object_editable(editor.scene(), &instance.id)
        {
            return Err(invalid(
                "tool target is detached, missing, locked, or non-invertible",
            ));
        }
        Ok(instance.attachment_epoch)
    }
    fn target_text<'a>(
        editor: &'a SceneEditor,
        host: &ScenePluginHost,
        target: ElementHandle,
    ) -> Result<(&'a str, u64), InteractionError> {
        let epoch = Self::target_epoch(editor, host, target)?;
        let Shape::Text(text) = &editor
            .engine
            .object(target)
            .map_err(|e| invalid(e.to_string()))?
            .shape
        else {
            return Err(invalid("text editing requires a built-in Text element"));
        };
        Ok((&text.content, epoch))
    }
    fn session_target_valid(
        &self,
        editor: &SceneEditor,
        host: &ScenePluginHost,
        session: &ToolSession,
    ) -> bool {
        Self::target_epoch(editor, host, session.target)
            .is_ok_and(|epoch| epoch == session.attachment_epoch)
            && self.registry.get(&session.tool_id).is_some_and(|entry| {
                entry.kind == session.kind
                    && editor
                        .engine
                        .object(session.target)
                        .is_ok_and(|o| entry.shape_tag.as_deref() == Some(o.shape.tag()))
            })
            && (session.kind != SceneToolKind::PlainText
                || Self::target_text(editor, host, session.target)
                    .is_ok_and(|(text, _)| text == session.text))
    }
    /// Revoke invalid/stale sessions without restoring any baseline. An external
    /// engine write is reconciled before deciding whether cancellation is safe.
    pub fn reconcile(&mut self, editor: &mut SceneEditor, host: &mut ScenePluginHost) -> bool {
        self.reconcile_with_resume(editor, host, true)
    }
    fn reconcile_with_resume(
        &mut self,
        editor: &mut SceneEditor,
        host: &mut ScenePluginHost,
        resume_outer: bool,
    ) -> bool {
        let Some(session) = &self.active else {
            return false;
        };
        let token = session.token;
        let valid = self.is_enabled()
            && session.document_epoch == editor.engine.document_epoch()
            && self.session_target_valid(editor, host, session)
            && !host.has_capture()
            && editor.is_edit_session_active(token.0);
        if valid {
            return false;
        }
        // finish reconciles external changes and can only seal this token; it
        // cannot commit a newer gesture/playback or restore a destroyed target.
        editor.finish_edit_session(token.0);
        self.session_event(ToolLifecycleKind::Unloaded(ToolCloseReason::Invalidated));
        self.active = None;
        if resume_outer {
            self.load_outer();
        }
        true
    }
    /// Non-mutating preflight for native adapters that must flush a pending IME
    /// value before switching targets. Failure never closes the current session.
    pub fn validate_text_target(
        &self,
        editor: &SceneEditor,
        host: &ScenePluginHost,
        target: ElementHandle,
    ) -> Result<(), InteractionError> {
        if !self.is_enabled() {
            return Err(invalid("tool controller is disabled or disposed"));
        }
        if self
            .bound_engine
            .is_some_and(|bound| bound != editor.engine.engine_id())
        {
            return Err(invalid("tool controller belongs to another editor engine"));
        }
        if !self
            .registry
            .inner_for_shape("text")
            .is_some_and(|entry| entry.kind == SceneToolKind::PlainText)
        {
            return Err(invalid("plain text tool is not registered"));
        }
        Self::target_text(editor, host, target)?;
        if host.has_capture() {
            return Err(InteractionError::GestureOwned(
                "active pointer capture".into(),
            ));
        }
        Ok(())
    }
    pub fn open_text(
        &mut self,
        editor: &mut SceneEditor,
        host: &mut ScenePluginHost,
        target: ElementHandle,
    ) -> Result<TextSessionToken, InteractionError> {
        self.bind(editor)?;
        self.reconcile(editor, host);
        if !self.is_enabled() {
            return Err(invalid("tool controller is disabled"));
        }
        let tool_id = self
            .registry
            .inner_for_shape("text")
            .filter(|entry| entry.kind == SceneToolKind::PlainText)
            .ok_or_else(|| invalid("plain text tool is not registered"))?
            .id
            .clone();
        let (text, attachment_epoch) = Self::target_text(editor, host, target)?;
        let text = text.to_owned();
        if host.has_capture() {
            return Err(InteractionError::GestureOwned(
                "active pointer capture".into(),
            ));
        }
        if let Some(session) = &self.active {
            if session.target == target && session.tool_id == tool_id {
                return Ok(session.token);
            }
            let old = session.token;
            self.finish_with_resume(editor, host, old, ToolCloseReason::TargetSwitch, false)?;
        }
        let token = match editor.begin_edit_session() {
            Ok(token) => TextSessionToken(token),
            Err(error) => {
                self.load_outer();
                return Err(invalid(error.to_string()));
            }
        };
        self.unload_outer(ToolCloseReason::InnerOpened);
        self.active = Some(ToolSession {
            kind: SceneToolKind::PlainText,
            tool_id,
            token,
            target,
            document_epoch: editor.engine.document_epoch(),
            attachment_epoch,
            text,
        });
        self.session_event(ToolLifecycleKind::Loaded);
        Ok(token)
    }
    /// Validate before a native host closes another editor or allocates a widget.
    pub fn validate_inner_target(
        &self,
        editor: &SceneEditor,
        host: &ScenePluginHost,
        id: &str,
        target: ElementHandle,
    ) -> Result<(), InteractionError> {
        if !self.is_enabled()
            || self
                .bound_engine
                .is_some_and(|v| v != editor.engine.engine_id())
        {
            return Err(invalid(
                "tool controller is disabled or belongs to another engine",
            ));
        }
        let entry = self
            .registry
            .get(id)
            .filter(|e| e.kind == SceneToolKind::CustomInner)
            .ok_or_else(|| invalid("custom inner tool is not registered"))?;
        Self::target_epoch(editor, host, target)?;
        let object = editor
            .engine
            .object(target)
            .map_err(|e| invalid(e.to_string()))?;
        if entry.shape_tag.as_deref() != Some(object.shape.tag()) {
            return Err(invalid("tool does not support target shape"));
        }
        if host.has_capture() {
            return Err(InteractionError::GestureOwned(
                "active pointer capture".into(),
            ));
        }
        Ok(())
    }
    pub fn open_inner(
        &mut self,
        editor: &mut SceneEditor,
        host: &mut ScenePluginHost,
        id: &str,
        target: ElementHandle,
    ) -> Result<ToolSessionToken, InteractionError> {
        self.bind(editor)?;
        self.validate_inner_target(editor, host, id, target)?;
        self.reconcile(editor, host);
        let epoch = Self::target_epoch(editor, host, target)?;
        if let Some(active) = &self.active {
            if active.target == target && active.tool_id == id {
                return Ok(active.token);
            }
            let old_token = active.token;
            self.finish_with_resume(
                editor,
                host,
                old_token,
                ToolCloseReason::TargetSwitch,
                false,
            )?;
        }
        let token = match editor.begin_edit_session() {
            Ok(token) => TextSessionToken(token),
            Err(error) => {
                self.load_outer();
                return Err(invalid(error.to_string()));
            }
        };
        self.unload_outer(ToolCloseReason::InnerOpened);
        self.active = Some(ToolSession {
            kind: SceneToolKind::CustomInner,
            tool_id: id.into(),
            token,
            target,
            document_epoch: editor.engine.document_epoch(),
            attachment_epoch: epoch,
            text: String::new(),
        });
        self.session_event(ToolLifecycleKind::Loaded);
        Ok(token)
    }
    /// A custom widget edits only its live target through the normal policy/filter path.
    pub fn set_inner_property(
        &mut self,
        editor: &mut SceneEditor,
        host: &mut ScenePluginHost,
        token: ToolSessionToken,
        key: PropertyKey,
        value: serde_json::Value,
    ) -> Result<(), InteractionError> {
        self.bind(editor)?;
        self.reconcile(editor, host);
        let target = self
            .active
            .as_ref()
            .filter(|s| s.token == token && s.kind == SceneToolKind::CustomInner)
            .ok_or_else(|| invalid("custom tool session ended or token is stale"))?
            .target;
        let before = editor.engine.revision();
        let result = host.execute(
            editor,
            SceneCommand::SetSessionProperty {
                token: token.0,
                handle: target,
                key,
                value,
            },
        );
        if let Err(error) = result {
            self.reconcile(editor, host);
            return Err(error);
        }
        // A successful edit may lock or otherwise invalidate its own target.
        // Report that accepted command as success, then seal and unload it;
        // a stale callback still cannot edit or cancel the completed session.
        if editor.engine.revision() != before {
            self.session_event(ToolLifecycleKind::Updated);
        }
        self.reconcile(editor, host);
        Ok(())
    }
    /// Resize a registered Custom definition through its measured policy in one
    /// token-owned atomic update. Done/Cancel retain ordinary inner-tool history.
    pub fn resize_inner(
        &mut self,
        editor: &mut SceneEditor,
        host: &mut ScenePluginHost,
        token: ToolSessionToken,
        size: gpui_flow_scene::Size,
        anchor: gpui_flow_scene::Point,
    ) -> Result<(), InteractionError> {
        self.bind(editor)?;
        self.reconcile(editor, host);
        let target = self
            .active
            .as_ref()
            .filter(|s| s.token == token && s.kind == SceneToolKind::CustomInner)
            .ok_or_else(|| invalid("custom tool session ended or token is stale"))?
            .target;
        let before = editor.engine.revision();
        let result = host.execute(
            editor,
            SceneCommand::ResizeCustomSession {
                token: token.0,
                handle: target,
                size,
                anchor,
            },
        );
        if result.is_ok() && editor.engine.revision() != before {
            self.session_event(ToolLifecycleKind::Updated);
        }
        self.reconcile(editor, host);
        result.map(|_| ())
    }
    pub fn set_text(
        &mut self,
        editor: &mut SceneEditor,
        host: &mut ScenePluginHost,
        token: TextSessionToken,
        text: impl Into<String>,
    ) -> Result<(), InteractionError> {
        self.bind(editor)?;
        self.reconcile(editor, host);
        let session = self
            .active
            .as_ref()
            .filter(|s| s.token == token && s.kind == SceneToolKind::PlainText)
            .ok_or_else(|| invalid("text session ended or token is stale"))?;
        let target = session.target;
        let text = text.into();
        // Adapter flushes (Done, blur, IME target switch) may repeat the already
        // accepted value. Do not run non-idempotent middleware a second time.
        if text == session.text {
            return Ok(());
        }
        let result = host.execute(
            editor,
            SceneCommand::SetSessionProperty {
                token: token.0,
                handle: target,
                key: PropertyKey::TextContent,
                value: serde_json::Value::String(text),
            },
        );
        if let Err(error) = result {
            self.reconcile(editor, host);
            return Err(error);
        }
        // Middleware may normalize content. Read back canonical state rather
        // than trusting the requested value or creating a second text buffer.
        let (text, _) = Self::target_text(editor, host, target)?;
        let text = text.to_owned();
        if !editor.is_edit_session_active(token.0) {
            self.reconcile(editor, host);
            return Err(invalid(
                "text session was revoked during command processing",
            ));
        }
        let changed = self
            .active
            .as_ref()
            .is_some_and(|session| session.text != text);
        self.active.as_mut().expect("owned text session").text = text;
        if changed {
            self.session_event(ToolLifecycleKind::Updated);
        }
        Ok(())
    }
    pub fn finish(
        &mut self,
        editor: &mut SceneEditor,
        host: &mut ScenePluginHost,
        token: TextSessionToken,
    ) -> Result<bool, InteractionError> {
        self.finish_with_reason(editor, host, token, ToolCloseReason::Done)
    }
    pub fn finish_with_reason(
        &mut self,
        editor: &mut SceneEditor,
        host: &mut ScenePluginHost,
        token: TextSessionToken,
        reason: ToolCloseReason,
    ) -> Result<bool, InteractionError> {
        self.finish_with_resume(editor, host, token, reason, true)
    }
    fn finish_with_resume(
        &mut self,
        editor: &mut SceneEditor,
        host: &mut ScenePluginHost,
        token: TextSessionToken,
        reason: ToolCloseReason,
        resume_outer: bool,
    ) -> Result<bool, InteractionError> {
        self.bind(editor)?;
        if self.active_token() != Some(token) {
            return Ok(false);
        }
        self.reconcile_with_resume(editor, host, resume_outer);
        if self.active_token() != Some(token) {
            return Ok(false);
        }
        if !editor.finish_edit_session(token.0) {
            self.reconcile_with_resume(editor, host, resume_outer);
            return Ok(false);
        }
        self.session_event(ToolLifecycleKind::Unloaded(reason));
        self.active = None;
        if resume_outer {
            self.load_outer();
        }
        Ok(true)
    }
    pub fn cancel(
        &mut self,
        editor: &mut SceneEditor,
        host: &mut ScenePluginHost,
        token: TextSessionToken,
    ) -> Result<bool, InteractionError> {
        self.bind(editor)?;
        self.reconcile(editor, host);
        if self.active_token() != Some(token) {
            return Ok(false);
        }
        if !editor.cancel_edit_session(token.0) {
            return Err(invalid("text baseline restoration was rejected"));
        }
        self.session_event(ToolLifecycleKind::Unloaded(ToolCloseReason::Cancelled));
        self.active = None;
        self.load_outer();
        Ok(true)
    }
    /// Named outer selection is lifecycle plumbing; the application decides
    /// which input plugin behavior corresponds to that registered identity.
    pub fn select_outer(
        &mut self,
        editor: &mut SceneEditor,
        host: &mut ScenePluginHost,
        id: &str,
    ) -> Result<bool, InteractionError> {
        self.bind(editor)?;
        if !self
            .registry
            .get(id)
            .is_some_and(|tool| tool.kind == SceneToolKind::Outer)
        {
            return Err(invalid("unknown outer tool"));
        }
        self.reconcile_with_resume(editor, host, false);
        let closed = if let Some(token) = self.active_token() {
            self.finish_with_resume(editor, host, token, ToolCloseReason::OuterToolSwitch, false)?
        } else {
            false
        };
        if self.outer == id {
            self.load_outer();
            return Ok(closed);
        }
        if host.has_capture() {
            host.cancel(editor);
        }
        self.unload_outer(ToolCloseReason::OuterToolSwitch);
        self.outer = id.into();
        self.load_outer();
        Ok(true)
    }
    pub fn set_enabled(
        &mut self,
        editor: &mut SceneEditor,
        host: &mut ScenePluginHost,
        enabled: bool,
    ) -> Result<bool, InteractionError> {
        self.bind(editor)?;
        self.reconcile_with_resume(editor, host, enabled);
        if self.enabled == enabled {
            return Ok(false);
        }
        if !enabled {
            if host.has_capture() {
                host.cancel(editor);
            }
            if let Some(token) = self.active_token() {
                self.finish_with_resume(editor, host, token, ToolCloseReason::Disabled, false)?;
            }
            self.unload_outer(ToolCloseReason::Disabled);
        }
        self.enabled = enabled;
        if enabled {
            self.load_outer();
        }
        Ok(true)
    }
    /// Explicit registration emits Created; replacement uses unregister then
    /// register so an active editor is always unloaded before its identity dies.
    pub fn register_outer(&mut self, id: impl Into<String>) -> Result<(), InteractionError> {
        if self.disposed {
            return Err(invalid("tool controller is disposed"));
        }
        let id = id.into();
        self.registry.register_outer(id.clone())?;
        self.events.push(SceneToolEvent {
            tool_id: id,
            target: None,
            token: None,
            kind: ToolLifecycleKind::Created,
        });
        Ok(())
    }
    pub fn register_inner(
        &mut self,
        id: impl Into<String>,
        shape_tag: impl Into<String>,
    ) -> Result<(), InteractionError> {
        if self.disposed {
            return Err(invalid("tool controller is disposed"));
        }
        let id = id.into();
        self.registry.register_inner(id.clone(), shape_tag)?;
        self.events.push(SceneToolEvent {
            tool_id: id,
            target: None,
            token: None,
            kind: ToolLifecycleKind::Created,
        });
        Ok(())
    }
    pub fn register_text(&mut self, id: impl Into<String>) -> Result<(), InteractionError> {
        if self.disposed {
            return Err(invalid("tool controller is disposed"));
        }
        let id = id.into();
        self.registry.register_text(id.clone())?;
        self.events.push(SceneToolEvent {
            tool_id: id,
            target: None,
            token: None,
            kind: ToolLifecycleKind::Created,
        });
        Ok(())
    }
    /// The configured default outer is retained as the fallback. Unknown IDs
    /// are idempotent. Removing an active tool seals its owned edits first.
    pub fn unregister(
        &mut self,
        editor: &mut SceneEditor,
        host: &mut ScenePluginHost,
        id: &str,
    ) -> Result<bool, InteractionError> {
        self.bind(editor)?;
        if id == self.registry.default_outer() {
            return Err(invalid("cannot unregister the default outer tool"));
        }
        if self.registry.get(id).is_none() {
            return Ok(false);
        }
        self.reconcile_with_resume(editor, host, false);
        if self
            .active
            .as_ref()
            .is_some_and(|session| session.tool_id == id)
        {
            let token = self.active_token().expect("active tool");
            self.finish_with_resume(editor, host, token, ToolCloseReason::Unregistered, false)?;
        }
        if self.outer == id {
            if host.has_capture() {
                host.cancel(editor);
            }
            self.unload_outer(ToolCloseReason::Unregistered);
            self.outer = self.registry.default_outer.clone();
        }
        self.registry.entries.remove(id);
        self.events.push(SceneToolEvent {
            tool_id: id.into(),
            target: None,
            token: None,
            kind: ToolLifecycleKind::Destroyed,
        });
        self.load_outer();
        Ok(true)
    }
    /// Seal live edits, emit deterministic teardown, then reject future opens.
    /// Repeated release/disposal is harmless and produces no duplicate events.
    pub fn dispose(
        &mut self,
        editor: &mut SceneEditor,
        host: &mut ScenePluginHost,
    ) -> Result<bool, InteractionError> {
        if self.disposed {
            return Ok(false);
        }
        self.bind(editor)?;
        self.reconcile_with_resume(editor, host, false);
        if host.has_capture() {
            host.cancel(editor);
        }
        if let Some(token) = self.active_token() {
            self.finish_with_resume(editor, host, token, ToolCloseReason::Disposed, false)?;
        }
        self.unload_outer(ToolCloseReason::Disposed);
        for tool in self.registry.registrations() {
            self.events.push(SceneToolEvent {
                tool_id: tool.id.clone(),
                target: None,
                token: None,
                kind: ToolLifecycleKind::Destroyed,
            });
        }
        self.disposed = true;
        Ok(true)
    }
}
fn invalid(message: impl Into<String>) -> InteractionError {
    InteractionError::InvalidInput(message.into())
}
