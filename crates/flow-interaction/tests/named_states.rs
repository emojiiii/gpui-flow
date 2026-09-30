//! Source-only regressions for named/placeholder/button state presentation.
use gpui_flow_editor::SceneEditor;
use gpui_flow_engine::{ElementHandle, PropertyKey};
use gpui_flow_interaction::{scene::*, *};
use gpui_flow_scene::{Color, DisplayObject, Paint, Point, Scene, Shape, Size, Style, display};
use serde_json::json;
use std::{borrow::Cow, collections::BTreeMap, time::Duration};

const NAMED: Color = Color::hex(0x111111);
const SELECTED: Color = Color::hex(0x222222);
const PLACEHOLDER: Color = Color::hex(0x333333);
const FOCUS: Color = Color::hex(0x444444);
const HOVER: Color = Color::hex(0x555555);
const PRESS: Color = Color::hex(0x666666);
const DISABLED: Color = Color::hex(0x777777);

struct Lab {
    editor: SceneEditor,
    host: ScenePluginHost,
    router: SceneEventRouter,
    sheet: SceneStateStyles,
}
impl Lab {
    fn new() -> Self {
        let mut scene = Scene::new();
        for (id, parent, shape) in [
            ("root", None, Shape::from(display::Frame::new(300., 200.))),
            (
                "middle",
                Some("root"),
                Shape::from(display::Frame::new(200., 100.)),
            ),
            (
                "deep",
                Some("middle"),
                Shape::from(display::Frame::new(150., 80.)),
            ),
            (
                "a",
                Some("root"),
                Shape::from(display::Text::new("", Size::new(80., 30.))),
            ),
            ("b", Some("root"), Shape::from(display::Rect::new(40., 30.))),
            (
                "grandchild",
                Some("middle"),
                Shape::from(display::Rect::new(20., 20.)),
            ),
            (
                "great_grandchild",
                Some("deep"),
                Shape::from(display::Rect::new(10., 10.)),
            ),
        ] {
            scene
                .insert(
                    DisplayObject::new(id, shape).with_style(Style::fill(Color::BLACK)),
                    parent.map(Into::into),
                    None,
                )
                .unwrap();
        }
        let editor = SceneEditor::new(scene).unwrap();
        let mut sheet = SceneStateStyles::new(&editor.engine);
        for id in [
            "root",
            "middle",
            "deep",
            "a",
            "b",
            "grandchild",
            "great_grandchild",
        ] {
            let handle = editor.engine.handle(&id.into()).unwrap();
            sheet
                .bind(&editor.engine, handle, StateStyles::default())
                .unwrap();
        }
        Self {
            editor,
            host: ScenePluginHost::new(),
            router: SceneEventRouter::new(),
            sheet,
        }
    }
    fn handle(&self, id: &str) -> ElementHandle {
        self.editor.engine.handle(&id.into()).unwrap()
    }
    fn palette(&mut self, id: &str, entries: &[(&str, StatePaint)]) {
        self.sheet
            .set_named_states(
                &self.editor.engine,
                self.handle(id),
                entries
                    .iter()
                    .map(|(name, paint)| (name.to_string(), paint.clone()))
                    .collect(),
            )
            .unwrap();
    }
    fn select(&mut self, id: &str, name: Option<&str>) {
        self.sheet
            .select_state(&self.editor.engine, self.handle(id), name)
            .unwrap();
    }
    fn button(&mut self, id: &str) {
        self.sheet
            .set_button(&self.editor.engine, self.handle(id), true)
            .unwrap();
    }
    fn event(&mut self, event: CanvasEvent) {
        self.router
            .dispatch(&mut self.editor, &mut self.host, event);
    }
    fn at(&mut self, id: &str, ms: u64) -> DisplayObject {
        self.sheet
            .presentation_at(&self.editor, &self.router, Duration::from_millis(ms))
            .unwrap()
            .get(&id.into())
            .unwrap()
            .object
            .clone()
    }
    fn state(&self, id: &str) -> ResolvedVisualState {
        self.sheet
            .resolved_state(&self.editor, &self.router, self.handle(id))
            .unwrap()
    }
    fn placeholder(&mut self, content: &str) {
        self.sheet
            .set_placeholder(
                &self.editor.engine,
                self.handle("a"),
                Some(TextPlaceholder::new(content, StatePaint::fill(PLACEHOLDER))),
            )
            .unwrap();
    }
}
fn pointer(id: &str) -> PointerInput {
    PointerInput::left(Point::new(10., 10.), HitTarget::Object(id.into()))
}
fn color(object: &DisplayObject) -> Color {
    match object.style.fill {
        Some(Paint::Solid { color }) => color,
        _ => panic!("solid fill required"),
    }
}
fn content(object: &DisplayObject) -> &str {
    let Shape::Text(text) = &object.shape else {
        panic!("text required")
    };
    &text.content
}
fn builtin_styles() -> StateStyles {
    StateStyles {
        selected: StatePaint::fill(SELECTED),
        focus: StatePaint::fill(FOCUS),
        hover: StatePaint::fill(HOVER),
        press: StatePaint::fill(PRESS),
        disabled: StatePaint::fill(DISABLED),
    }
}
fn opacity(value: f32) -> StatePaint {
    StatePaint {
        opacity: Some(value),
        ..Default::default()
    }
}

#[test]
fn named_selected_placeholder_focus_hover_press_disabled_follow_upstream_priority() {
    let mut l = Lab::new();
    let a = l.handle("a");
    l.sheet.bind(&l.editor.engine, a, builtin_styles()).unwrap();
    l.palette(
        "a",
        &[(
            "busy",
            StatePaint {
                fill: Some(NAMED),
                opacity: Some(0.7),
            },
        )],
    );
    l.select("a", Some("busy"));
    assert_eq!(color(&l.at("a", 0)), NAMED);
    l.editor.selection.select("a".into(), false);
    assert_eq!(color(&l.at("a", 0)), SELECTED);
    l.placeholder("Type here");
    assert_eq!(color(&l.at("a", 0)), PLACEHOLDER);
    assert_eq!(content(&l.at("a", 0)), "Type here");
    l.event(CanvasEvent::PointerDown(pointer("a")));
    assert_eq!(color(&l.at("a", 0)), PRESS);
    l.event(CanvasEvent::PointerUp(pointer("a")));
    assert_eq!(color(&l.at("a", 0)), FOCUS);
    l.event(CanvasEvent::PointerMove(pointer("a")));
    assert_eq!(color(&l.at("a", 0)), HOVER);
    l.sheet.set_disabled(&l.editor.engine, a, true).unwrap();
    assert_eq!(color(&l.at("a", 0)), DISABLED);
    assert_eq!(l.at("a", 0).style.opacity, 0.7);
    l.sheet
        .bind(
            &l.editor.engine,
            a,
            StateStyles {
                disabled: StatePaint::default(),
                ..builtin_styles()
            },
        )
        .unwrap();
    assert_eq!(color(&l.at("a", 0)), PLACEHOLDER); // empty disabled still suppresses input layers
    assert!(l.state("a").visual.hover); // visual suppression does not rewrite router input
}

#[test]
fn missing_selection_is_atomic_clear_restores_base_and_palette_removal_is_safe() {
    let mut l = Lab::new();
    let a = l.handle("a");
    l.palette("a", &[("busy", StatePaint::fill(NAMED))]);
    l.select("a", Some("busy"));
    assert_eq!(
        l.sheet.select_state(&l.editor.engine, a, Some("typo")),
        Err(StateStyleError::MissingState("typo".into()))
    );
    assert_eq!(l.state("a").named.as_deref(), Some("busy"));
    assert_eq!(color(&l.at("a", 0)), NAMED);
    l.palette("a", &[]);
    assert_eq!(l.state("a").named.as_deref(), Some("busy"));
    assert_eq!(color(&l.at("a", 0)), Color::BLACK);
    l.palette("a", &[("busy", StatePaint::fill(NAMED))]);
    assert_eq!(color(&l.at("a", 0)), NAMED);
    l.select("a", None);
    assert_eq!(l.state("a").named, None);
    assert!(matches!(
        l.sheet.presentation(&l.editor, &l.router).unwrap(),
        Cow::Borrowed(_)
    ));
    l.sheet.clear();
    assert!(!l.editor.undo());
}

#[test]
fn placeholder_renders_in_fixed_box_without_document_text_hit_or_history_changes() {
    let mut l = Lab::new();
    let original = l.editor.scene().clone();
    let revision = l.editor.engine.revision();
    let hit = l
        .editor
        .engine
        .hit_test(Point::new(10., 10.), Default::default())
        .unwrap();
    let a = l.handle("a");
    let bounds = l.editor.engine.object(a).unwrap().shape.local_bounds();
    l.placeholder("Placeholder actually reaches the paint list");
    let presented = l.sheet.presentation(&l.editor, &l.router).unwrap();
    let item = presented
        .paint_list()
        .unwrap()
        .into_iter()
        .find(|item| item.object.id.as_str() == "a")
        .unwrap();
    assert_eq!(
        content(item.object),
        "Placeholder actually reaches the paint list"
    );
    assert_eq!(item.object.shape.local_bounds(), bounds);
    assert_eq!(l.editor.scene(), &original);
    assert_eq!(l.editor.engine.revision(), revision);
    assert_eq!(
        l.editor
            .engine
            .hit_test(Point::new(10., 10.), Default::default())
            .unwrap(),
        hit
    );
    assert!(!l.editor.undo());
    l.editor
        .transaction(|scene| {
            scene.update_object(&"a".into(), |object| {
                if let Shape::Text(text) = &mut object.shape {
                    text.content = " ".into();
                }
            })
        })
        .unwrap();
    assert!(!l.state("a").placeholder);
    assert_eq!(content(&l.at("a", 0)), " ");
    assert_eq!(color(&l.at("a", 0)), Color::BLACK);
    assert!(l.editor.undo());
    assert!(l.state("a").placeholder);
    l.sheet.set_placeholder(&l.editor.engine, a, None).unwrap();
    assert_eq!(content(&l.at("a", 0)), "");
    assert!(!l.editor.undo());
    l.placeholder("");
    assert!(!l.state("a").placeholder);
}

#[test]
fn inherited_button_uses_child_palette_and_local_name_overrides_until_cleared() {
    let mut l = Lab::new();
    l.button("root");
    l.palette("root", &[("busy", StatePaint::fill(Color::WHITE))]);
    l.palette(
        "a",
        &[
            ("busy", StatePaint::fill(NAMED)),
            ("local", StatePaint::fill(SELECTED)),
        ],
    );
    l.select("root", Some("busy"));
    assert_eq!(l.state("a").button, Some(l.handle("root")));
    assert_eq!(l.state("a").named.as_deref(), Some("busy"));
    assert_eq!(color(&l.at("a", 0)), NAMED);
    assert_eq!(color(&l.at("b", 0)), Color::BLACK); // missing child palette never borrows parent paint
    l.select("a", Some("local"));
    assert_eq!(color(&l.at("a", 0)), SELECTED);
    l.select("a", None);
    assert_eq!(color(&l.at("a", 0)), NAMED);
    l.sheet
        .set_inheritance(&l.editor.engine, l.handle("a"), StateInheritance::None)
        .unwrap();
    assert_eq!(l.state("a").button, None);
    assert_eq!(color(&l.at("a", 0)), Color::BLACK);
    l.sheet
        .set_inheritance(
            &l.editor.engine,
            l.handle("a"),
            StateInheritance::ParentButton,
        )
        .unwrap();
    assert_eq!(color(&l.at("a", 0)), NAMED);
    l.sheet.unbind(l.handle("root"));
    assert_eq!(color(&l.at("a", 0)), Color::BLACK);
}

#[test]
fn button_inherits_fixed_and_router_states_without_mutating_frozen_selection() {
    let mut l = Lab::new();
    let a = l.handle("a");
    let root = l.handle("root");
    l.sheet.bind(&l.editor.engine, a, builtin_styles()).unwrap();
    l.button("root");
    l.editor.selection.select("root".into(), false);
    let selection = l.editor.selection.clone();
    assert_eq!(color(&l.at("a", 0)), SELECTED);
    assert!(!l.editor.selection.ids.contains(&"a".into()));
    l.event(CanvasEvent::PointerDown(pointer("root")));
    assert_eq!(color(&l.at("a", 0)), PRESS);
    assert!(!l.router.state(&l.editor.engine, a).press);
    assert!(l.state("a").visual.press);
    l.event(CanvasEvent::PointerUp(pointer("root")));
    assert_eq!(color(&l.at("a", 0)), FOCUS);
    l.event(CanvasEvent::PointerMove(pointer("b")));
    assert_eq!(color(&l.at("a", 0)), HOVER); // a sibling hit activates the button
    l.sheet.set_disabled(&l.editor.engine, root, true).unwrap();
    assert_eq!(color(&l.at("a", 0)), DISABLED);
    l.sheet.set_disabled(&l.editor.engine, root, false).unwrap();
    l.event(CanvasEvent::FocusLost);
    assert_eq!(color(&l.at("a", 0)), SELECTED);
    assert_eq!(l.editor.selection, selection);
    assert!(!l.editor.undo());
}

#[test]
fn nearest_parent_or_grandparent_button_only_and_nested_button_is_boundary() {
    let mut l = Lab::new();
    l.button("root");
    l.palette("root", &[("busy", StatePaint::fill(NAMED))]);
    l.select("root", Some("busy"));
    assert_eq!(l.state("grandchild").button, Some(l.handle("root")));
    assert_eq!(l.state("great_grandchild").button, None);
    l.button("middle");
    assert_eq!(l.state("middle").button, None);
    assert_eq!(l.state("grandchild").button, Some(l.handle("middle")));
    assert_eq!(l.state("grandchild").named, None); // do not recursively inherit root through middle
    assert_eq!(l.state("great_grandchild").button, Some(l.handle("middle")));
}

#[test]
fn inactive_invalid_palette_placeholder_and_foreign_engine_fail_atomically() {
    let mut l = Lab::new();
    let a = l.handle("a");
    l.palette("a", &[("busy", StatePaint::fill(NAMED))]);
    l.select("a", Some("busy"));
    l.placeholder("valid");
    for states in [
        BTreeMap::from([("unused".into(), opacity(f32::NAN))]),
        BTreeMap::from([(" ".into(), StatePaint::default())]),
        BTreeMap::from([("x".repeat(MAX_STATE_NAME_BYTES + 1), StatePaint::default())]),
    ] {
        assert!(
            l.sheet
                .set_named_states(&l.editor.engine, a, states)
                .is_err()
        );
    }
    let too_many = (0..=MAX_NAMED_STATES_PER_BINDING)
        .map(|i| (format!("s{i}"), StatePaint::default()))
        .collect();
    assert_eq!(
        l.sheet.set_named_states(&l.editor.engine, a, too_many),
        Err(StateStyleError::Capacity)
    );
    assert!(
        l.sheet
            .set_placeholder(
                &l.editor.engine,
                a,
                Some(TextPlaceholder::new("bad", opacity(2.)))
            )
            .is_err()
    );
    assert_eq!(
        l.sheet.set_placeholder(
            &l.editor.engine,
            a,
            Some(TextPlaceholder::new(
                "x".repeat(MAX_PLACEHOLDER_BYTES + 1),
                StatePaint::default()
            ))
        ),
        Err(StateStyleError::Capacity)
    );
    assert!(
        l.sheet
            .set_placeholder(
                &l.editor.engine,
                l.handle("b"),
                Some(TextPlaceholder::default())
            )
            .is_err()
    );
    let other = Lab::new();
    assert_eq!(
        l.sheet.select_state(&other.editor.engine, a, None),
        Err(StateStyleError::WrongEngine)
    );
    assert_eq!(l.state("a").named.as_deref(), Some("busy"));
    assert_eq!(content(&l.at("a", 0)), "valid");
    assert!(l.sheet.unbind(a));
    assert_eq!(
        l.sheet.select_state(&l.editor.engine, a, None),
        Err(StateStyleError::Unbound)
    );
}

#[test]
fn detach_remount_retires_pointer_state_but_preserves_named_placeholder_registrations() {
    let mut l = Lab::new();
    let a = l.handle("a");
    let root = l.handle("root");
    l.sheet.bind(&l.editor.engine, a, builtin_styles()).unwrap();
    l.button("root");
    l.palette("root", &[("busy", StatePaint::default())]);
    l.palette("a", &[("busy", StatePaint::fill(NAMED))]);
    l.select("root", Some("busy"));
    l.placeholder("hint");
    l.event(CanvasEvent::PointerMove(pointer("root")));
    l.event(CanvasEvent::PointerDown(pointer("root")));
    assert_eq!(color(&l.at("a", 0)), PRESS);
    let old_epoch = l.editor.engine.instance(a).unwrap().attachment_epoch;
    let (subtree, _) = l.editor.engine.detach(root).unwrap();
    assert_eq!(l.state("a"), ResolvedVisualState::default());
    l.editor.engine.attach(&subtree, None, None).unwrap();
    assert_ne!(
        l.editor.engine.instance(a).unwrap().attachment_epoch,
        old_epoch
    );
    assert_eq!(l.state("a").visual, VisualState::default());
    assert_eq!(l.state("a").named.as_deref(), Some("busy"));
    assert_eq!(content(&l.at("a", 1)), "hint");
    assert_eq!(color(&l.at("a", 1)), PLACEHOLDER);
    l.editor.engine.destroy(a).unwrap();
    l.editor
        .engine
        .create("text", "a", json!({}), Some(root), None)
        .unwrap();
    assert_ne!(l.handle("a"), a);
    l.sheet.presentation(&l.editor, &l.router).unwrap();
    assert_eq!(
        l.sheet.resolved_state(&l.editor, &l.router, l.handle("a")),
        Err(StateStyleError::Unbound)
    );
    l.editor
        .engine
        .reset_scene(l.editor.scene().clone())
        .unwrap();
    l.sheet.presentation(&l.editor, &l.router).unwrap();
    assert!(l.sheet.is_empty());
}

#[test]
fn named_retarget_and_clear_use_current_sample_and_distinct_exit_duration() {
    let mut l = Lab::new();
    let a = l.handle("a");
    l.palette("a", &[("dim", opacity(0.)), ("bright", opacity(0.8))]);
    l.sheet
        .set_transitions(
            &l.editor.engine,
            a,
            StateTransitions {
                transition: StateTransition::new(100),
                transition_out: Some(StateTransition::new(200)),
            },
        )
        .unwrap();
    l.select("a", Some("dim"));
    assert_eq!(l.at("a", 0).style.opacity, 1.);
    assert_eq!(l.at("a", 50).style.opacity, 0.5);
    l.select("a", Some("bright"));
    assert_eq!(l.at("a", 50).style.opacity, 0.5);
    assert!((l.at("a", 100).style.opacity - 0.65).abs() < 1e-6);
    l.select("a", None);
    assert!((l.at("a", 100).style.opacity - 0.65).abs() < 1e-6);
    assert!((l.at("a", 200).style.opacity - 0.825).abs() < 1e-6);
    assert_eq!(l.at("a", 300).style.opacity, 1.);
    assert!(!l.sheet.has_active_transitions());
    assert!(!l.editor.undo());
}

#[test]
fn placeholder_entry_exit_transition_retains_text_discrete_and_paint_continuous() {
    let mut l = Lab::new();
    let a = l.handle("a");
    l.sheet
        .set_transitions(
            &l.editor.engine,
            a,
            StateTransitions {
                transition: StateTransition::new(100),
                transition_out: Some(StateTransition::new(200)),
            },
        )
        .unwrap();
    l.sheet
        .set_placeholder(
            &l.editor.engine,
            a,
            Some(TextPlaceholder::new("hint", opacity(0.))),
        )
        .unwrap();
    assert_eq!(content(&l.at("a", 0)), "hint");
    assert_eq!(l.at("a", 50).style.opacity, 0.5);
    l.sheet.set_placeholder(&l.editor.engine, a, None).unwrap();
    assert_eq!(content(&l.at("a", 50)), "");
    assert_eq!(l.at("a", 150).style.opacity, 0.75);
    assert_eq!(l.at("a", 250).style.opacity, 1.);
    assert!(!l.editor.undo());
}

#[test]
fn remount_discards_inherited_motion_even_without_a_detached_presentation() {
    let mut l = Lab::new();
    let a = l.handle("a");
    let root = l.handle("root");
    l.button("root");
    l.palette("root", &[("dim", StatePaint::default())]);
    l.palette("a", &[("dim", opacity(0.))]);
    l.sheet
        .set_transitions(
            &l.editor.engine,
            a,
            StateTransitions {
                transition: StateTransition::new(100),
                transition_out: None,
            },
        )
        .unwrap();
    l.select("root", Some("dim"));
    l.at("a", 0);
    assert_eq!(l.at("a", 50).style.opacity, 0.5);
    let (subtree, _) = l.editor.engine.detach(root).unwrap();
    l.editor.engine.attach(&subtree, None, None).unwrap();
    assert_eq!(l.at("a", 60).style.opacity, 1.); // new attachment starts from canonical, never old sample
    assert_eq!(l.at("a", 110).style.opacity, 0.5);
    l.sheet.finish_transitions();
    assert_eq!(l.at("a", 110).style.opacity, 0.);
}

#[test]
fn invalid_named_presentation_keeps_clock_and_other_channels_atomic() {
    let mut l = Lab::new();
    let a = l.handle("a");
    l.palette("a", &[("dim", opacity(0.))]);
    l.palette("b", &[("colored", StatePaint::fill(NAMED))]);
    l.sheet
        .set_transitions(
            &l.editor.engine,
            a,
            StateTransitions {
                transition: StateTransition::new(100),
                transition_out: None,
            },
        )
        .unwrap();
    l.at("a", 0);
    l.select("a", Some("dim"));
    l.select("b", Some("colored"));
    l.editor
        .transaction(|scene| {
            scene.update_object(&"b".into(), |object| {
                object.style.fill = None;
                object.style.fill_layers = vec![Color::WHITE.into(); 32];
            })
        })
        .unwrap();
    let canonical = l.editor.scene().clone();
    assert!(
        l.sheet
            .presentation_at(&l.editor, &l.router, Duration::from_millis(10))
            .is_err()
    );
    assert_eq!(l.sheet.transition_time(), Duration::ZERO);
    assert!(!l.sheet.has_active_transitions());
    assert_eq!(l.editor.scene(), &canonical);
    l.select("b", None);
    assert_eq!(l.at("a", 20).style.opacity, 1.);
    assert_eq!(l.at("a", 70).style.opacity, 0.5);
    assert!(l.editor.undo());
    assert!(!l.editor.undo());
}

#[test]
fn state_operations_preserve_playback_cancel_redo_and_canonical_animation_values() {
    let mut l = Lab::new();
    let a = l.handle("a");
    l.editor
        .set_property(a, PropertyKey::Name, json!("prior"))
        .unwrap();
    assert!(l.editor.undo());
    l.palette("a", &[("busy", StatePaint::fill(NAMED))]);
    l.select("a", Some("busy"));
    l.placeholder("hint");
    l.at("a", 0);
    assert!(l.editor.redo());
    let original = l.editor.scene().clone();
    let token = l.editor.begin_playback().unwrap();
    l.editor
        .apply_playback_properties(token, [(a, PropertyKey::Opacity, json!(0.25))])
        .unwrap();
    assert_eq!(l.at("a", 1).style.opacity, 0.25);
    assert!(l.editor.is_playback_active(token));
    assert!(l.editor.cancel_playback(token));
    assert_eq!(l.editor.scene(), &original);
    assert!(l.editor.undo());
    assert!(!l.editor.undo());
}

#[test]
fn resolving_state_never_consumes_space_or_capture_and_keeps_gesture_selection_frozen() {
    let mut l = Lab::new();
    l.host = ScenePluginHost::with_defaults();
    l.palette("b", &[("busy", opacity(0.4))]);
    l.select("b", Some("busy"));
    l.event(CanvasEvent::PointerDown(pointer("b")));
    let selection = l.editor.selection.clone();
    let mut moved = pointer("b");
    moved.position = Point::new(50., 40.);
    l.event(CanvasEvent::PointerMove(moved));
    assert!(l.host.has_capture());
    assert!(l.editor.editing());
    l.at("b", 0);
    l.select("b", None);
    l.at("b", 1);
    assert!(l.host.has_capture());
    assert_eq!(l.editor.selection, selection);
    l.event(CanvasEvent::PointerCancel);
    assert!(!l.host.has_capture());
    assert!(!l.editor.undo());
    l.event(CanvasEvent::KeyDown {
        key: KeyCode::Space,
        modifiers: Modifiers::default(),
    });
    assert!(l.router.space_held());
    l.at("b", 2);
    assert!(l.router.space_held());
    l.event(CanvasEvent::KeyUp {
        key: KeyCode::Space,
        modifiers: Modifiers::default(),
    });
    assert!(!l.router.space_held());
}
