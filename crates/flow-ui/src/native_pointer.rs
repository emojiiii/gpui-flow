//! GPUI reports mouse transitions, not a full device snapshot. Retain only
//! observed buttons and timestamp receipt on one monotonic process clock.
use gpui::{MouseButton, MouseDownEvent, MouseMoveEvent, MousePressureEvent, MouseUpEvent};
use gpui_flow_core::Point;
use gpui_flow_interaction::{HitTarget, Modifiers, PointerButton, PointerInput};

/// Monotonic receipt time shared by native views and host-injected pointer input.
/// Use this domain for timed input passed to SceneView or LayeredApp.
pub fn pointer_timestamp_millis() -> u64 {
    static EPOCH: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    EPOCH
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_millis()
        .min(u64::MAX as u128) as u64
}
fn button(button: MouseButton) -> Option<PointerButton> {
    match button {
        MouseButton::Left => Some(PointerButton::Left),
        MouseButton::Middle => Some(PointerButton::Middle),
        MouseButton::Right => Some(PointerButton::Right),
        _ => None,
    }
}
fn mask(button: MouseButton) -> u16 {
    match button {
        MouseButton::Navigate(gpui::NavigationDirection::Back) => 8,
        MouseButton::Navigate(gpui::NavigationDirection::Forward) => 16,
        value => self::button(value).map_or(0, PointerButton::mask),
    }
}
#[derive(Default)]
pub(crate) struct NativePointerState {
    buttons: u16,
    pressure: Option<f32>,
}
impl NativePointerState {
    pub fn reset(&mut self) {
        self.buttons = 0;
        self.pressure = None;
    }
    fn input(
        &self,
        position: Point,
        hit: HitTarget,
        changed: Option<MouseButton>,
        modifiers: gpui::Modifiers,
        space: bool,
    ) -> PointerInput {
        PointerInput {
            position,
            hit,
            button: changed.and_then(button),
            buttons: Some(self.buttons),
            timestamp_millis: Some(pointer_timestamp_millis()),
            pressure: self.pressure,
            modifiers: Modifiers {
                shift: modifiers.shift,
                control: modifiers.control,
                alt: modifiers.alt,
                meta: modifiers.platform,
                space,
            },
            // The GPUI mouse interface cannot distinguish a physical mouse from
            // its synthesized touch taps. Pressure arrives separately when the platform supplies it.
            ..Default::default()
        }
    }
    pub fn down(
        &mut self,
        event: &MouseDownEvent,
        position: Point,
        hit: HitTarget,
        space: bool,
    ) -> PointerInput {
        if self.buttons == 0 {
            self.pressure = None;
        }
        self.buttons |= mask(event.button);
        self.input(position, hit, Some(event.button), event.modifiers, space)
    }
    pub fn moved(
        &mut self,
        event: &MouseMoveEvent,
        position: Point,
        hit: HitTarget,
        space: bool,
    ) -> PointerInput {
        if let Some(button) = event.pressed_button {
            self.buttons |= mask(button);
        } else {
            self.reset();
        }
        self.input(position, hit, event.pressed_button, event.modifiers, space)
    }
    /// GPUI's value is pressure within its current force-click stage, not a
    /// calibrated pen force. No pressure is synthesized from button state.
    pub fn pressure(
        &mut self,
        event: &MousePressureEvent,
        position: Point,
        hit: HitTarget,
        space: bool,
    ) -> PointerInput {
        if event.pressure.is_finite() && (0.0..=1.0).contains(&event.pressure) {
            self.pressure = Some(event.pressure);
        }
        let held = if self.buttons & 1 != 0 {
            Some(MouseButton::Left)
        } else if self.buttons & 2 != 0 {
            Some(MouseButton::Right)
        } else if self.buttons & 4 != 0 {
            Some(MouseButton::Middle)
        } else {
            None
        };
        let mut pointer = self.input(position, hit, held, event.modifiers, space);
        pointer.pressure = Some(event.pressure);
        pointer
    }
    pub fn up(
        &mut self,
        event: &MouseUpEvent,
        position: Point,
        hit: HitTarget,
        space: bool,
    ) -> PointerInput {
        self.buttons &= !mask(event.button);
        self.pressure = None;
        self.input(position, hit, Some(event.button), event.modifiers, space)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[::core::prelude::v1::test]
    fn observed_chord_release_and_missing_buttons_are_explicit() {
        let mut state = NativePointerState::default();
        let down = |button| MouseDownEvent {
            button,
            ..Default::default()
        };
        let first = state.down(
            &down(MouseButton::Left),
            Point::ZERO,
            HitTarget::Background,
            false,
        );
        assert_eq!(first.buttons, Some(1));
        let second = state.down(
            &down(MouseButton::Right),
            Point::ZERO,
            HitTarget::Background,
            false,
        );
        assert_eq!(second.buttons, Some(3));
        assert!(second.timestamp_millis >= first.timestamp_millis);
        assert_eq!(second.pressure, None);
        let up = state.up(
            &MouseUpEvent {
                button: MouseButton::Right,
                ..Default::default()
            },
            Point::ZERO,
            HitTarget::Background,
            false,
        );
        assert_eq!(up.buttons, Some(1));
        let lost = state.moved(
            &MouseMoveEvent::default(),
            Point::ZERO,
            HitTarget::Background,
            false,
        );
        assert_eq!(lost.buttons, Some(0));
        state.down(
            &down(MouseButton::Left),
            Point::ZERO,
            HitTarget::Background,
            false,
        );
        state.reset();
        assert_eq!(state.buttons, 0);
    }
    #[::core::prelude::v1::test]
    fn pressure_is_reported_only_after_a_real_pressure_observation_and_cleared_on_up() {
        let mut state = NativePointerState::default();
        let down = state.down(
            &MouseDownEvent::default(),
            Point::ZERO,
            HitTarget::Background,
            false,
        );
        assert_eq!(down.pressure, None);
        let pressure = state.pressure(
            &MousePressureEvent {
                pressure: 0.6,
                stage: gpui::PressureStage::Normal,
                ..Default::default()
            },
            Point::ZERO,
            HitTarget::Background,
            false,
        );
        assert_eq!(pressure.pressure, Some(0.6));
        assert_eq!(pressure.buttons, Some(1));
        let up = state.up(
            &MouseUpEvent::default(),
            Point::ZERO,
            HitTarget::Background,
            false,
        );
        assert_eq!(up.pressure, None);
        assert_eq!(up.buttons, Some(0));
    }
}
