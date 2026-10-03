use gpui_flow_interaction::scene::{StatePaint, StateStyles, VisualState};
use gpui_flow_scene::{Color, Style};

#[test]
fn state_paint_preserves_base_blur_without_adding_a_state_filter_api() {
    let base = Style {
        blur: 4.,
        ..Default::default()
    };
    let states = StateStyles {
        hover: StatePaint {
            fill: Some(Color::WHITE),
            opacity: Some(0.5),
        },
        ..Default::default()
    };
    let presented = states.resolve(
        &base,
        VisualState {
            hover: true,
            ..Default::default()
        },
    );
    assert_eq!(presented.blur, 4.);
    assert_eq!(presented.opacity, 0.5);
    assert_eq!(base.opacity, 1.);
}
