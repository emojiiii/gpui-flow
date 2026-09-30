//! Paint-only state overlays. The canonical document, caches, and edit history
//! are never overwritten by transient hover/press/focus presentation.
use super::SceneEventRouter;
use gpui_flow_editor::SceneEditor;
use gpui_flow_engine::{ElementHandle, EngineError, InstanceState, SceneEngine};
use gpui_flow_scene::{Color, Scene, SceneError, Shape, Style};
use std::{borrow::Cow, collections::BTreeMap};

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
}
/// Opt-in, engine-scoped registrations. No listener closures or timers are
/// installed. Explicit unbind/clear release registrations; dead generations are
/// pruned on presentation/bind. Detached registrations survive, pointer states do not.
pub struct SceneStateStyles {
    engine: u64,
    bindings: BTreeMap<ElementHandle, Binding>,
}
impl SceneStateStyles {
    pub fn new(engine: &SceneEngine) -> Self {
        Self {
            engine: engine.engine_id(),
            bindings: BTreeMap::new(),
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
        self.bindings.insert(handle, Binding { styles, disabled });
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
    /// Resolve against the *current* canonical style, not a saved restoration
    /// snapshot. Edits/animation made while hovered become visible when it ends.
    /// Unchanged presentation borrows the source; overlays allocate a private
    /// scene so inherited opacity and clipping use the existing paint pipeline.
    pub fn presentation<'a>(
        &mut self,
        editor: &'a SceneEditor,
        router: &SceneEventRouter,
    ) -> Result<Cow<'a, Scene>, StateStyleError> {
        self.prune(&editor.engine)?;
        let mut scene = Cow::Borrowed(editor.scene());
        for (handle, binding) in &self.bindings {
            let info = editor.engine.instance(*handle)?;
            if info.state != InstanceState::Attached {
                continue;
            }
            let input = router.state(&editor.engine, *handle);
            let object = editor.engine.object(*handle)?;
            let next = binding.styles.resolve(
                &object.style,
                VisualState {
                    selected: editor.selection.ids.contains(&info.id),
                    focus: input.focus,
                    hover: input.hover,
                    press: input.press,
                    disabled: binding.disabled,
                },
            );
            if next != object.style {
                scene
                    .to_mut()
                    .update_object(&info.id, |object| object.style = next)?;
            }
        }
        Ok(scene)
    }
}
