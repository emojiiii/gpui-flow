use gpui_flow_editor::SceneEditor;
use gpui_flow_engine::{ElementHandle, PropertyKey};
use gpui_flow_interaction::{scene::*, *};
use gpui_flow_scene::{
    Color, DisplayObject, EventKind, ListenerOptions, Paint, Point, Scene, Style, display,
};
use serde_json::json;
use std::{borrow::Cow, cell::Cell, rc::Rc};
const BASE: Color = Color::hex(0x111111);
const SELECTED: Color = Color::hex(0x222222);
const FOCUS: Color = Color::hex(0x333333);
const HOVER: Color = Color::hex(0x444444);
const PRESS: Color = Color::hex(0x555555);
const DISABLED: Color = Color::hex(0x666666);
fn styles() -> StateStyles {
    StateStyles {
        selected: StatePaint::fill(SELECTED),
        focus: StatePaint::fill(FOCUS),
        hover: StatePaint::fill(HOVER),
        press: StatePaint::fill(PRESS),
        disabled: StatePaint::fill(DISABLED),
    }
}
fn setup() -> (
    SceneEditor,
    ScenePluginHost,
    SceneEventRouter,
    SceneStateStyles,
    ElementHandle,
) {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("root", display::Frame::new(300., 200.)),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("a", display::Rect::new(100., 80.)).with_style(Style::fill(BASE)),
            Some("root".into()),
            None,
        )
        .unwrap();
    let editor = SceneEditor::new(scene).unwrap();
    let a = editor.engine.handle(&"a".into()).unwrap();
    let mut sheet = SceneStateStyles::new(&editor.engine);
    sheet.bind(&editor.engine, a, styles()).unwrap();
    (
        editor,
        ScenePluginHost::new(),
        SceneEventRouter::new(),
        sheet,
        a,
    )
}
fn pointer(hit: HitTarget) -> PointerInput {
    PointerInput::left(Point::new(20., 20.), hit)
}
fn on_a() -> PointerInput {
    pointer(HitTarget::Object("a".into()))
}
fn color(scene: &Scene) -> Color {
    let Some(Paint::Solid { color }) = scene.get(&"a".into()).unwrap().object.style.fill else {
        panic!("solid fill")
    };
    color
}
fn shown(sheet: &mut SceneStateStyles, editor: &SceneEditor, router: &SceneEventRouter) -> Color {
    color(&sheet.presentation(editor, router).unwrap())
}
#[test]
fn priority_selected_focus_hover_press_and_disabled_suppression() {
    let s = styles();
    let base = Style::fill(BASE);
    for (state, expected) in [
        (VisualState::default(), BASE),
        (
            VisualState {
                selected: true,
                ..Default::default()
            },
            SELECTED,
        ),
        (
            VisualState {
                selected: true,
                focus: true,
                ..Default::default()
            },
            FOCUS,
        ),
        (
            VisualState {
                selected: true,
                focus: true,
                hover: true,
                ..Default::default()
            },
            HOVER,
        ),
        (
            VisualState {
                selected: true,
                focus: true,
                hover: true,
                press: true,
                disabled: false,
            },
            PRESS,
        ),
        (
            VisualState {
                selected: true,
                focus: true,
                hover: true,
                press: true,
                disabled: true,
            },
            DISABLED,
        ),
    ] {
        assert_eq!(s.resolve(&base, state).fill, Some(expected.into()));
    }
    let mut empty_disabled = s;
    empty_disabled.disabled = StatePaint::default();
    assert_eq!(
        empty_disabled
            .resolve(
                &base,
                VisualState {
                    selected: true,
                    focus: true,
                    hover: true,
                    press: true,
                    disabled: true
                }
            )
            .fill,
        Some(SELECTED.into())
    );
}
#[test]
fn patches_merge_fields_and_keep_other_paints_unchanged() {
    let mut set = styles();
    set.hover.opacity = Some(0.4);
    set.press = StatePaint::default();
    let mut base = Style::fill(BASE);
    base.fill_layers.push(Color::WHITE.into());
    let resolved = set.resolve(
        &base,
        VisualState {
            hover: true,
            press: true,
            ..Default::default()
        },
    );
    assert_eq!(resolved.fill, Some(HOVER.into()));
    assert_eq!(resolved.opacity, 0.4);
    assert_eq!(resolved.fill_layers, base.fill_layers);
    assert_eq!(resolved.stroke, base.stroke);
}
#[test]
fn pointer_states_follow_routes_and_cancel_even_when_release_is_prevented() {
    let (mut e, mut h, mut r, mut sheet, a) = setup();
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerMove(on_a()));
    assert_eq!(shown(&mut sheet, &e, &r), HOVER);
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerDown(on_a()));
    assert_eq!(shown(&mut sheet, &e, &r), PRESS);
    e.engine
        .on(
            a,
            EventKind::PointerUp,
            ListenerOptions::default(),
            |event| event.prevent_default(),
        )
        .unwrap();
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerUp(on_a()));
    assert!(!r.state(&e.engine, a).press);
    assert_eq!(shown(&mut sheet, &e, &r), HOVER);
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerCancel);
    assert_eq!(shown(&mut sheet, &e, &r), FOCUS);
    r.dispatch(&mut e, &mut h, CanvasEvent::FocusLost);
    assert_eq!(shown(&mut sheet, &e, &r), BASE);
}
#[test]
fn prevented_down_cannot_arm_press_or_focus() {
    let (mut e, mut h, mut r, mut sheet, a) = setup();
    e.engine
        .on(
            a,
            EventKind::PointerDown,
            ListenerOptions::default(),
            |event| event.prevent_default(),
        )
        .unwrap();
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerDown(on_a()));
    assert_eq!(r.state(&e.engine, a), SceneInteractionState::default());
    assert_eq!(shown(&mut sheet, &e, &r), BASE);
}
#[test]
fn ancestors_share_hover_and_press_but_not_focus() {
    let (mut e, mut h, mut r, _, a) = setup();
    let root = e.engine.handle(&"root".into()).unwrap();
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerMove(on_a()));
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerDown(on_a()));
    assert_eq!(
        r.state(&e.engine, root),
        SceneInteractionState {
            hover: true,
            press: true,
            focus: false
        }
    );
    assert_eq!(
        r.state(&e.engine, a),
        SceneInteractionState {
            hover: true,
            press: true,
            focus: true
        }
    );
    r.dispatch(
        &mut e,
        &mut h,
        CanvasEvent::PointerMove(pointer(HitTarget::Background)),
    );
    assert!(!r.state(&e.engine, root).hover);
    assert!(r.state(&e.engine, root).press);
}
#[test]
fn inherited_presentation_opacity_uses_real_paint_list_without_editing_scene() {
    let (mut e, mut h, mut r, mut sheet, _) = setup();
    let root = e.engine.handle(&"root".into()).unwrap();
    sheet
        .bind(
            &e.engine,
            root,
            StateStyles {
                hover: StatePaint {
                    opacity: Some(0.5),
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .unwrap();
    let before = e.scene().clone();
    let revision = e.engine.revision();
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerMove(on_a()));
    let presentation = sheet.presentation(&e, &r).unwrap();
    assert!(matches!(presentation, Cow::Owned(_)));
    assert_eq!(
        presentation
            .paint_list()
            .unwrap()
            .iter()
            .find(|p| p.object.id.as_str() == "a")
            .unwrap()
            .opacity,
        0.5
    );
    assert_eq!(e.scene(), &before);
    assert_eq!(e.engine.revision(), revision);
}
#[test]
fn no_active_patch_borrows_scene_and_noop_binding_installs_no_listeners() {
    let (mut e, _, r, mut sheet, a) = setup();
    let count = e.engine.listener_count();
    assert!(matches!(
        sheet.presentation(&e, &r).unwrap(),
        Cow::Borrowed(_)
    ));
    sheet.bind(&e.engine, a, StateStyles::default()).unwrap();
    assert!(matches!(
        sheet.presentation(&e, &r).unwrap(),
        Cow::Borrowed(_)
    ));
    assert_eq!(e.engine.listener_count(), count);
    assert!(!e.undo());
}
#[test]
fn baseline_edit_during_hover_is_revealed_without_restoring_stale_style_or_history() {
    let (mut e, mut h, mut r, mut sheet, a) = setup();
    e.set_property(a, PropertyKey::Name, json!("prior"))
        .unwrap();
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerMove(on_a()));
    let notifications = Rc::new(Cell::new(0));
    let n = notifications.clone();
    e.engine
        .on_change(None, move |_| n.set(n.get() + 1))
        .unwrap();
    let changed = Color::hex(0xff0000);
    e.transaction(|scene| {
        scene.update_object(&"a".into(), |o| o.style.fill = Some(changed.into()))
    })
    .unwrap();
    let revision = e.engine.revision();
    assert_eq!(shown(&mut sheet, &e, &r), HOVER);
    r.dispatch(
        &mut e,
        &mut h,
        CanvasEvent::PointerMove(pointer(HitTarget::Background)),
    );
    assert_eq!(shown(&mut sheet, &e, &r), changed);
    assert_eq!(notifications.get(), 1);
    assert_eq!(e.engine.revision(), revision);
    assert!(e.undo());
    assert_eq!(color(e.scene()), BASE);
    assert!(e.undo());
    assert_eq!(e.scene().get(&"a".into()).unwrap().object.name, "");
    assert!(!e.undo());
}
#[test]
fn presentation_does_not_interrupt_playback_or_change_its_cancel_baseline() {
    let (mut e, mut h, mut r, mut sheet, a) = setup();
    e.set_property(a, PropertyKey::Name, json!("prior"))
        .unwrap();
    let before = e.scene().clone();
    let token = e.begin_playback().unwrap();
    e.apply_playback_properties(token, [(a, PropertyKey::Opacity, json!(0.25))])
        .unwrap();
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerMove(on_a()));
    assert_eq!(shown(&mut sheet, &e, &r), HOVER);
    assert!(e.is_playback_active(token));
    assert_eq!(
        sheet
            .presentation(&e, &r)
            .unwrap()
            .get(&"a".into())
            .unwrap()
            .object
            .style
            .opacity,
        0.25
    );
    assert!(e.cancel_playback(token));
    assert_eq!(e.scene(), &before);
    assert!(e.undo());
    assert!(!e.undo());
}
#[test]
fn disabled_is_visual_and_preserves_selected_lower_layer() {
    let (mut e, mut h, mut r, mut sheet, a) = setup();
    e.selection.select("a".into(), false);
    sheet
        .bind(
            &e.engine,
            a,
            StateStyles {
                disabled: StatePaint {
                    opacity: Some(0.3),
                    ..Default::default()
                },
                ..styles()
            },
        )
        .unwrap();
    sheet.set_disabled(&e.engine, a, true).unwrap();
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerDown(on_a()));
    assert!(r.state(&e.engine, a).press);
    let scene = sheet.presentation(&e, &r).unwrap();
    assert_eq!(color(&scene), SELECTED);
    assert_eq!(scene.get(&"a".into()).unwrap().object.style.opacity, 0.3);
    sheet.set_disabled(&e.engine, a, false).unwrap();
    assert_eq!(shown(&mut sheet, &e, &r), PRESS);
}
#[test]
fn bad_replacement_is_atomic_and_checks_inactive_styles() {
    let (mut e, mut h, mut r, mut sheet, a) = setup();
    let mut invalid = styles();
    invalid.disabled.opacity = Some(f32::NAN);
    assert!(sheet.bind(&e.engine, a, invalid).is_err());
    assert_eq!(sheet.len(), 1);
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerMove(on_a()));
    assert_eq!(shown(&mut sheet, &e, &r), HOVER);
    let mut invalid = styles();
    invalid.press.opacity = Some(2.);
    assert!(sheet.bind(&e.engine, a, invalid).is_err());
}
#[test]
fn destroying_and_recreating_same_public_id_does_not_inherit_state_registration() {
    let (mut e, mut h, mut r, mut sheet, a) = setup();
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerDown(on_a()));
    e.engine.destroy(a).unwrap();
    e.engine.create("rect", "a", json!({}), None, None).unwrap();
    assert_ne!(e.engine.handle(&"a".into()).unwrap(), a);
    assert_eq!(r.state(&e.engine, a), SceneInteractionState::default());
    sheet.presentation(&e, &r).unwrap();
    assert!(sheet.is_empty());
}
#[test]
fn detach_reattach_between_events_keeps_styles_but_never_revives_press_hover_or_focus() {
    let (mut e, mut h, mut r, mut sheet, a) = setup();
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerMove(on_a()));
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerDown(on_a()));
    let old_epoch = e.engine.instance(a).unwrap().attachment_epoch;
    let (subtree, _) = e.engine.detach(a).unwrap();
    sheet.presentation(&e, &r).unwrap();
    assert_eq!(sheet.len(), 1);
    e.engine.attach(&subtree, None, None).unwrap();
    assert_eq!(e.engine.handle(&"a".into()).unwrap(), a);
    assert_ne!(e.engine.instance(a).unwrap().attachment_epoch, old_epoch);
    assert_eq!(r.state(&e.engine, a), SceneInteractionState::default());
    assert_eq!(shown(&mut sheet, &e, &r), BASE);
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerUp(on_a()));
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerMove(on_a()));
    assert_eq!(shown(&mut sheet, &e, &r), HOVER);
}
#[test]
fn document_reset_prunes_even_equal_ids_and_foreign_engine_cannot_change_sheet() {
    let (mut e, _, r, mut sheet, a) = setup();
    let (other, _, _, _, _) = setup();
    assert_eq!(
        sheet.bind(&other.engine, a, styles()).unwrap_err(),
        StateStyleError::WrongEngine
    );
    assert_eq!(sheet.len(), 1);
    e.engine.reset_scene(e.scene().clone()).unwrap();
    sheet.presentation(&e, &r).unwrap();
    assert!(sheet.is_empty());
}
#[test]
fn unbind_and_clear_remove_overlays_without_writing_document() {
    let (mut e, mut h, mut r, mut sheet, a) = setup();
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerMove(on_a()));
    assert_eq!(shown(&mut sheet, &e, &r), HOVER);
    assert!(sheet.unbind(a));
    assert!(!sheet.unbind(a));
    assert_eq!(shown(&mut sheet, &e, &r), BASE);
    sheet.bind(&e.engine, a, styles()).unwrap();
    sheet.clear();
    assert!(sheet.is_empty());
    assert!(!e.undo());
}

#[test]
fn canvas_leave_emits_path_once_clears_hover_and_retains_press_until_cancel() {
    let (mut e, mut h, mut r, mut sheet, a) = setup();
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerMove(on_a()));
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerDown(on_a()));
    let out = r.dispatch(&mut e, &mut h, CanvasEvent::PointerLeave);
    assert_eq!(
        out.events
            .iter()
            .filter(|e| e.kind == EventKind::PointerOut)
            .count(),
        1
    );
    assert_eq!(
        out.events
            .iter()
            .filter(|e| e.kind == EventKind::PointerLeave)
            .count(),
        2
    );
    assert!(!r.state(&e.engine, a).hover);
    assert!(r.state(&e.engine, a).press);
    assert_eq!(shown(&mut sheet, &e, &r), PRESS);
    assert!(
        r.dispatch(&mut e, &mut h, CanvasEvent::PointerLeave)
            .events
            .is_empty()
    );
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerCancel);
    assert!(!r.state(&e.engine, a).press);
    assert_eq!(shown(&mut sheet, &e, &r), FOCUS);
}
#[test]
fn transparent_overlay_keeps_canonical_hit_geometry_and_undo_redo_unchanged() {
    let (mut e, mut h, mut r, mut sheet, a) = setup();
    sheet
        .bind(
            &e.engine,
            a,
            StateStyles {
                hover: StatePaint {
                    opacity: Some(0.),
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .unwrap();
    e.set_property(a, PropertyKey::Name, json!("redo")).unwrap();
    assert!(e.undo());
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerMove(on_a()));
    assert!(
        !sheet
            .presentation(&e, &r)
            .unwrap()
            .paint_list()
            .unwrap()
            .iter()
            .any(|p| p.object.id.as_str() == "a")
    );
    assert_eq!(
        e.engine
            .hit_test(Point::new(20., 20.), Default::default())
            .unwrap()
            .unwrap()
            .id
            .as_str(),
        "a"
    );
    assert!(e.redo());
    assert_eq!(e.scene().get(&"a".into()).unwrap().object.name, "redo");
}

#[test]
fn nonpainting_group_rejects_fill_states_but_accepts_opacity() {
    let (mut e, _, _, mut sheet, _) = setup();
    e.engine
        .transaction(|s| {
            s.insert(
                DisplayObject::new("group", display::Group::new()),
                None,
                None,
            )
        })
        .unwrap();
    let group = e.engine.handle(&"group".into()).unwrap();
    assert!(sheet.bind(&e.engine, group, styles()).is_err());
    sheet
        .bind(
            &e.engine,
            group,
            StateStyles {
                hover: StatePaint {
                    opacity: Some(0.5),
                    ..Default::default()
                },
                ..Default::default()
            },
        )
        .unwrap();
}

#[test]
fn leave_during_drag_retains_capture_reentry_moves_and_external_up_cancel_rolls_back() {
    let (mut e, _, mut r, _, a) = setup();
    let mut h = ScenePluginHost::with_defaults();
    let before = e.scene().clone();
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerDown(on_a()));
    let mut p = on_a();
    p.position.x = 70.;
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerMove(p.clone()));
    assert!(h.has_capture());
    assert!(e.editing());
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerLeave);
    assert!(h.has_capture());
    assert!(r.state(&e.engine, a).press);
    assert!(!r.state(&e.engine, a).hover);
    p.position.x = 90.;
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerMove(p));
    assert!(r.state(&e.engine, a).hover);
    assert_ne!(e.scene(), &before);
    // SceneView's on_mouse_up_out sends the same cancellation.
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerCancel);
    assert!(!h.has_capture());
    assert!(!e.editing());
    assert!(!r.state(&e.engine, a).press);
    assert_eq!(e.scene(), &before);
    assert!(!e.undo());
}
#[test]
fn new_canonical_paint_limit_can_reject_overlay_without_mutating_document() {
    let (mut e, mut h, mut r, mut sheet, _) = setup();
    r.dispatch(&mut e, &mut h, CanvasEvent::PointerMove(on_a()));
    e.transaction(|s| {
        s.update_object(&"a".into(), |o| {
            o.style.fill = None;
            o.style.fill_layers = vec![Color::WHITE.into(); 32];
        })
    })
    .unwrap();
    let canonical = e.scene().clone();
    let revision = e.engine.revision();
    assert!(sheet.presentation(&e, &r).is_err());
    assert_eq!(e.scene(), &canonical);
    assert_eq!(e.engine.revision(), revision);
    assert!(e.undo());
    assert_eq!(shown(&mut sheet, &e, &r), HOVER);
}
