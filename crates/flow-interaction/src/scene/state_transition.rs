use super::state_style::{
    geometry::{shape_size, shape_size_mut},
    timing::timing,
};
use super::{ResolvedVisualState, StateKey, StateTransitionOverrides};
use gpui_flow_animation::{Easing, TweenValue};
use gpui_flow_engine::ElementHandle;
use gpui_flow_scene::{DisplayObject, Paint};
use std::collections::BTreeMap;
use std::time::Duration;

/// A paint and explicit-geometry transition. Zero duration applies the target immediately.
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
    pub button: Option<(ElementHandle, u64)>,
    state: ResolvedVisualState,
    fill: Channel<Option<Paint>>,
    opacity: Channel<f32>,
    x: Channel<f32>,
    y: Channel<f32>,
    width: Option<Channel<f32>>,
    height: Option<Channel<f32>>,
}
impl TransitionFrame {
    pub fn new(epoch: u64, button: Option<(ElementHandle, u64)>, base: &DisplayObject) -> Self {
        Self {
            epoch,
            button,
            state: ResolvedVisualState::default(),
            fill: Channel::new(base.style.fill.clone()),
            opacity: Channel::new(base.style.opacity),
            x: Channel::new(base.transform.tx),
            y: Channel::new(base.transform.ty),
            width: shape_size(&base.shape).map(|s| Channel::new(s.width)),
            height: shape_size(&base.shape).map(|s| Channel::new(s.height)),
        }
    }
    pub fn resolve(
        &mut self,
        base: &DisplayObject,
        mut target: DisplayObject,
        state: ResolvedVisualState,
        transitions: StateTransitions,
        overrides: &BTreeMap<StateKey, StateTransitionOverrides>,
        now: Duration,
    ) -> DisplayObject {
        let spec = timing(&self.state, &state, transitions, overrides);
        target.style.fill = self
            .fill
            .update(&base.style.fill, target.style.fill, spec, now);
        target.style.opacity =
            self.opacity
                .update(&base.style.opacity, target.style.opacity, spec, now);
        target.transform.tx = self
            .x
            .update(&base.transform.tx, target.transform.tx, spec, now);
        target.transform.ty = self
            .y
            .update(&base.transform.ty, target.transform.ty, spec, now);
        if let (Some(base_size), Some(size)) =
            (shape_size(&base.shape), shape_size_mut(&mut target.shape))
        {
            size.width = self
                .width
                .get_or_insert_with(|| Channel::new(base_size.width))
                .update(&base_size.width, size.width, spec, now);
            size.height = self
                .height
                .get_or_insert_with(|| Channel::new(base_size.height))
                .update(&base_size.height, size.height, spec, now);
        } else {
            self.width = None;
            self.height = None;
        }
        self.state = state;
        target
    }
    pub fn active(&self) -> bool {
        self.fill.active
            || self.opacity.active
            || self.x.active
            || self.y.active
            || self.width.as_ref().is_some_and(|c| c.active)
            || self.height.as_ref().is_some_and(|c| c.active)
    }
    pub fn finish(&mut self) {
        self.fill.active = false;
        self.opacity.active = false;
        self.x.active = false;
        self.y.active = false;
        if let Some(c) = &mut self.width {
            c.active = false;
        }
        if let Some(c) = &mut self.height {
            c.active = false;
        }
    }
}
