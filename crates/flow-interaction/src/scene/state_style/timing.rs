use super::{ResolvedVisualState, SceneStateStyles, StateStyleError, named::validate_name};
use crate::scene::{StateTransition, StateTransitions};
use gpui_flow_engine::{ElementHandle, SceneEngine};
use std::collections::BTreeMap;

/// State identity for local timing overrides. Named states are the local palette
/// entry even when its name or input flags come from a parent button.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum StateKey {
    Named(String),
    Selected,
    Placeholder,
    Focus,
    Hover,
    Press,
    Disabled,
}
/// Optional per-state timing. Entry falls back to binding entry. Exit chooses
/// state exit, binding exit, state entry, then binding entry, matching the pinned
/// upstream fallback. Some(zero) explicitly disables that direction's motion.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct StateTransitionOverrides {
    pub transition: Option<StateTransition>,
    pub transition_out: Option<StateTransition>,
}
impl SceneStateStyles {
    /// Set/remove a local state timing override. Reconfiguration finishes current
    /// motion exactly like set_transitions; no history or clock is changed.
    pub fn set_state_transitions(
        &mut self,
        engine: &SceneEngine,
        handle: ElementHandle,
        state: StateKey,
        transitions: Option<StateTransitionOverrides>,
    ) -> Result<(), StateStyleError> {
        self.check(engine)?;
        engine.object(handle)?;
        if let StateKey::Named(name) = &state {
            validate_name(name)?;
        }
        let binding = self
            .bindings
            .get_mut(&handle)
            .ok_or(StateStyleError::Unbound)?;
        if let Some(transitions) = transitions {
            if !binding.state_transitions.contains_key(&state)
                && binding.state_transitions.len() >= super::MAX_NAMED_STATES_PER_BINDING + 6
            {
                return Err(StateStyleError::Capacity);
            }
            binding.state_transitions.insert(state, transitions);
        } else {
            binding.state_transitions.remove(&state);
        }
        if let Some(frame) = &mut binding.frame {
            frame.finish();
        }
        Ok(())
    }
}
fn active(state: &ResolvedVisualState) -> Vec<StateKey> {
    let mut result = Vec::new();
    if let Some(name) = &state.named {
        result.push(StateKey::Named(name.clone()));
    }
    if state.visual.selected {
        result.push(StateKey::Selected);
    }
    if state.placeholder {
        result.push(StateKey::Placeholder);
    }
    if state.visual.disabled {
        result.push(StateKey::Disabled);
    } else {
        if state.visual.focus {
            result.push(StateKey::Focus);
        }
        if state.visual.hover {
            result.push(StateKey::Hover);
        }
        if state.visual.press {
            result.push(StateKey::Press);
        }
    }
    result
}
pub(in crate::scene) fn timing(
    old: &ResolvedVisualState,
    new: &ResolvedVisualState,
    defaults: StateTransitions,
    overrides: &BTreeMap<StateKey, StateTransitionOverrides>,
) -> StateTransition {
    let before = active(old);
    let after = active(new);
    // Disabling/leaving disabled is its own explicit entry/exit. Otherwise a
    // simultaneous change prefers the highest-priority added layer, then the
    // highest removed layer. An edited active patch is an entry retarget.
    let changed = if old.visual.disabled != new.visual.disabled {
        Some((StateKey::Disabled, new.visual.disabled))
    } else if let Some(key) = after.iter().rev().find(|key| !before.contains(key)) {
        Some((key.clone(), true))
    } else if let Some(key) = before.iter().rev().find(|key| !after.contains(key)) {
        Some((key.clone(), false))
    } else {
        after.last().cloned().map(|key| (key, true))
    };
    let Some((key, entering)) = changed else {
        return defaults.transition;
    };
    let patch = overrides.get(&key).copied().unwrap_or_default();
    if entering {
        patch.transition.unwrap_or(defaults.transition)
    } else {
        patch
            .transition_out
            .or(defaults.transition_out)
            .or(patch.transition)
            .unwrap_or(defaults.transition)
    }
}
