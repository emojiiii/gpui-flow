//! Presentation-aware wheel targets retain the canonical router/default contract.
use gpui_flow_core::{Point, Viewport};
use gpui_flow_editor::SceneEditor;
use gpui_flow_engine::{ElementHandle, EngineError};
use gpui_flow_interaction::{scene::*, *};
use gpui_flow_scene::{
    Affine, Color, DisplayObject, EventKind, ListenerOptions, Scene, Style, display,
};

struct Lab {
    editor: SceneEditor,
    host: ScenePluginHost,
    router: SceneEventRouter,
    a: ElementHandle,
    b: ElementHandle,
}
impl Lab {
    fn new() -> Self {
        let mut scene = Scene::new();
        for (id, x) in [("a", 0.), ("b", 200.)] {
            scene
                .insert(
                    DisplayObject::new(id, display::Rect::new(80., 80.))
                        .with_transform(Affine::translation(x, 0.))
                        .with_style(Style::fill(Color::BLACK)),
                    None,
                    None,
                )
                .unwrap();
        }
        let editor = SceneEditor::new(scene).unwrap();
        let a = editor.engine.handle(&"a".into()).unwrap();
        let b = editor.engine.handle(&"b".into()).unwrap();
        Self {
            editor,
            host: ScenePluginHost::with_defaults(),
            router: SceneEventRouter::new(),
            a,
            b,
        }
    }
    fn prevent_wheel(&mut self, handle: ElementHandle) {
        self.editor
            .engine
            .on(handle, EventKind::Wheel, ListenerOptions::default(), |e| {
                e.prevent_default()
            })
            .unwrap();
    }
    fn picked(&mut self, position: Point, target: Option<ElementHandle>) -> RoutedReport {
        self.router
            .dispatch_picked_wheel(
                &mut self.editor,
                &mut self.host,
                position,
                Point::new(0., 40.),
                zoom_modifiers(),
                target,
            )
            .unwrap()
    }
    fn legacy(&mut self, position: Point) -> RoutedReport {
        self.router.dispatch(
            &mut self.editor,
            &mut self.host,
            CanvasEvent::Wheel {
                position,
                delta: Point::new(0., 40.),
                modifiers: zoom_modifiers(),
            },
        )
    }
    fn start_press(&mut self) {
        self.router.dispatch(
            &mut self.editor,
            &mut self.host,
            CanvasEvent::PointerDown(PointerInput::left(
                Point::new(20., 20.),
                HitTarget::Object("a".into()),
            )),
        );
        self.router.dispatch(
            &mut self.editor,
            &mut self.host,
            CanvasEvent::KeyDown {
                key: KeyCode::Space,
                modifiers: Modifiers::default(),
            },
        );
        assert!(self.host.has_capture());
        assert!(self.router.has_pressed(&self.editor.engine));
        assert!(self.router.space_held());
    }
}
fn zoom_modifiers() -> Modifiers {
    Modifiers {
        control: true,
        ..Default::default()
    }
}
fn wheel_target(report: &RoutedReport) -> Option<&str> {
    let mut events = report.events.iter().filter(|e| e.kind == EventKind::Wheel);
    let target = events.next().map(|e| e.target.as_str());
    assert!(
        events.next().is_none(),
        "wheel must be dispatched only once"
    );
    target
}

#[test]
fn explicit_presentation_hit_wins_over_a_different_canonical_target() {
    let mut lab = Lab::new();
    let position = Point::new(220., 20.);
    // A presentation/alpha picker chose a although canonical geometry hits b.
    let picked = lab.picked(position, Some(lab.a));
    assert_eq!(wheel_target(&picked), Some("a"));
    assert!(picked.errors.is_empty());
    assert!(picked.default.viewport_changed);
    lab.editor.viewport = Viewport::default();
    let legacy = lab.legacy(position);
    assert_eq!(wheel_target(&legacy), Some("b"));
    assert!(legacy.errors.is_empty());
}

#[test]
fn moved_presentation_target_receives_wheel_over_canonical_empty_space() {
    let mut lab = Lab::new();
    lab.prevent_wheel(lab.a);
    let before = lab.editor.viewport;
    let position = Point::new(500., 250.);
    let picked = lab.picked(position, Some(lab.a));
    assert_eq!(wheel_target(&picked), Some("a"));
    assert!(picked.default.default_prevented);
    assert_eq!(lab.editor.viewport, before);
    let legacy = lab.legacy(position);
    assert_eq!(wheel_target(&legacy), None);
    assert_ne!(lab.editor.viewport, before);
}

#[test]
fn explicit_background_never_repicks_or_invokes_a_canonical_listener() {
    let mut lab = Lab::new();
    lab.prevent_wheel(lab.a);
    let before = lab.editor.viewport;
    let background = lab.picked(Point::new(20., 20.), None);
    assert_eq!(wheel_target(&background), None);
    assert!(!background.default.default_prevented);
    assert_ne!(lab.editor.viewport, before);
    // Explicit override is operation-local; legacy dispatch remains canonical.
    lab.editor.viewport = before;
    let legacy = lab.legacy(Point::new(20., 20.));
    assert_eq!(wheel_target(&legacy), Some("a"));
    assert!(legacy.default.default_prevented);
    assert_eq!(lab.editor.viewport, before);
}

#[test]
fn picked_listener_can_prevent_zoom_without_changing_document_or_history() {
    let mut lab = Lab::new();
    lab.prevent_wheel(lab.a);
    let before = lab.editor.scene().clone();
    let viewport = lab.editor.viewport;
    let revision = lab.editor.engine.revision();
    let picked = lab.picked(Point::new(220., 20.), Some(lab.a));
    assert_eq!(wheel_target(&picked), Some("a"));
    assert!(picked.default.default_prevented);
    assert_eq!(lab.editor.viewport, viewport);
    assert_eq!(lab.editor.scene(), &before);
    assert_eq!(lab.editor.engine.revision(), revision);
    assert!(lab.editor.selection.ids.is_empty());
    assert!(!lab.editor.undo());
}

#[test]
fn pressed_owner_precedes_explicit_target_and_explicit_background() {
    let mut lab = Lab::new();
    lab.prevent_wheel(lab.a);
    lab.start_press();
    for target in [Some(lab.b), None] {
        let picked = lab.picked(Point::new(220., 20.), target);
        assert_eq!(wheel_target(&picked), Some("a"));
        assert!(picked.default.default_prevented);
        assert!(lab.host.has_capture());
        assert!(lab.router.has_pressed(&lab.editor.engine));
    }
}

#[test]
fn foreign_detached_and_replaced_handles_fail_before_any_state_or_queue_mutation() {
    for kind in 0..3 {
        let mut lab = Lab::new();
        let (target, expected) = match kind {
            0 => (Lab::new().a, EngineError::InvalidHandle),
            1 => {
                lab.editor.engine.detach(lab.b).unwrap();
                (lab.b, EngineError::DetachedHandle)
            }
            _ => {
                lab.editor.engine.destroy(lab.b).unwrap();
                lab.editor
                    .engine
                    .transaction(|scene| {
                        scene.insert(
                            DisplayObject::new("b", display::Rect::new(80., 80.)),
                            None,
                            None,
                        )?;
                        Ok(())
                    })
                    .unwrap();
                assert_ne!(lab.editor.engine.handle(&"b".into()), Some(lab.b));
                (lab.b, EngineError::InvalidHandle)
            }
        };
        lab.start_press();
        let scene = lab.editor.scene().clone();
        let viewport = lab.editor.viewport;
        let selection = lab.editor.selection.clone();
        let revision = lab.editor.engine.revision();
        let editing = lab.editor.editing();
        let sender = lab.router.action_sender();
        for _ in 0..256 {
            sender.enqueue(lab.a, SceneCommand::ClearSelection).unwrap();
        }
        let result = lab.router.dispatch_picked_wheel(
            &mut lab.editor,
            &mut lab.host,
            Point::new(220., 20.),
            Point::new(0., 40.),
            zoom_modifiers(),
            Some(target),
        );
        assert_eq!(result.err(), Some(expected));
        assert_eq!(lab.editor.scene(), &scene);
        assert_eq!(lab.editor.viewport, viewport);
        assert_eq!(lab.editor.selection, selection);
        assert_eq!(lab.editor.engine.revision(), revision);
        assert_eq!(lab.editor.editing(), editing);
        assert_eq!(lab.router.focused(&lab.editor.engine), Some(lab.a));
        assert!(lab.router.has_pressed(&lab.editor.engine));
        assert!(lab.router.space_held());
        assert!(lab.host.has_capture());
        assert_eq!(
            sender.enqueue(lab.a, SceneCommand::ClearSelection),
            Err(ActionQueueError::Full)
        );
    }
}

#[test]
fn picked_wheel_uses_the_current_viewport_for_world_and_local_coordinates() {
    use std::{cell::Cell, rc::Rc};
    let mut lab = Lab::new();
    lab.editor.viewport = Viewport {
        pan: Point::new(100., 30.),
        zoom: 2.,
        ..Default::default()
    };
    let local = Rc::new(Cell::new(None));
    let observed = local.clone();
    lab.editor
        .engine
        .on(
            lab.b,
            EventKind::Wheel,
            ListenerOptions::default(),
            move |e| {
                observed.set(e.local_position);
                e.prevent_default();
            },
        )
        .unwrap();
    let report = lab.picked(Point::new(540., 70.), Some(lab.b));
    let event = report
        .events
        .iter()
        .find(|e| e.kind == EventKind::Wheel)
        .unwrap();
    assert_eq!(event.world_position, Some(Point::new(220., 20.)));
    assert_eq!(local.get(), Some(Point::new(20., 20.)));
}
