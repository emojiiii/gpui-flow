//! Paint-only state overlays. The canonical document, caches, and edit history
//! are never overwritten by transient hover/press/focus presentation.
use super::{SceneEventRouter, StateTransitions, state_transition::TransitionFrame};
use gpui_flow_editor::SceneEditor;
use gpui_flow_engine::{ElementHandle, EngineError, InstanceState, SceneEngine};
use gpui_flow_scene::{Color, Scene, SceneError, Shape, Style};
use std::{borrow::Cow, collections::BTreeMap, time::Duration};

pub const MAX_STATE_STYLE_BINDINGS: usize = 1024;

/// An absolute replacement for the base fill and/or local opacity. Additional
/// paints, stroke geometry and all document fields remain unchanged.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct StatePaint {
    pub fill: Option<Color>,
    pub opacity: Option<f32>,
}
impl StatePaint {
    pub fn fill(color: Color) -> Self {
        Self {
            fill: Some(color),
            opacity: None,
        }
    }
    fn validate(&self) -> Result<(), StateStyleError> {
        if self
            .opacity
            .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
        {
            return Err(StateStyleError::InvalidStyle(
                "opacity must be finite and in [0,1]".into(),
            ));
        }
        Ok(())
    }
    fn apply(&self, style: &mut Style) {
        if let Some(fill) = self.fill {
            style.fill = Some(fill.into());
        }
        if let Some(opacity) = self.opacity {
            style.opacity = opacity;
        }
    }
}
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct VisualState {
    pub selected: bool,
    pub focus: bool,
    pub hover: bool,
    pub press: bool,
    pub disabled: bool,
}
#[derive(Debug, Default, Clone, PartialEq)]
pub struct StateStyles {
    pub selected: StatePaint,
    pub focus: StatePaint,
    pub hover: StatePaint,
    pub press: StatePaint,
    pub disabled: StatePaint,
}
impl StateStyles {
    /// Lower states supply fields that higher states do not override. Disabled
    /// suppresses focus/hover/press even if its own patch is empty.
    pub fn resolve(&self, base: &Style, state: VisualState) -> Style {
        let mut next = base.clone();
        if state.selected {
            self.selected.apply(&mut next);
        }
        if state.disabled {
            self.disabled.apply(&mut next);
        } else {
            if state.focus {
                self.focus.apply(&mut next);
            }
            if state.hover {
                self.hover.apply(&mut next);
            }
            if state.press {
                self.press.apply(&mut next);
            }
        }
        next
    }
    fn paints(&self) -> [&StatePaint; 5] {
        [
            &self.selected,
            &self.focus,
            &self.hover,
            &self.press,
            &self.disabled,
        ]
    }
}
#[derive(Debug, Clone, PartialEq)]
pub enum StateStyleError {
    Engine(EngineError),
    Scene(SceneError),
    WrongEngine,
    Unbound,
    Capacity,
    InvalidStyle(String),
    TimeWentBackwards,
}
impl From<EngineError> for StateStyleError {
    fn from(error: EngineError) -> Self {
        Self::Engine(error)
    }
}
impl From<SceneError> for StateStyleError {
    fn from(error: SceneError) -> Self {
        Self::Scene(error)
    }
}
impl std::fmt::Display for StateStyleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Engine(error) => error.fmt(f),
            Self::Scene(error) => error.fmt(f),
            Self::WrongEngine => write!(f, "state styles belong to another scene engine"),
            Self::TimeWentBackwards => write!(f, "state transition time must be monotonic"),
            Self::Unbound => write!(f, "instance has no state style binding"),
            Self::Capacity => write!(f, "state style binding capacity exhausted"),
            Self::InvalidStyle(error) => write!(f, "invalid state style: {error}"),
        }
    }
}
impl std::error::Error for StateStyleError {}
struct Binding {
    styles: StateStyles,
    disabled: bool,
    transitions: StateTransitions,
    frame: Option<TransitionFrame>,
}
/// Opt-in, engine-scoped registrations. No listener closures or timers are
/// installed. Explicit unbind/clear release registrations; dead generations are
/// pruned on presentation/bind. Detached registrations survive, pointer states do not.
pub struct SceneStateStyles {
    engine: u64,
    bindings: BTreeMap<ElementHandle, Binding>,
    time: Duration,
}
impl SceneStateStyles {
    pub fn new(engine: &SceneEngine) -> Self {
        Self {
            engine: engine.engine_id(),
            bindings: BTreeMap::new(),
            time: Duration::ZERO,
        }
    }
    fn check(&self, engine: &SceneEngine) -> Result<(), StateStyleError> {
        if self.engine == engine.engine_id() {
            Ok(())
        } else {
            Err(StateStyleError::WrongEngine)
        }
    }
    pub fn len(&self) -> usize {
        self.bindings.len()
    }
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }
    pub fn clear(&mut self) {
        self.bindings.clear();
    }
    pub fn unbind(&mut self, handle: ElementHandle) -> bool {
        self.bindings.remove(&handle).is_some()
    }
    pub fn prune(&mut self, engine: &SceneEngine) -> Result<usize, StateStyleError> {
        self.check(engine)?;
        let before = self.bindings.len();
        self.bindings.retain(|handle, _| {
            engine
                .instance(*handle)
                .is_ok_and(|i| i.state != InstanceState::Destroyed)
        });
        Ok(before - self.bindings.len())
    }
    /// Validate every state, including currently inactive ones, before replacing
    /// a registration. Replacement preserves its host-controlled disabled flag.
    pub fn bind(
        &mut self,
        engine: &SceneEngine,
        handle: ElementHandle,
        styles: StateStyles,
    ) -> Result<(), StateStyleError> {
        self.check(engine)?;
        let object = engine.object(handle)?;
        for paint in styles.paints() {
            paint.validate()?;
            if paint.fill.is_some() && matches!(object.shape, Shape::Image(_) | Shape::Group(_)) {
                return Err(StateStyleError::InvalidStyle(
                    "base fill states require a vector/text/custom element; Image and Group support opacity only".into(),
                ));
            }
            let mut probe = object.clone();
            paint.apply(&mut probe.style);
            probe.validate().map_err(StateStyleError::InvalidStyle)?;
        }
        self.prune(engine)?;
        if !self.bindings.contains_key(&handle) && self.bindings.len() >= MAX_STATE_STYLE_BINDINGS {
            return Err(StateStyleError::Capacity);
        }
        let disabled = self.bindings.get(&handle).is_some_and(|b| b.disabled);
        let (transitions, frame) = self
            .bindings
            .get(&handle)
            .map(|binding| (binding.transitions, binding.frame.clone()))
            .unwrap_or_default();
        self.bindings.insert(
            handle,
            Binding {
                styles,
                disabled,
                transitions,
                frame,
            },
        );
        Ok(())
    }
    /// Visual state only. Editing/hit/command authorization stays in the host's
    /// access policy; this flag never silently disables a listener or command.
    pub fn set_disabled(
        &mut self,
        engine: &SceneEngine,
        handle: ElementHandle,
        disabled: bool,
    ) -> Result<(), StateStyleError> {
        self.check(engine)?;
        engine.object(handle)?;
        self.bindings
            .get_mut(&handle)
            .ok_or(StateStyleError::Unbound)?
            .disabled = disabled;
        Ok(())
    }
    /// Set timing for this binding. Reconfiguration finishes existing motion;
    /// future state changes use the new settings. Runtime-only, never an edit.
    pub fn set_transitions(
        &mut self,
        engine: &SceneEngine,
        handle: ElementHandle,
        transitions: StateTransitions,
    ) -> Result<(), StateStyleError> {
        self.check(engine)?;
        engine.object(handle)?;
        let binding = self
            .bindings
            .get_mut(&handle)
            .ok_or(StateStyleError::Unbound)?;
        binding.transitions = transitions;
        if let Some(frame) = &mut binding.frame {
            frame.finish();
        }
        Ok(())
    }
    pub fn has_active_transitions(&self) -> bool {
        self.bindings
            .values()
            .any(|b| b.frame.as_ref().is_some_and(TransitionFrame::active))
    }
    /// Snap running presentation to its resolved endpoint, leaving registrations
    /// in place. Unbind/clear instead release the overlay to the canonical scene.
    pub fn finish_transitions(&mut self) {
        for binding in self.bindings.values_mut() {
            if let Some(frame) = &mut binding.frame {
                frame.finish();
            }
        }
    }
    pub fn transition_time(&self) -> Duration {
        self.time
    }
    /// Resolve at the most recently accepted time. With default zero-duration
    /// timing this retains immediate state-style behavior.
    pub fn presentation<'a>(
        &mut self,
        editor: &'a SceneEditor,
        router: &SceneEventRouter,
    ) -> Result<Cow<'a, Scene>, StateStyleError> {
        self.presentation_at(editor, router, self.time)
    }
    /// Resolve against the current canonical style at a monotonic host time.
    /// Entry/exit retarget from the interpolated display value. A canonical fill
    /// or opacity edit cancels motion on that channel; the current state patch
    /// still takes precedence. Other fields always come from the document.
    /// Invalid presentation leaves the accepted clock and all motion unchanged.
    pub fn presentation_at<'a>(
        &mut self,
        editor: &'a SceneEditor,
        router: &SceneEventRouter,
        now: Duration,
    ) -> Result<Cow<'a, Scene>, StateStyleError> {
        self.check(&editor.engine)?;
        if now < self.time {
            return Err(StateStyleError::TimeWentBackwards);
        }
        self.prune(&editor.engine)?;
        let mut scene = Cow::Borrowed(editor.scene());
        let mut frames = Vec::with_capacity(self.bindings.len());
        for (handle, binding) in &self.bindings {
            let info = editor.engine.instance(*handle)?;
            if info.state != InstanceState::Attached {
                frames.push((*handle, None));
                continue;
            }
            let input = router.state(&editor.engine, *handle);
            let object = editor.engine.object(*handle)?;
            let state = VisualState {
                selected: editor.selection.ids.contains(&info.id),
                focus: input.focus,
                hover: input.hover,
                press: input.press,
                disabled: binding.disabled,
            };
            let target = binding.styles.resolve(&object.style, state);
            let mut frame = binding
                .frame
                .clone()
                .filter(|frame| frame.epoch == info.attachment_epoch)
                .unwrap_or_else(|| TransitionFrame::new(info.attachment_epoch, &object.style));
            let next = frame.resolve(&object.style, target, state, binding.transitions, now);
            if next != object.style {
                scene
                    .to_mut()
                    .update_object(&info.id, |object| object.style = next)?;
            }
            frames.push((*handle, Some(frame)));
        }
        for (handle, frame) in frames {
            self.bindings
                .get_mut(&handle)
                .expect("binding exists during presentation")
                .frame = frame;
        }
        self.time = now;
        Ok(scene)
    }
}
