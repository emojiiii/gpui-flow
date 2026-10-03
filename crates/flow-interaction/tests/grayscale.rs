use gpui_flow_interaction::scene::{StatePaint, StateStyles, VisualState};
use gpui_flow_scene::{Color, Style};

#[test]
fn state_paint_preserves_grayscale_without_expanding_its_api() {
    let base = Style {
        grayscale: 0.75,
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
    assert_eq!(presented.grayscale, 0.75);
    assert_eq!(presented.opacity, 0.5);
    assert_eq!(base.opacity, 1.);
}
