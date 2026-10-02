use gpui_flow_core::Point;
use std::time::Duration;

/// Logical-pixel velocity with exponential damping. Disabled unless explicitly
/// supplied to ScenePanPlugin. No global timer or wall clock is consulted.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanInertiaOptions {
    /// Velocity multiplier exp(-decay_per_second * elapsed_seconds).
    pub decay_per_second: f32,
    pub stop_speed: f32,
    pub max_speed: f32,
    /// Release must follow the last moving sample within this interval.
    pub release_window: Duration,
}
impl Default for PanInertiaOptions {
    fn default() -> Self {
        Self {
            decay_per_second: 8.0,
            stop_speed: 8.0,
            max_speed: 4000.0,
            release_window: Duration::from_millis(100),
        }
    }
}
impl PanInertiaOptions {
    pub fn is_valid(self) -> bool {
        self.decay_per_second.is_finite()
            && self.decay_per_second > 0.0
            && self.stop_speed.is_finite()
            && self.stop_speed > 0.0
            && self.max_speed.is_finite()
            && self.max_speed >= self.stop_speed
            && !self.release_window.is_zero()
    }
}
/// Deterministic continuous-time integrator. Integrating one long frame gives
/// the same displacement as partitioning it (up to f32 output rounding).
#[derive(Debug, Clone)]
pub struct PanInertia {
    options: PanInertiaOptions,
    velocity: Point,
    time: Duration,
}
impl PanInertia {
    pub fn new(velocity: Point, now: Duration, options: PanInertiaOptions) -> Option<Self> {
        if !options.is_valid() || !velocity.is_finite() {
            return None;
        }
        let speed = (velocity.x as f64).hypot(velocity.y as f64);
        if speed < options.stop_speed as f64 {
            return None;
        }
        let scale = (options.max_speed as f64 / speed).min(1.0) as f32;
        Some(Self {
            options,
            velocity: velocity * scale,
            time: now,
        })
    }
    pub fn velocity(&self) -> Point {
        self.velocity
    }
    pub fn is_active(&self) -> bool {
        self.velocity != Point::ZERO
    }
    pub fn stop(&mut self) {
        self.velocity = Point::ZERO;
    }
    /// A backwards clock cancels movement. A zero-duration frame is a no-op.
    pub fn advance(&mut self, now: Duration) -> Point {
        let Some(elapsed) = now.checked_sub(self.time) else {
            self.stop();
            return Point::ZERO;
        };
        self.time = now;
        if !self.is_active() || elapsed.is_zero() {
            return Point::ZERO;
        }
        let decay = self.options.decay_per_second as f64;
        let speed = (self.velocity.x as f64).hypot(self.velocity.y as f64);
        let until_stop = (speed / self.options.stop_speed as f64).ln().max(0.0) / decay;
        let seconds = elapsed.as_secs_f64().min(until_stop);
        let multiplier = (-decay * seconds).exp();
        let distance = -(-decay * seconds).exp_m1() / decay;
        let delta = Point::new(
            (self.velocity.x as f64 * distance) as f32,
            (self.velocity.y as f64 * distance) as f32,
        );
        if !delta.is_finite() {
            self.stop();
            return Point::ZERO;
        }
        if elapsed.as_secs_f64() >= until_stop {
            self.stop();
        } else {
            self.velocity = self.velocity * multiplier as f32;
        }
        delta
    }
}
/// Policy for normalized logical-pixel wheel input. It makes no inference about
/// whether a platform event came from a mouse wheel or a touchpad.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SceneWheelPolicy {
    #[default]
    CommandZoom,
    PanOnly,
    ZoomOnly,
    Disabled,
}
impl SceneWheelPolicy {
    pub(crate) fn pans(self, command: bool) -> bool {
        self == Self::PanOnly || (self == Self::CommandZoom && !command)
    }
    pub(crate) fn zooms(self, command: bool) -> bool {
        self == Self::ZoomOnly || (self == Self::CommandZoom && command)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScenePanPolicy {
    pub background_drag: bool,
    pub space_drag: bool,
    pub middle_drag: bool,
    pub wheel: SceneWheelPolicy,
    /// Shift-wheel vertical deltas become horizontal; default preserves legacy behavior.
    pub shift_wheel_horizontal: bool,
}
impl Default for ScenePanPolicy {
    fn default() -> Self {
        Self {
            background_drag: true,
            space_drag: true,
            middle_drag: true,
            wheel: SceneWheelPolicy::default(),
            shift_wheel_horizontal: false,
        }
    }
}
