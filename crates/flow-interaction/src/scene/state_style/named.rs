use super::{SceneStateStyles, StatePaint, StateStyleError, VisualState, validate_paint};
use crate::scene::SceneEventRouter;
use gpui_flow_editor::SceneEditor;
use gpui_flow_engine::{ElementHandle, InstanceState, SceneEngine};
use gpui_flow_scene::Shape;
use std::collections::BTreeMap;

pub const MAX_NAMED_STATES_PER_BINDING: usize = 128;
pub const MAX_STATE_NAME_BYTES: usize = 128;
pub const MAX_PLACEHOLDER_BYTES: usize = 65_536;

/// How a non-button binding obtains a button's state. The default mirrors
/// Leafer's parent/grandparent lookup, rather than an unbounded tree cascade.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum StateInheritance {
    /// Only this element's state and the router's normal ancestor input state.
    None,
    /// Nearest registered button among this element's parent and grandparent.
    /// A button never inherits another button's state.
    #[default]
    ParentButton,
}

/// Runtime placeholder for a Text's existing fixed layout box. Its content is
/// rendered only when canonical text is exactly empty. Whitespace is content.
/// It never changes intrinsic measurement, document text, selection or hit boxes.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct TextPlaceholder {
    pub content: String,
    pub paint: StatePaint,
}
impl TextPlaceholder {
    pub fn new(content: impl Into<String>, paint: StatePaint) -> Self {
        Self {
            content: content.into(),
            paint,
        }
    }
}

/// Read-only input to presentation. An inherited name may have no local paint;
/// in that case only its named layer is absent. Button paints are never copied.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ResolvedVisualState {
    pub visual: VisualState,
    pub named: Option<String>,
    pub placeholder: bool,
    pub button: Option<ElementHandle>,
}

impl SceneStateStyles {
    /// Replace the local named palette atomically, checking inactive paints too.
    /// The selected name survives palette replacement; if removed, its layer is
    /// absent until redefined or explicitly cleared. Other state layers remain.
    pub fn set_named_states(
        &mut self,
        engine: &SceneEngine,
        handle: ElementHandle,
        states: BTreeMap<String, StatePaint>,
    ) -> Result<(), StateStyleError> {
        self.check(engine)?;
        let object = engine.object(handle)?;
        if !self.bindings.contains_key(&handle) {
            return Err(StateStyleError::Unbound);
        }
        if states.len() > MAX_NAMED_STATES_PER_BINDING {
            return Err(StateStyleError::Capacity);
        }
        for (name, paint) in &states {
            validate_name(name)?;
            validate_paint(object, paint)?;
        }
        self.bindings
            .get_mut(&handle)
            .expect("checked binding")
            .named = states;
        Ok(())
    }

    /// Select a locally defined named state. `None` clears the local override
    /// and permits parent-button inheritance again. A missing local name is an
    /// atomic error, not a typo that silently resets the current state.
    pub fn select_state(
        &mut self,
        engine: &SceneEngine,
        handle: ElementHandle,
        name: Option<&str>,
    ) -> Result<(), StateStyleError> {
        self.check(engine)?;
        engine.object(handle)?;
        let binding = self
            .bindings
            .get_mut(&handle)
            .ok_or(StateStyleError::Unbound)?;
        if let Some(name) = name {
            validate_name(name)?;
            if !binding.named.contains_key(name) {
                return Err(StateStyleError::MissingState(name.into()));
            }
        }
        binding.state = name.map(str::to_owned);
        Ok(())
    }

    /// Configure real placeholder rendering, not an unrelated boolean flag.
    /// Empty placeholder text is allowed and has no effect. Non-Text instances
    /// reject a placeholder, even when their current state is inactive.
    pub fn set_placeholder(
        &mut self,
        engine: &SceneEngine,
        handle: ElementHandle,
        placeholder: Option<TextPlaceholder>,
    ) -> Result<(), StateStyleError> {
        self.check(engine)?;
        let object = engine.object(handle)?;
        if !self.bindings.contains_key(&handle) {
            return Err(StateStyleError::Unbound);
        }
        if let Some(placeholder) = &placeholder {
            if !matches!(object.shape, Shape::Text(_)) {
                return Err(StateStyleError::InvalidStyle(
                    "placeholder requires Text".into(),
                ));
            }
            if placeholder.content.len() > MAX_PLACEHOLDER_BYTES {
                return Err(StateStyleError::Capacity);
            }
            validate_paint(object, &placeholder.paint)?;
        }
        self.bindings
            .get_mut(&handle)
            .expect("checked binding")
            .placeholder = placeholder;
        Ok(())
    }

    /// A runtime button is a source of state for opted-in children, not a native
    /// control or event authorization policy. It requires a live binding, which
    /// may have empty styles. Removing the binding removes its button role too.
    pub fn set_button(
        &mut self,
        engine: &SceneEngine,
        handle: ElementHandle,
        button: bool,
    ) -> Result<(), StateStyleError> {
        self.check(engine)?;
        engine.object(handle)?;
        self.bindings
            .get_mut(&handle)
            .ok_or(StateStyleError::Unbound)?
            .button = button;
        Ok(())
    }

    pub fn set_inheritance(
        &mut self,
        engine: &SceneEngine,
        handle: ElementHandle,
        inheritance: StateInheritance,
    ) -> Result<(), StateStyleError> {
        self.check(engine)?;
        engine.object(handle)?;
        self.bindings
            .get_mut(&handle)
            .ok_or(StateStyleError::Unbound)?
            .inheritance = inheritance;
        Ok(())
    }

    /// Resolve actual router input, the current frozen editor selection, local
    /// flags and live parent-button state without changing any of them. Parent
    /// state is read directly (never recursively from presentation). Instance
    /// generations and attachment epochs are checked by the router on every read.
    pub fn resolved_state(
        &self,
        editor: &SceneEditor,
        router: &SceneEventRouter,
        handle: ElementHandle,
    ) -> Result<ResolvedVisualState, StateStyleError> {
        self.check(&editor.engine)?;
        let object = editor.engine.object(handle)?;
        let info = editor.engine.instance(handle)?;
        let binding = self.bindings.get(&handle).ok_or(StateStyleError::Unbound)?;
        if info.state != InstanceState::Attached {
            return Ok(ResolvedVisualState::default());
        }
        let input = router.state(&editor.engine, handle);
        let mut result = ResolvedVisualState {
            visual: VisualState {
                selected: editor.selection.ids.contains(&info.id),
                focus: input.focus,
                hover: input.hover,
                press: input.press,
                disabled: binding.disabled,
            },
            named: binding.state.clone(),
            placeholder: binding.placeholder.as_ref().is_some_and(|placeholder| {
                !placeholder.content.is_empty()
                    && matches!(&object.shape, Shape::Text(text) if text.content.is_empty())
            }),
            button: None,
        };
        if !binding.button && binding.inheritance == StateInheritance::ParentButton {
            for id in editor
                .scene()
                .ancestor_path(&info.id)?
                .iter()
                .rev()
                .skip(1)
                .take(2)
            {
                let Some(button) = editor.engine.handle(id) else {
                    continue;
                };
                let Some(parent) = self.bindings.get(&button).filter(|binding| binding.button)
                else {
                    continue;
                };
                // Resolve from the current scene every time; no ID/epoch cache
                // can resurrect an old parent's state after detach or replacement.
                if editor.engine.instance(button)?.state != InstanceState::Attached {
                    continue;
                }
                let input = router.state(&editor.engine, button);
                result.button = Some(button);
                if result.named.is_none() {
                    result.named.clone_from(&parent.state);
                }
                result.visual.selected |= editor.selection.ids.contains(id);
                result.visual.disabled |= parent.disabled;
                result.visual.focus |= input.focus;
                result.visual.hover |= input.hover;
                result.visual.press |= input.press;
                break;
            }
        }
        Ok(result)
    }
}

fn validate_name(name: &str) -> Result<(), StateStyleError> {
    if name.trim().is_empty() || name.len() > MAX_STATE_NAME_BYTES {
        return Err(StateStyleError::InvalidStyle(format!(
            "state name must be nonblank and at most {MAX_STATE_NAME_BYTES} UTF-8 bytes"
        )));
    }
    Ok(())
}
