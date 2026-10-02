//! Real independent engines/plugin hosts/routers behind the production dispatcher.
use super::*;
use ::core::prelude::v1::test;
use gpui_flow_editor::SceneEditor;
use gpui_flow_interaction::{
    KeyCode, Modifiers, PointerInput,
    scene::{SceneEventRouter, ScenePluginHost},
};
use gpui_flow_scene::{
    Affine, Color, DisplayObject, EventKind, HitTestOptions, SceneEvent, Style, display,
};
struct Model {
    editor: SceneEditor,
    host: ScenePluginHost,
    router: SceneEventRouter,
    events: Vec<SceneEvent>,
}
impl Model {
    fn new(x: f32) -> Self {
        let mut scene = Scene::new();
        scene
            .insert(
                DisplayObject::new("same-id", display::Rect::new(80., 80.))
                    .with_transform(Affine::translation(x, 10.))
                    .with_style(Style::fill(Color::BLACK)),
                None,
                None,
            )
            .unwrap();
        Self {
            editor: SceneEditor::new(scene).unwrap(),
            host: ScenePluginHost::with_defaults(),
            router: SceneEventRouter::new(),
            events: vec![],
        }
    }
    fn handle(&self) -> ElementHandle {
        self.editor.engine.handle(&"same-id".into()).unwrap()
    }
    fn x(&self) -> f32 {
        self.editor
            .scene()
            .get(&"same-id".into())
            .unwrap()
            .object
            .transform
            .tx
    }
}
struct Access {
    layers: BTreeMap<LayerId, Model>,
    sent: Vec<(LayerId, CanvasEvent)>,
}
impl LayerAccess for Access {
    fn hit(&mut self, id: LayerId, p: ScenePoint) -> HitTarget {
        let layer = &self.layers[&id];
        layer
            .editor
            .engine
            .hit_test(
                layer.editor.viewport.screen_to_world(p),
                HitTestOptions {
                    include_locked: true,
                    ..Default::default()
                },
            )
            .unwrap()
            .map_or(HitTarget::Background, |hit| {
                HitTarget::Object(hit.id.to_string())
            })
    }
    fn send(&mut self, id: LayerId, e: CanvasEvent) {
        self.sent.push((id, e.clone()));
        if let Some(layer) = self.layers.get_mut(&id) {
            let report = layer.router.dispatch(&mut layer.editor, &mut layer.host, e);
            assert!(report.errors.is_empty(), "{:?}", report.errors);
            layer.events.extend(report.events);
        }
    }
}
fn id(serial: u64) -> LayerId {
    LayerId { app: 1, serial }
}
fn lab() -> (LayerRouting, Access) {
    let mut route = LayerRouting::default();
    route.entries = vec![
        (
            id(1),
            LayerOptions {
                hit_background: true,
                ..Default::default()
            },
        ),
        (id(2), LayerOptions::default()),
    ];
    (
        route,
        Access {
            layers: [(id(1), Model::new(10.)), (id(2), Model::new(50.))].into(),
            sent: vec![],
        },
    )
}
fn pointer(x: f32) -> PointerInput {
    PointerInput::left(
        ScenePoint::new(x, 40.),
        HitTarget::Object("spoofed-other-layer-id".into()),
    )
}
fn down(x: f32) -> CanvasEvent {
    CanvasEvent::PointerDown(pointer(x))
}
fn moved(x: f32) -> CanvasEvent {
    CanvasEvent::PointerMove(pointer(x))
}
fn up(x: f32) -> CanvasEvent {
    CanvasEvent::PointerUp(pointer(x))
}
fn key(k: KeyCode) -> CanvasEvent {
    CanvasEvent::KeyDown {
        key: k,
        modifiers: Modifiers::default(),
    }
}
#[test]
fn topmost_overlap_wins_and_identical_document_ids_stay_independent() {
    let (mut r, mut a) = lab();
    assert_eq!(r.route(down(70.), &mut a), Some(id(2)));
    assert!(a.layers[&id(1)].events.is_empty());
    assert!(a.layers[&id(1)].editor.selection.ids.is_empty());
    assert!(
        a.layers[&id(2)]
            .editor
            .selection
            .ids
            .contains(&"same-id".into())
    );
    assert_ne!(a.layers[&id(1)].handle(), a.layers[&id(2)].handle());
    assert_eq!(
        a.layers[&id(2)]
            .events
            .iter()
            .find(|e| e.kind == EventKind::PointerDown)
            .unwrap()
            .target
            .as_str(),
        "same-id"
    );
}
#[test]
fn background_passes_through_and_only_explicit_ground_catches_empty_space() {
    let (mut r, mut a) = lab();
    assert_eq!(r.route(down(25.), &mut a), Some(id(1)));
    r.route(up(25.), &mut a);
    assert_eq!(r.route(down(250.), &mut a), Some(id(1)));
    r.route(up(250.), &mut a);
    r.entries[0].1.hit_background = false;
    assert_eq!(r.route(down(250.), &mut a), None);
    r.entries[1].1.hit_background = true;
    assert_eq!(r.route(down(25.), &mut a), Some(id(2)));
}
#[test]
fn per_layer_viewport_world_and_object_local_coordinates_are_preserved() {
    let (mut r, mut a) = lab();
    a.layers.get_mut(&id(2)).unwrap().editor.viewport = Viewport {
        pan: ScenePoint::new(100., 0.),
        zoom: 2.,
        ..Default::default()
    };
    let local = std::rc::Rc::new(std::cell::Cell::new(None));
    let observed = local.clone();
    let layer = a.layers.get_mut(&id(2)).unwrap();
    layer
        .editor
        .engine
        .on(
            layer.handle(),
            EventKind::PointerDown,
            Default::default(),
            move |event| observed.set(event.local_position),
        )
        .unwrap();
    assert_eq!(r.route(down(220.), &mut a), Some(id(2)));
    let e = a.layers[&id(2)]
        .events
        .iter()
        .find(|e| e.kind == EventKind::PointerDown)
        .unwrap();
    assert_eq!(e.world_position, Some(ScenePoint::new(60., 20.)));
    assert_eq!(local.get(), Some(ScenePoint::new(10., 10.)));
}
#[test]
fn cross_layer_world_mapping_roundtrips_and_rejects_invalid_or_overflow() {
    let a = Viewport {
        pan: ScenePoint::new(100., -20.),
        zoom: 2.,
        ..Default::default()
    };
    let b = Viewport {
        pan: ScenePoint::new(-40., 60.),
        zoom: 0.5,
        ..Default::default()
    };
    let p = ScenePoint::new(12., 8.);
    let screen = map_coordinate(a, p, false).unwrap();
    let mapped = map_coordinate(b, screen, true).unwrap();
    assert_eq!(mapped, ScenePoint::new(328., -128.));
    assert_eq!(
        map_coordinate(a, map_coordinate(b, mapped, false).unwrap(), true).unwrap(),
        p
    );
    assert!(map_coordinate(a, ScenePoint::new(f32::MAX, 0.), false).is_err());
    assert!(map_coordinate(a, ScenePoint::new(f32::NAN, 0.), true).is_err());
    assert!(map_coordinate(Viewport { zoom: 0., ..a }, p, true).is_err());
}
#[test]
fn captured_drag_crossing_other_layers_stays_owned_and_undo_is_independent() {
    let (mut r, mut a) = lab();
    r.route(down(70.), &mut a);
    assert!(a.layers[&id(2)].host.has_capture());
    r.route(moved(20.), &mut a);
    assert_eq!(r.pressed_layer(), Some(id(2)));
    assert_eq!(a.layers[&id(1)].x(), 10.);
    assert_ne!(a.layers[&id(2)].x(), 50.);
    r.route(up(20.), &mut a);
    assert!(!a.layers[&id(2)].host.has_capture());
    assert!(a.layers.get_mut(&id(2)).unwrap().editor.undo());
    assert_eq!(a.layers[&id(2)].x(), 50.);
    assert!(!a.layers.get_mut(&id(1)).unwrap().editor.undo());
}
#[test]
fn focus_and_held_keys_are_isolated_when_changing_layers() {
    let (mut r, mut a) = lab();
    r.route(down(70.), &mut a);
    r.route(up(70.), &mut a);
    r.route(key(KeyCode::Space), &mut a);
    assert!(a.layers[&id(2)].router.space_held());
    r.route(down(25.), &mut a);
    r.route(up(25.), &mut a);
    assert_eq!(r.focused, Some(id(1)));
    assert!(!a.layers[&id(2)].router.space_held());
    assert!(
        a.layers[&id(2)]
            .router
            .focused(&a.layers[&id(2)].editor.engine)
            .is_none()
    );
    a.sent.clear();
    r.route(key(KeyCode::Character('a')), &mut a);
    assert_eq!(a.sent.len(), 1);
    assert_eq!(a.sent[0].0, id(1));
}
#[test]
fn hiding_capture_cancels_rolls_back_and_swallows_old_release() {
    let (mut r, mut a) = lab();
    r.route(down(70.), &mut a);
    r.route(moved(20.), &mut a);
    r.invalidate(id(2), &mut a);
    r.entries[1].1.visible = false;
    assert_eq!(a.layers[&id(2)].x(), 50.);
    assert!(r.focused.is_none());
    a.sent.clear();
    assert_eq!(r.route(moved(25.), &mut a), None);
    assert_eq!(r.route(up(25.), &mut a), None);
    assert!(a.sent.is_empty());
    assert_eq!(r.route(down(25.), &mut a), Some(id(1)));
}
#[test]
fn reorder_cancels_capture_and_next_press_uses_new_topmost_order() {
    let (mut r, mut a) = lab();
    r.route(down(70.), &mut a);
    r.route(moved(90.), &mut a);
    r.reorder(&mut a);
    r.entries.swap(0, 1);
    assert_eq!(a.layers[&id(2)].x(), 50.);
    assert_eq!(r.route(up(70.), &mut a), None);
    assert_eq!(r.route(down(70.), &mut a), Some(id(1)));
}
#[test]
fn removed_layer_id_and_stale_release_never_address_a_replacement() {
    let (mut r, mut a) = lab();
    r.route(down(70.), &mut a);
    r.invalidate(id(2), &mut a);
    r.entries.pop();
    let old = a.layers.remove(&id(2)).unwrap();
    a.layers.insert(id(3), old);
    r.entries.push((id(3), LayerOptions::default()));
    a.sent.clear();
    r.route(up(70.), &mut a);
    assert!(a.sent.is_empty());
    assert_eq!(r.route(down(70.), &mut a), Some(id(3)));
}
#[test]
fn detached_or_recreated_object_cannot_resume_a_layer_press() {
    let (mut r, mut a) = lab();
    r.route(down(70.), &mut a);
    let l = a.layers.get_mut(&id(2)).unwrap();
    let handle = l.handle();
    l.editor.engine.destroy(handle).unwrap();
    r.route(moved(90.), &mut a);
    r.route(up(90.), &mut a);
    assert!(!a.layers[&id(2)].host.has_capture());
    assert!(
        a.layers[&id(2)]
            .events
            .iter()
            .all(|e| e.kind != EventKind::Click)
    );
    assert!(a.layers[&id(1)].events.is_empty());
}
#[test]
fn window_focus_loss_cancels_every_layer_and_suppresses_unpaired_release() {
    let (mut r, mut a) = lab();
    r.route(down(70.), &mut a);
    r.route(moved(95.), &mut a);
    r.route(CanvasEvent::FocusLost, &mut a);
    assert_eq!(a.layers[&id(2)].x(), 50.);
    assert!(r.focused.is_none());
    assert!(r.pressed_layer().is_none());
    assert_eq!(r.route(up(70.), &mut a), None);
    assert_eq!(r.route(key(KeyCode::Delete), &mut a), None);
}
#[test]
fn pointer_leave_keeps_capture_but_clears_hover_on_the_original_layer() {
    let (mut r, mut a) = lab();
    r.route(down(70.), &mut a);
    r.route(CanvasEvent::PointerLeave, &mut a);
    assert_eq!(r.pressed_layer(), Some(id(2)));
    assert!(r.hovered.is_none());
    r.route(moved(300.), &mut a);
    r.route(up(300.), &mut a);
    assert!(a.layers[&id(1)].events.is_empty());
    assert!(!a.layers[&id(2)].host.has_capture());
}
#[test]
fn wrong_pointer_or_button_cannot_end_or_steal_the_stream() {
    let (mut r, mut a) = lab();
    r.route(down(70.), &mut a);
    let mut wrong = pointer(20.);
    wrong.pointer_id = 99;
    assert_eq!(
        r.route(CanvasEvent::PointerDown(wrong.clone()), &mut a),
        None
    );
    assert_eq!(r.route(CanvasEvent::PointerUp(wrong), &mut a), None);
    let mut right_click = pointer(20.);
    right_click.button = Some(gpui_flow_interaction::PointerButton::Right);
    assert_eq!(r.route(CanvasEvent::PointerUp(right_click), &mut a), None);
    assert_eq!(r.pressed_layer(), Some(id(2)));
    r.route(up(70.), &mut a);
    assert!(r.pressed_layer().is_none());
}
#[test]
fn missing_physical_release_cancels_before_new_hover_without_click() {
    let (mut r, mut a) = lab();
    r.route(down(70.), &mut a);
    r.route(moved(90.), &mut a);
    let mut p = pointer(25.);
    p.button = None;
    assert_eq!(r.route(CanvasEvent::PointerMove(p), &mut a), Some(id(1)));
    assert_eq!(a.layers[&id(2)].x(), 50.);
    assert!(r.pressed_layer().is_none());
    assert!(
        a.layers[&id(2)]
            .events
            .iter()
            .all(|e| e.kind != EventKind::Click)
    );
}
#[test]
fn disabled_and_hidden_layers_pass_through_without_changing_scene_visibility() {
    let (mut r, mut a) = lab();
    let original = a.layers[&id(2)].editor.scene().clone();
    for options in [
        LayerOptions {
            interactive: false,
            ..Default::default()
        },
        LayerOptions {
            visible: false,
            ..Default::default()
        },
    ] {
        r.entries[1].1 = options;
        assert_eq!(r.route(down(70.), &mut a), Some(id(1)));
        r.route(up(70.), &mut a);
    }
    assert_eq!(a.layers[&id(2)].editor.scene(), &original);
}
#[test]
fn prevented_down_and_up_do_not_leak_into_underlying_layer() {
    let (mut r, mut a) = lab();
    let l = a.layers.get_mut(&id(2)).unwrap();
    for event in [EventKind::PointerDown, EventKind::PointerUp] {
        l.editor
            .engine
            .on(l.handle(), event, Default::default(), |e| {
                e.prevent_default()
            })
            .unwrap();
    }
    r.route(down(70.), &mut a);
    r.route(moved(20.), &mut a);
    r.route(up(20.), &mut a);
    assert!(a.layers[&id(1)].events.is_empty());
    assert_eq!(a.layers[&id(2)].x(), 50.);
    assert!(!a.layers[&id(2)].host.has_capture());
    assert!(r.pressed_layer().is_none());
}
#[test]
fn wheel_routes_to_one_layer_and_invalid_input_changes_nothing() {
    let (mut r, mut a) = lab();
    let event = CanvasEvent::Wheel {
        position: ScenePoint::new(70., 40.),
        delta: ScenePoint::new(0., 10.),
        modifiers: Modifiers::default(),
    };
    assert_eq!(r.route(event, &mut a), Some(id(2)));
    assert_eq!(a.layers[&id(1)].editor.viewport, Viewport::default());
    a.sent.clear();
    assert_eq!(r.route(down(f32::NAN), &mut a), None);
    assert!(a.sent.is_empty());
}
#[test]
fn destroying_one_engine_runs_only_its_disposers_and_preserves_the_other() {
    use std::{cell::Cell, rc::Rc};
    let (mut r, mut a) = lab();
    let counts = Rc::new([Cell::new(0), Cell::new(0)]);
    for serial in [1, 2] {
        let l = a.layers.get_mut(&id(serial)).unwrap();
        let counts = counts.clone();
        l.editor
            .engine
            .add_disposer(l.handle(), move || {
                counts[(serial - 1) as usize].set(counts[(serial - 1) as usize].get() + 1)
            })
            .unwrap();
    }
    r.invalidate(id(2), &mut a);
    r.entries.pop();
    drop(a.layers.remove(&id(2)));
    assert_eq!((counts[0].get(), counts[1].get()), (0, 1));
    assert_eq!(r.route(down(25.), &mut a), Some(id(1)));
    drop(a);
    assert_eq!((counts[0].get(), counts[1].get()), (1, 1));
}

#[test]
fn same_handle_detach_reattach_epoch_ends_the_old_press() {
    let (mut r, mut a) = lab();
    r.route(down(70.), &mut a);
    let l = a.layers.get_mut(&id(2)).unwrap();
    let handle = l.handle();
    let old_epoch = l.editor.engine.instance(handle).unwrap().attachment_epoch;
    let (detached, _) = l.editor.engine.detach(handle).unwrap();
    l.editor.engine.attach(&detached, None, None).unwrap();
    assert_eq!(handle, l.handle());
    assert_ne!(
        old_epoch,
        l.editor.engine.instance(handle).unwrap().attachment_epoch
    );
    r.route(moved(90.), &mut a);
    r.route(up(90.), &mut a);
    assert!(!a.layers[&id(2)].host.has_capture());
    assert_eq!(a.layers[&id(2)].x(), 50.);
    assert!(
        a.layers[&id(2)]
            .events
            .iter()
            .all(|e| e.kind != EventKind::Click)
    );
    assert!(a.layers[&id(1)].events.is_empty());
    assert_eq!(r.route(down(70.), &mut a), Some(id(2)));
}

#[test]
fn native_outside_release_respects_button_and_does_not_clear_unrelated_keys() {
    use gpui_flow_interaction::PointerButton;
    let (mut r, mut a) = lab();
    r.route(down(70.), &mut a);
    r.route(key(KeyCode::Space), &mut a);
    a.sent.clear();
    assert_eq!(
        r.release_outside(0, Some(PointerButton::Right), &mut a),
        None
    );
    assert!(a.sent.is_empty());
    assert!(a.layers[&id(2)].router.space_held());
    assert_eq!(r.pressed_layer(), Some(id(2)));
    assert_eq!(
        r.release_outside(0, Some(PointerButton::Left), &mut a),
        Some(id(2))
    );
    assert!(r.pressed_layer().is_none());
    r.route(key(KeyCode::Space), &mut a);
    a.sent.clear();
    r.release_outside(0, Some(PointerButton::Left), &mut a);
    assert!(a.sent.is_empty());
    assert!(a.layers[&id(2)].router.space_held());
}
#[test]
fn native_outside_move_continues_owner_without_hovering_clipped_content() {
    let (mut r, mut a) = lab();
    r.route(down(70.), &mut a);
    assert_eq!(r.move_outside(pointer(350.), &mut a), Some(id(2)));
    assert_eq!(r.pressed_layer(), Some(id(2)));
    assert!(r.hovered.is_none());
    assert!(
        !a.layers[&id(2)]
            .router
            .state(&a.layers[&id(2)].editor.engine, a.layers[&id(2)].handle())
            .hover
    );
    assert_ne!(a.layers[&id(2)].x(), 50.);
    assert!(a.layers[&id(1)].events.is_empty());
    r.release_outside(0, pointer(350.).button, &mut a);
    assert_eq!(a.layers[&id(2)].x(), 50.);
}
