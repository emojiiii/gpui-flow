use super::VisualState;
use gpui_flow_animation::{Easing, TweenValue};
use gpui_flow_scene::{Paint, Style};
use std::time::Duration;

/// A paint-only transition. Zero duration applies the target immediately.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct StateTransition {
    pub duration_ms: u64,
    pub easing: Easing,
}
impl StateTransition {
    pub fn new(duration_ms: u64) -> Self {
        Self {
            duration_ms,
            easing: Easing::Linear,
        }
    }
}

/// Per-binding transition defaults. An omitted exit uses the entry settings;
/// an explicit zero-duration exit disables exit motion.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct StateTransitions {
    pub transition: StateTransition,
    pub transition_out: Option<StateTransition>,
}
impl StateTransitions {
    fn choose(self, old: VisualState, new: VisualState) -> StateTransition {
        let entering = if new.disabled != old.disabled {
            new.disabled
        } else {
            (new.selected && !old.selected)
                || (new.focus && !old.focus)
                || (new.hover && !old.hover)
                || (new.press && !old.press)
                || new == old // changing an active patch is an entry retarget
        };
        if entering {
            self.transition
        } else {
            self.transition_out.unwrap_or(self.transition)
        }
    }
}

trait Value: Clone + PartialEq {
    fn sample(from: &Self, to: &Self, progress: f64, easing: Easing) -> Option<Self>;
}
impl Value for f32 {
    fn sample(from: &Self, to: &Self, progress: f64, easing: Easing) -> Option<Self> {
        match TweenValue::Number(*from as f64)
            .interpolate(&TweenValue::Number(*to as f64), progress, easing)
            .ok()?
        {
            TweenValue::Number(value) => Some(value as f32),
            _ => None,
        }
    }
}
impl Value for Option<Paint> {
    fn sample(from: &Self, to: &Self, progress: f64, easing: Easing) -> Option<Self> {
        let (Some(Paint::Solid { color: a }), Some(Paint::Solid { color: b })) = (from, to) else {
            return None;
        };
        match TweenValue::Color(*a)
            .interpolate(&TweenValue::Color(*b), progress, easing)
            .ok()?
        {
            TweenValue::Color(color) => Some(Some(color.into())),
            _ => None,
        }
    }
}

#[derive(Clone)]
struct Channel<T> {
    base: T,
    from: T,
    target: T,
    start: Duration,
    spec: StateTransition,
    active: bool,
}
impl<T: Value> Channel<T> {
    fn new(base: T) -> Self {
        Self {
            base: base.clone(),
            from: base.clone(),
            target: base,
            start: Duration::ZERO,
            spec: StateTransition::default(),
            active: false,
        }
    }
    fn sample(&self, now: Duration) -> T {
        if !self.active {
            return self.target.clone();
        }
        let elapsed = now.saturating_sub(self.start).as_secs_f64();
        T::sample(
            &self.from,
            &self.target,
            elapsed / Duration::from_millis(self.spec.duration_ms).as_secs_f64(),
            self.spec.easing,
        )
        .unwrap_or_else(|| self.target.clone())
    }
    fn update(&mut self, base: &T, target: T, spec: StateTransition, now: Duration) -> T {
        let current = self.sample(now);
        if *base != self.base {
            // A canonical edit/animation owns this channel. Do not animate stale
            // document values back over it or chase a moving animation endpoint.
            self.base = base.clone();
            self.target = target;
            self.active = false;
        } else if target != self.target {
            self.from = current;
            self.target = target;
            self.spec = spec;
            self.start = now;
            self.active = spec.duration_ms > 0
                && self.from != self.target
                && T::sample(&self.from, &self.target, 0., spec.easing).is_some();
        }
        if self.active
            && now.saturating_sub(self.start) >= Duration::from_millis(self.spec.duration_ms)
        {
            self.active = false;
        }
        self.sample(now)
    }
}

#[derive(Clone)]
pub(super) struct TransitionFrame {
    pub epoch: u64,
    state: VisualState,
    fill: Channel<Option<Paint>>,
    opacity: Channel<f32>,
}
impl TransitionFrame {
    pub fn new(epoch: u64, base: &Style) -> Self {
        Self {
            epoch,
            state: VisualState::default(),
            fill: Channel::new(base.fill.clone()),
            opacity: Channel::new(base.opacity),
        }
    }
    pub fn resolve(
        &mut self,
        base: &Style,
        mut target: Style,
        state: VisualState,
        transitions: StateTransitions,
        now: Duration,
    ) -> Style {
        let spec = transitions.choose(self.state, state);
        target.fill = self.fill.update(&base.fill, target.fill, spec, now);
        target.opacity = self
            .opacity
            .update(&base.opacity, target.opacity, spec, now);
        self.state = state;
        target
    }
    pub fn active(&self) -> bool {
        self.fill.active || self.opacity.active
    }
    pub fn finish(&mut self) {
        self.fill.active = false;
        self.opacity.active = false;
    }
}
