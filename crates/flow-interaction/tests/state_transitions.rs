use gpui_flow_animation::{AnimationController, AnimationSpec, Easing, Tween, TweenValue};
use gpui_flow_editor::SceneEditor;
use gpui_flow_engine::{ElementHandle, PropertyKey};
use gpui_flow_interaction::{scene::*, *};
use gpui_flow_scene::{Color, DisplayObject, Paint, Point, Scene, Style, display};
use serde_json::json;
use std::time::Duration;

struct Lab {
    editor: SceneEditor,
    host: ScenePluginHost,
    router: SceneEventRouter,
    sheet: SceneStateStyles,
    handle: ElementHandle,
}
impl Lab {
    fn new() -> Self {
        let mut scene = Scene::new();
        for id in ["a", "b"] {
            scene
                .insert(
                    DisplayObject::new(id, display::Rect::new(40., 30.))
                        .with_style(Style::fill(Color::BLACK)),
                    None,
                    None,
                )
                .unwrap();
        }
        let editor = SceneEditor::new(scene).unwrap();
        let handle = editor.engine.handle(&"a".into()).unwrap();
        let mut sheet = SceneStateStyles::new(&editor.engine);
        sheet
            .bind(
                &editor.engine,
                handle,
                StateStyles {
                    hover: StatePaint {
                        fill: Some(Color::WHITE),
                        opacity: Some(0.),
                    },
                    ..Default::default()
                },
            )
            .unwrap();
        sheet
            .set_transitions(
                &editor.engine,
                handle,
                StateTransitions {
                    transition: StateTransition::new(100),
                    transition_out: None,
                },
            )
            .unwrap();
        Self {
            editor,
            host: ScenePluginHost::new(),
            router: SceneEventRouter::new(),
            sheet,
            handle,
        }
    }
    fn event(&mut self, event: CanvasEvent) {
        self.router
            .dispatch(&mut self.editor, &mut self.host, event);
    }
    fn enter(&mut self) {
        self.event(CanvasEvent::PointerMove(pointer()));
    }
    fn leave(&mut self) {
        self.event(CanvasEvent::PointerLeave);
    }
    fn at(&mut self, ms: u64) -> Style {
        self.sheet
            .presentation_at(&self.editor, &self.router, Duration::from_millis(ms))
            .unwrap()
            .get(&"a".into())
            .unwrap()
            .object
            .style
            .clone()
    }
}
fn pointer() -> PointerInput {
    PointerInput::left(Point::new(10., 10.), HitTarget::Object("a".into()))
}
fn color(style: &Style) -> Color {
    match style.fill {
        Some(Paint::Solid { color }) => color,
        _ => panic!("expected solid fill"),
    }
}
#[test]
fn entry_exit_fallback_interpolate_exact_endpoints_without_edits() {
    let mut l = Lab::new();
    let original = l.editor.scene().clone();
    let revision = l.editor.engine.revision();
    l.enter();
    assert_eq!(color(&l.at(0)), Color::BLACK);
    assert!(l.sheet.has_active_transitions());
    assert_eq!(color(&l.at(50)), Color::rgb(128, 128, 128));
    assert_eq!(l.at(50).opacity, 0.5);
    assert_eq!(color(&l.at(100)), Color::WHITE);
    assert!(!l.sheet.has_active_transitions());
    l.leave();
    assert_eq!(color(&l.at(100)), Color::WHITE);
    assert_eq!(color(&l.at(150)), Color::rgb(128, 128, 128));
    assert_eq!(l.at(200), original.get(&"a".into()).unwrap().object.style);
    assert_eq!(l.editor.scene(), &original);
    assert_eq!(l.editor.engine.revision(), revision);
    assert!(!l.editor.undo());
}
#[test]
fn explicit_exit_duration_and_zero_override_entry() {
    let mut l = Lab::new();
    l.sheet
        .set_transitions(
            &l.editor.engine,
            l.handle,
            StateTransitions {
                transition: StateTransition::new(100),
                transition_out: Some(StateTransition::new(200)),
            },
        )
        .unwrap();
    l.enter();
    l.at(0);
    l.at(100);
    l.leave();
    l.at(100);
    assert_eq!(l.at(200).opacity, 0.5);
    assert_eq!(l.at(300).opacity, 1.);
    l.sheet
        .set_transitions(
            &l.editor.engine,
            l.handle,
            StateTransitions {
                transition: StateTransition::new(100),
                transition_out: Some(StateTransition::new(0)),
            },
        )
        .unwrap();
    l.enter();
    l.at(300);
    l.at(400);
    l.leave();
    assert_eq!(l.at(400).opacity, 1.);
    assert!(!l.sheet.has_active_transitions());
}
#[test]
fn rapid_reversal_samples_current_value_without_jumps_or_restarting_each_frame() {
    let mut l = Lab::new();
    l.enter();
    l.at(0);
    // Last rendered at 0, but the reversal happens at 40: sample the old motion
    // at the new host time, not the previous rendered value.
    l.leave();
    let reverse = l.at(40);
    assert_eq!(color(&reverse), Color::rgb(102, 102, 102));
    assert!((reverse.opacity - 0.6).abs() < 1e-6);
    assert!((l.at(90).opacity - 0.8).abs() < 1e-6);
    l.enter();
    assert!((l.at(90).opacity - 0.8).abs() < 1e-6);
    assert!((l.at(140).opacity - 0.4).abs() < 1e-6);
    assert_eq!(l.at(190).opacity, 0.);
}
#[test]
fn disabled_priority_and_cancel_retarget_current_display() {
    let mut l = Lab::new();
    l.sheet
        .bind(
            &l.editor.engine,
            l.handle,
            StateStyles {
                hover: StatePaint {
                    fill: Some(Color::WHITE),
                    opacity: Some(0.),
                },
                press: StatePaint::fill(Color::rgb(200, 0, 0)),
                disabled: StatePaint {
                    fill: Some(Color::BLACK),
                    opacity: Some(0.5),
                },
                ..Default::default()
            },
        )
        .unwrap();
    l.enter();
    l.at(0);
    l.at(50);
    l.event(CanvasEvent::PointerDown(pointer()));
    assert_eq!(color(&l.at(50)), Color::rgb(128, 128, 128));
    // Opacity target stays zero and must not restart with the press color.
    assert_eq!(l.at(100).opacity, 0.);
    l.sheet
        .set_disabled(&l.editor.engine, l.handle, true)
        .unwrap();
    let from = l.at(100);
    assert!(l.sheet.has_active_transitions());
    assert_eq!(l.at(150).opacity, 0.25);
    assert_ne!(color(&from), Color::BLACK);
    assert_eq!(color(&l.at(200)), Color::BLACK);
    l.sheet
        .set_disabled(&l.editor.engine, l.handle, false)
        .unwrap();
    l.event(CanvasEvent::PointerCancel);
    l.at(200);
    assert_eq!(l.at(300).opacity, 1.);
}
#[test]
fn canonical_edit_cancels_only_changed_channel_preserves_undo_and_other_motion() {
    let mut l = Lab::new();
    l.enter();
    l.at(0);
    l.at(25);
    l.editor
        .set_property(
            l.handle,
            PropertyKey::Fill,
            json!(Paint::solid(Color::rgb(200, 0, 0))),
        )
        .unwrap();
    let style = l.at(50);
    assert_eq!(color(&style), Color::WHITE); // active override remains, old color tween is retired
    assert_eq!(style.opacity, 0.5); // independent opacity tween still runs
    l.leave();
    l.at(50);
    l.at(150);
    assert_eq!(color(&l.at(150)), Color::rgb(200, 0, 0));
    assert!(l.editor.undo());
    assert_eq!(color(&l.at(160)), Color::BLACK);
    assert!(!l.editor.undo());
    assert!(l.editor.redo());
    assert_eq!(color(&l.at(170)), Color::rgb(200, 0, 0));
}
#[test]
fn animation_and_state_motion_keep_separate_clocks_and_history() {
    let mut l = Lab::new();
    l.editor
        .set_property(l.handle, PropertyKey::Opacity, json!(0.8))
        .unwrap();
    let token = l.editor.begin_playback().unwrap();
    let mut animations = AnimationController::new(&l.editor.engine);
    animations
        .animate(
            &l.editor.engine,
            l.handle,
            AnimationSpec::new(100, vec![Tween::opacity(0.4)]),
        )
        .unwrap();
    l.enter();
    l.at(0);
    animations
        .tick_editor(&mut l.editor, token, Duration::from_millis(50))
        .unwrap();
    let rendered = l.at(50);
    assert_eq!(rendered.opacity, 0.); // live canonical opacity changed: cancel its stale transition
    assert_eq!(color(&rendered), Color::rgb(128, 128, 128));
    l.leave();
    l.at(50);
    animations
        .tick_editor(&mut l.editor, token, Duration::from_millis(50))
        .unwrap();
    assert_eq!(l.at(100).opacity, 0.4); // leaving motion cannot overwrite a new animation value
    animations.finish_editor(&mut l.editor, token).unwrap();
    assert!(l.editor.undo());
    assert_eq!(l.at(110).opacity, 0.8);
    assert!(l.editor.undo());
    assert_eq!(l.at(120).opacity, 1.);
    assert!(!l.editor.undo());
}
#[test]
fn attachment_epoch_cancels_motion_even_without_detached_frame_and_reset_prunes() {
    let mut l = Lab::new();
    l.enter();
    l.at(0);
    l.at(40);
    let (subtree, _) = l.editor.engine.detach(l.handle).unwrap();
    l.editor.engine.attach(&subtree, None, None).unwrap();
    assert_eq!(color(&l.at(50)), Color::BLACK);
    assert!(!l.sheet.has_active_transitions());
    l.enter();
    l.at(50);
    assert_eq!(l.at(100).opacity, 0.5);
    l.editor
        .engine
        .reset_scene(l.editor.scene().clone())
        .unwrap();
    assert_eq!(color(&l.at(110)), Color::BLACK);
    assert!(l.sheet.is_empty());
    assert!(!l.sheet.has_active_transitions());
}
#[test]
fn replacement_retargets_but_unbind_clear_and_finish_release_motion() {
    let mut l = Lab::new();
    l.enter();
    l.at(0);
    l.at(50);
    assert!(
        l.sheet
            .bind(
                &l.editor.engine,
                l.handle,
                StateStyles {
                    hover: StatePaint {
                        opacity: Some(f32::NAN),
                        ..Default::default()
                    },
                    ..Default::default()
                }
            )
            .is_err()
    );
    assert_eq!(l.at(60).opacity, 0.4);
    l.sheet
        .bind(
            &l.editor.engine,
            l.handle,
            StateStyles {
                hover: StatePaint {
                    opacity: Some(0.8),
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(l.at(60).opacity, 0.4);
    assert!((l.at(110).opacity - 0.6).abs() < 1e-6);
    l.sheet.finish_transitions();
    assert_eq!(l.at(110).opacity, 0.8);
    assert!(!l.sheet.has_active_transitions());
    l.sheet.unbind(l.handle);
    assert_eq!(l.at(120).opacity, 1.);
    assert!(!l.sheet.has_active_transitions());
    l.sheet.clear();
}
#[test]
fn invalid_frame_keeps_clock_and_all_transitions_atomic_then_recovers() {
    let mut l = Lab::new();
    let b = l.editor.engine.handle(&"b".into()).unwrap();
    l.sheet
        .bind(
            &l.editor.engine,
            b,
            StateStyles {
                selected: StatePaint::fill(Color::WHITE),
                ..Default::default()
            },
        )
        .unwrap();
    l.enter();
    l.at(0);
    l.editor.selection.ids.insert("b".into());
    l.editor
        .engine
        .transaction(|scene| {
            scene.update_object(&"b".into(), |o| {
                o.style.fill = None;
                o.style.fill_layers = vec![Color::BLACK.into(); 32];
            })
        })
        .unwrap();
    assert!(
        l.sheet
            .presentation_at(&l.editor, &l.router, Duration::from_millis(90))
            .is_err()
    );
    assert_eq!(l.sheet.transition_time(), Duration::ZERO);
    l.editor.selection.ids.clear();
    assert_eq!(l.at(50).opacity, 0.5); // failed time 90 was not accepted
    assert!(l.sheet.has_active_transitions());
    assert!(matches!(
        l.sheet
            .presentation_at(&l.editor, &l.router, Duration::from_millis(49)),
        Err(StateStyleError::TimeWentBackwards)
    ));
    assert_eq!(l.sheet.transition_time(), Duration::from_millis(50));
}
#[test]
fn missing_fill_snaps_discretely_while_opacity_interpolates_and_timing_change_finishes() {
    let mut l = Lab::new();
    l.editor
        .engine
        .set_property(l.handle, PropertyKey::Fill, serde_json::Value::Null)
        .unwrap();
    l.enter();
    assert_eq!(color(&l.at(0)), Color::WHITE);
    assert_eq!(l.at(50).opacity, 0.5);
    l.sheet
        .set_transitions(&l.editor.engine, l.handle, StateTransitions::default())
        .unwrap();
    assert_eq!(l.at(50).opacity, 0.);
    l.leave();
    assert_eq!(l.at(50).fill, None);
    assert_eq!(l.at(50).opacity, 1.);
}
#[test]
fn easing_uses_shared_sampler_rgba_channels_and_large_times_are_finite() {
    let mut l = Lab::new();
    l.sheet
        .bind(
            &l.editor.engine,
            l.handle,
            StateStyles {
                hover: StatePaint::fill(Color::rgba(200, 100, 40, 55)),
                ..Default::default()
            },
        )
        .unwrap();
    l.sheet
        .set_transitions(
            &l.editor.engine,
            l.handle,
            StateTransitions {
                transition: StateTransition {
                    duration_ms: 100,
                    easing: Easing::EaseIn,
                },
                transition_out: None,
            },
        )
        .unwrap();
    l.enter();
    l.at(0);
    assert_eq!(color(&l.at(50)), Color::rgba(25, 13, 5, 230));
    assert_eq!(color(&l.at(u64::MAX)), Color::rgba(200, 100, 40, 55));
    assert!(!l.sheet.has_active_transitions());
}
#[test]
fn public_pure_sampler_rejects_invalid_inputs_and_preserves_exact_endpoints() {
    let value = TweenValue::Number(2.);
    assert!(
        value
            .interpolate(&TweenValue::Number(4.), f64::NAN, Easing::Linear)
            .is_err()
    );
    assert!(
        value
            .interpolate(&TweenValue::Number(f64::INFINITY), 0., Easing::Linear)
            .is_err()
    );
    assert!(
        value
            .interpolate(&TweenValue::Color(Color::BLACK), 0., Easing::Linear)
            .is_err()
    );
    assert_eq!(
        value
            .interpolate(&TweenValue::Number(4.), -1., Easing::Linear)
            .unwrap(),
        value
    );
    assert_eq!(
        value
            .interpolate(&TweenValue::Number(4.), 2., Easing::Linear)
            .unwrap(),
        TweenValue::Number(4.)
    );
}

#[test]
fn maximum_duration_and_zero_duration_are_finite_and_configuration_finishes_same_target() {
    let mut l = Lab::new();
    l.sheet
        .set_transitions(
            &l.editor.engine,
            l.handle,
            StateTransitions {
                transition: StateTransition::new(u64::MAX),
                transition_out: None,
            },
        )
        .unwrap();
    l.enter();
    assert_eq!(l.at(0).opacity, 1.);
    let midway = l.at(u64::MAX / 2);
    assert!(midway.opacity.is_finite());
    assert!((midway.opacity - 0.5).abs() < 1e-6);
    assert!(l.sheet.has_active_transitions());
    // Changing timing is an explicit finish, even when the resolved target is unchanged.
    l.sheet
        .set_transitions(&l.editor.engine, l.handle, StateTransitions::default())
        .unwrap();
    assert!(!l.sheet.has_active_transitions());
    assert_eq!(l.at(u64::MAX / 2).opacity, 0.);
    l.leave();
    let immediate = l.at(u64::MAX / 2);
    assert_eq!(immediate.opacity, 1.);
    assert!(!l.sheet.has_active_transitions());
}

#[test]
fn failed_later_binding_does_not_commit_an_earlier_new_transition() {
    let mut l = Lab::new();
    let b = l.editor.engine.handle(&"b".into()).unwrap();
    l.sheet
        .bind(
            &l.editor.engine,
            b,
            StateStyles {
                selected: StatePaint::fill(Color::WHITE),
                ..Default::default()
            },
        )
        .unwrap();
    l.at(0);
    l.enter();
    l.editor.selection.ids.insert("b".into());
    l.editor
        .engine
        .transaction(|scene| {
            scene.update_object(&"b".into(), |o| {
                o.style.fill = None;
                o.style.fill_layers = vec![Color::BLACK.into(); 32];
            })
        })
        .unwrap();
    assert!(
        l.sheet
            .presentation_at(&l.editor, &l.router, Duration::from_millis(10))
            .is_err()
    );
    assert!(!l.sheet.has_active_transitions());
    assert_eq!(l.sheet.transition_time(), Duration::ZERO);
    l.editor.selection.ids.clear();
    assert_eq!(l.at(20).opacity, 1.); // must start at 20, not failed frame 10
    assert_eq!(l.at(70).opacity, 0.5);
}
