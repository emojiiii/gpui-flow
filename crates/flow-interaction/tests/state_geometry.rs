//! Geometry/paint/picking share one accepted projection without canonical edits.
use gpui_flow_editor::SceneEditor;
use gpui_flow_engine::{
    ChangeOrigin, ElementHandle, EngineError, LayoutProvider, PropertyKey, SceneEngine,
};
use gpui_flow_interaction::{scene::*, *};
use gpui_flow_scene::{Affine, Color, DisplayObject, Point, Scene, Shape, Size, Style, display};
use std::{collections::BTreeMap, time::Duration};

struct Lab {
    editor: SceneEditor,
    router: SceneEventRouter,
    host: ScenePluginHost,
    sheet: SceneStateStyles,
    a: ElementHandle,
}
impl Lab {
    fn new() -> Self {
        let mut scene = Scene::new();
        scene
            .insert(
                DisplayObject::new("a", display::Rect::new(40., 30.))
                    .with_style(Style::fill(Color::BLACK)),
                None,
                None,
            )
            .unwrap();
        scene
            .insert(
                DisplayObject::new("b", display::Rect::new(10., 10.))
                    .with_transform(Affine::translation(40., 0.))
                    .with_style(Style::fill(Color::WHITE)),
                None,
                None,
            )
            .unwrap();
        let editor = SceneEditor::new(scene).unwrap();
        let a = editor.engine.handle(&"a".into()).unwrap();
        let mut sheet = SceneStateStyles::new(&editor.engine);
        sheet
            .bind(&editor.engine, a, StateStyles::default())
            .unwrap();
        Self {
            editor,
            a,
            sheet,
            router: SceneEventRouter::new(),
            host: ScenePluginHost::new(),
        }
    }
    fn shape(&mut self, shape: &Shape) {
        self.editor
            .engine
            .set_property(self.a, PropertyKey::Shape, serde_json::json!(shape))
            .unwrap();
        self.a = self.editor.engine.handle(&"a".into()).unwrap();
        self.sheet
            .bind(&self.editor.engine, self.a, StateStyles::default())
            .unwrap();
    }
    fn named(&mut self, name: &str, geometry: StateGeometry) {
        self.sheet
            .set_named_geometry(
                &self.editor.engine,
                self.a,
                BTreeMap::from([(name.into(), geometry)]),
            )
            .unwrap();
        self.select(Some(name));
    }
    fn select(&mut self, name: Option<&str>) {
        self.sheet
            .select_state(&self.editor.engine, self.a, name)
            .unwrap();
    }
    fn timing(&mut self, ms: u64, out: Option<u64>) {
        self.sheet
            .set_transitions(
                &self.editor.engine,
                self.a,
                StateTransitions {
                    transition: StateTransition::new(ms),
                    transition_out: out.map(StateTransition::new),
                },
            )
            .unwrap();
    }
    fn at(&mut self, ms: u64) -> Scene {
        self.sheet
            .presentation_at(&self.editor, &self.router, Duration::from_millis(ms))
            .unwrap()
            .into_owned()
    }
    fn width(&mut self, ms: u64) -> f32 {
        self.at(ms)
            .get(&"a".into())
            .unwrap()
            .object
            .shape
            .local_bounds()
            .unwrap()
            .size
            .width
    }
    fn event(&mut self, event: CanvasEvent) {
        self.router
            .dispatch(&mut self.editor, &mut self.host, event);
    }
}
fn grow(width: f32) -> StateGeometry {
    StateGeometry {
        width: Some(width),
        ..Default::default()
    }
}
fn pointer() -> PointerInput {
    PointerInput::left(Point::new(5., 5.), HitTarget::Object("a".into()))
}

#[test]
fn geometry_changes_real_layout_paint_and_hit_without_canonical_or_history_mutation() {
    let mut l = Lab::new();
    let original = l.editor.scene().clone();
    let revision = l.editor.engine.revision();
    l.named(
        "wide",
        StateGeometry {
            x: Some(100.),
            y: Some(20.),
            width: Some(90.),
            height: Some(50.),
        },
    );
    let scene = l.at(0);
    let hit_engine = l.sheet.geometry_engine(&l.editor.engine).unwrap();
    assert_eq!(scene, *hit_engine.scene());
    let hit = hit_engine
        .hit_test(Point::new(180., 50.), Default::default())
        .unwrap()
        .unwrap();
    assert_eq!(hit.id.as_str(), "a");
    assert_eq!(hit.local_point, Point::new(80., 30.));
    assert!(
        l.editor
            .engine
            .hit_test(Point::new(180., 50.), Default::default())
            .unwrap()
            .is_none()
    );
    assert_eq!(
        scene.paint_list().unwrap()[0].world_transform,
        hit_engine.world_transform(&"a".into()).unwrap()
    );
    assert_eq!(l.editor.scene(), &original);
    assert_eq!(l.editor.engine.revision(), revision);
    assert!(!l.editor.undo());
    l.select(None);
    assert_eq!(l.at(0), original);
    assert!(l.sheet.geometry_engine(&l.editor.engine).is_none());
}

#[test]
fn geometry_interpolates_and_rapid_reversal_samples_current_host_time() {
    let mut l = Lab::new();
    l.named("wide", grow(140.));
    l.timing(100, Some(200));
    assert_eq!(l.width(0), 40.);
    assert_eq!(l.width(40), 80.);
    l.select(None);
    assert_eq!(l.width(60), 100.);
    assert_eq!(l.width(160), 70.);
    l.select(Some("wide"));
    assert_eq!(l.width(160), 70.);
    assert_eq!(l.width(210), 105.);
    assert_eq!(l.width(260), 140.);
    assert!(!l.sheet.has_active_transitions());
}

#[test]
fn external_geometry_edits_cancel_only_the_edited_channel_and_undo_keeps_live_baseline() {
    let mut l = Lab::new();
    l.named(
        "wide",
        StateGeometry {
            x: Some(100.),
            width: Some(140.),
            ..Default::default()
        },
    );
    l.timing(100, None);
    l.at(0);
    l.at(30);
    l.editor
        .transaction(|scene| scene.update_object(&"a".into(), |object| object.transform.tx = 20.))
        .unwrap();
    let scene = l.at(50);
    assert_eq!(scene.get(&"a".into()).unwrap().object.transform.tx, 100.);
    assert_eq!(l.width(50), 90.); // independent width motion was not canceled
    l.select(None);
    l.at(50);
    assert_eq!(l.at(150).get(&"a".into()).unwrap().object.transform.tx, 20.);
    assert!(l.editor.undo());
    assert_eq!(l.at(150).get(&"a".into()).unwrap().object.transform.tx, 0.);
    assert_eq!(
        l.editor
            .scene()
            .get(&"a".into())
            .unwrap()
            .object
            .transform
            .tx,
        0.
    );
}

#[test]
fn geometry_fields_merge_with_disabled_suppression_and_preserve_affine_linear_terms() {
    let mut l = Lab::new();
    let transform = Affine::new(0.8, 0.6, -0.6, 0.8, 3., 4.);
    l.editor
        .engine
        .set_property(l.a, PropertyKey::Transform, serde_json::json!(transform))
        .unwrap();
    l.named(
        "named",
        StateGeometry {
            x: Some(25.),
            width: Some(80.),
            ..Default::default()
        },
    );
    l.sheet
        .set_geometry_styles(
            &l.editor.engine,
            l.a,
            StateGeometryStyles {
                selected: StateGeometry {
                    y: Some(8.),
                    ..Default::default()
                },
                focus: grow(90.),
                hover: grow(100.),
                press: grow(110.),
                disabled: grow(120.),
                ..Default::default()
            },
        )
        .unwrap();
    l.editor.selection.ids.insert("a".into());
    l.event(CanvasEvent::PointerDown(pointer()));
    let object = l.at(0).get(&"a".into()).unwrap().object.clone();
    assert_eq!(
        object.transform,
        Affine {
            tx: 25.,
            ty: 8.,
            ..transform
        }
    );
    assert_eq!(l.width(0), 110.);
    l.sheet.set_disabled(&l.editor.engine, l.a, true).unwrap();
    assert_eq!(l.width(0), 120.);
    l.sheet
        .set_geometry_styles(&l.editor.engine, l.a, StateGeometryStyles::default())
        .unwrap();
    assert_eq!(l.width(0), 80.); // empty disabled still suppresses pointer geometry
}

#[test]
fn zero_opacity_state_does_not_erase_its_presented_hit_target() {
    let mut l = Lab::new();
    l.named("wide", grow(100.));
    l.sheet
        .set_named_states(
            &l.editor.engine,
            l.a,
            BTreeMap::from([(
                "wide".into(),
                StatePaint {
                    opacity: Some(0.),
                    ..Default::default()
                },
            )]),
        )
        .unwrap();
    assert_eq!(l.at(0).get(&"a".into()).unwrap().object.style.opacity, 0.);
    let hit_engine = l.sheet.geometry_engine(&l.editor.engine).unwrap();
    assert_eq!(
        hit_engine
            .scene()
            .get(&"a".into())
            .unwrap()
            .object
            .style
            .opacity,
        1.
    );
    assert_eq!(
        hit_engine
            .hit_test(Point::new(90., 15.), Default::default())
            .unwrap()
            .unwrap()
            .id
            .as_str(),
        "a"
    );
}

#[test]
fn all_supported_sized_shapes_are_real_intrinsic_size_changes() {
    let shapes = [
        Shape::from(display::Rect::new(40., 30.)),
        Shape::from(display::Ellipse::new(40., 30.)),
        Shape::from(display::Text::new("hello", Size::new(40., 30.))),
        Shape::from(display::Image::new("image.png", Size::new(40., 30.))),
        Shape::from(display::Box::new(40., 30.)),
        Shape::from(display::Frame::new(40., 30.)),
    ];
    for shape in shapes {
        let mut l = Lab::new();
        l.shape(&shape);
        l.named(
            "size",
            StateGeometry {
                width: Some(70.),
                height: Some(60.),
                ..Default::default()
            },
        );
        let scene = l.at(0);
        let object = &scene.get(&"a".into()).unwrap().object;
        assert_eq!(
            object.shape.local_bounds().unwrap().size,
            Size::new(70., 60.)
        );
        assert_eq!(object.transform, Affine::IDENTITY);
        assert_eq!(object.shape.tag(), shape.tag());
    }
}

#[test]
fn unsupported_sizes_and_nonfinite_inactive_geometry_are_atomic_errors() {
    let mut l = Lab::new();
    l.named("good", grow(80.));
    for bad in [f32::NAN, f32::INFINITY, -1., 0.] {
        assert!(
            l.sheet
                .set_geometry_styles(
                    &l.editor.engine,
                    l.a,
                    StateGeometryStyles {
                        disabled: grow(bad),
                        ..Default::default()
                    }
                )
                .is_err()
        );
        assert_eq!(l.width(0), 80.);
    }
    for shape in [
        Shape::from(display::Group::new()),
        Shape::from(display::Canvas::new(40., 30.)),
    ] {
        let mut l = Lab::new();
        l.shape(&shape);
        assert!(
            l.sheet
                .set_named_geometry(
                    &l.editor.engine,
                    l.a,
                    BTreeMap::from([("bad".into(), grow(60.))])
                )
                .is_err()
        );
        l.named(
            "move",
            StateGeometry {
                x: Some(7.),
                ..Default::default()
            },
        );
        assert_eq!(l.at(0).get(&"a".into()).unwrap().object.transform.tx, 7.);
    }
}

struct FollowWidth;
impl LayoutProvider for FollowWidth {
    fn derive(&self, _: &SceneEngine, candidate: &Scene) -> Result<Scene, EngineError> {
        let width = candidate
            .get(&"a".into())
            .unwrap()
            .object
            .shape
            .local_bounds()
            .unwrap()
            .size
            .width;
        let mut next = candidate.clone();
        next.update_object(&"b".into(), |object| object.transform.tx = width)?;
        Ok(next)
    }
}
#[test]
fn installed_layout_reflows_unbound_siblings_in_both_paint_and_engine_hit() {
    let mut l = Lab::new();
    l.editor
        .engine
        .install_layout_provider(FollowWidth)
        .unwrap();
    l.named("wide", grow(100.));
    let scene = l.at(0);
    assert_eq!(scene.get(&"b".into()).unwrap().object.transform.tx, 100.);
    assert_eq!(
        l.sheet
            .geometry_engine(&l.editor.engine)
            .unwrap()
            .hit_test(Point::new(105., 5.), Default::default())
            .unwrap()
            .unwrap()
            .id
            .as_str(),
        "b"
    );
    assert_eq!(
        l.editor
            .scene()
            .get(&"b".into())
            .unwrap()
            .object
            .transform
            .tx,
        40.
    );
    l.select(None);
    assert_eq!(l.at(0), *l.editor.scene());
}
struct OwnWidth;
impl LayoutProvider for OwnWidth {
    fn derive(&self, _: &SceneEngine, candidate: &Scene) -> Result<Scene, EngineError> {
        let mut next = candidate.clone();
        next.update_object(&"a".into(), |object| {
            if let Shape::Rect(rect) = &mut object.shape {
                rect.size.width = 40.;
            }
        })?;
        Ok(next)
    }
}
#[test]
fn layout_owned_endpoint_is_rejected_even_at_zero_progress_without_clock_or_history_changes() {
    let mut l = Lab::new();
    l.editor.engine.install_layout_provider(OwnWidth).unwrap();
    let original = l.editor.scene().clone();
    l.named("wide", grow(100.));
    l.timing(100, None);
    assert!(
        l.sheet
            .presentation_at(&l.editor, &l.router, Duration::from_millis(50))
            .is_err()
    );
    assert_eq!(l.sheet.transition_time(), Duration::ZERO);
    assert!(!l.sheet.has_active_transitions());
    assert!(l.sheet.geometry_engine(&l.editor.engine).is_none());
    assert_eq!(l.editor.scene(), &original);
    assert!(!l.editor.undo());
}

#[test]
fn cached_geometry_reuses_engine_and_expires_after_edit_reset_unbind_and_failure() {
    let mut l = Lab::new();
    l.named("wide", grow(100.));
    l.at(0);
    let engine_id = l
        .sheet
        .geometry_engine(&l.editor.engine)
        .unwrap()
        .engine_id();
    l.at(1);
    assert_eq!(
        l.sheet
            .geometry_engine(&l.editor.engine)
            .unwrap()
            .engine_id(),
        engine_id
    );
    l.editor
        .engine
        .set_property(l.a, PropertyKey::Opacity, serde_json::json!(0.5))
        .unwrap();
    assert!(l.sheet.geometry_engine(&l.editor.engine).is_none());
    l.at(2);
    assert!(l.sheet.geometry_engine(&l.editor.engine).is_some());
    assert!(
        l.sheet
            .presentation_at(&l.editor, &l.router, Duration::ZERO)
            .is_err()
    );
    assert!(l.sheet.geometry_engine(&l.editor.engine).is_none());
    assert_eq!(l.sheet.transition_time(), Duration::from_millis(2));
    l.at(3);
    l.sheet.unbind(l.a);
    assert!(l.sheet.geometry_engine(&l.editor.engine).is_none());
}

#[test]
fn named_geometry_inherits_button_flags_but_child_definitions_and_timings_stay_local() {
    let mut l = Lab::new();
    l.shape(&Shape::from(display::Frame::new(40., 30.)));
    let b = l.editor.engine.handle(&"b".into()).unwrap();
    l.editor.engine.reparent(b, Some(l.a), None, false).unwrap();
    l.sheet
        .bind(&l.editor.engine, b, StateStyles::default())
        .unwrap();
    l.sheet.set_button(&l.editor.engine, l.a, true).unwrap();
    l.named("wide", grow(100.));
    l.sheet
        .set_named_geometry(
            &l.editor.engine,
            b,
            BTreeMap::from([("wide".into(), grow(20.))]),
        )
        .unwrap();
    l.sheet
        .set_state_transitions(
            &l.editor.engine,
            b,
            StateKey::Named("wide".into()),
            Some(StateTransitionOverrides {
                transition: Some(StateTransition::new(100)),
                transition_out: None,
            }),
        )
        .unwrap();
    l.at(0);
    let scene = l.at(50);
    assert_eq!(
        scene
            .get(&"a".into())
            .unwrap()
            .object
            .shape
            .local_bounds()
            .unwrap()
            .size
            .width,
        100.
    );
    assert_eq!(
        scene
            .get(&"b".into())
            .unwrap()
            .object
            .shape
            .local_bounds()
            .unwrap()
            .size
            .width,
        15.
    );
    l.sheet
        .set_inheritance(&l.editor.engine, b, StateInheritance::None)
        .unwrap();
    l.at(50);
    assert_eq!(
        l.at(150)
            .get(&"b".into())
            .unwrap()
            .object
            .shape
            .local_bounds()
            .unwrap()
            .size
            .width,
        10.
    );
}

#[test]
fn per_named_transition_entry_exit_and_binding_exit_fallback_follow_upstream_order() {
    let mut l = Lab::new();
    l.named("wide", grow(140.));
    l.timing(400, Some(200));
    l.sheet
        .set_state_transitions(
            &l.editor.engine,
            l.a,
            StateKey::Named("wide".into()),
            Some(StateTransitionOverrides {
                transition: Some(StateTransition::new(100)),
                transition_out: None,
            }),
        )
        .unwrap();
    l.at(0);
    assert_eq!(l.width(50), 90.);
    assert_eq!(l.width(100), 140.);
    l.select(None);
    l.at(100);
    assert_eq!(l.width(200), 90.);
    assert_eq!(l.width(300), 40.);
    l.sheet
        .set_state_transitions(
            &l.editor.engine,
            l.a,
            StateKey::Named("wide".into()),
            Some(StateTransitionOverrides {
                transition: Some(StateTransition::new(100)),
                transition_out: Some(StateTransition::new(0)),
            }),
        )
        .unwrap();
    l.select(Some("wide"));
    l.at(300);
    l.at(400);
    l.select(None);
    assert_eq!(l.width(400), 40.);
}

#[test]
fn per_state_entry_is_exit_fallback_when_binding_and_state_exit_are_unspecified() {
    let mut l = Lab::new();
    l.named("wide", grow(140.));
    l.timing(400, None);
    l.sheet
        .set_state_transitions(
            &l.editor.engine,
            l.a,
            StateKey::Named("wide".into()),
            Some(StateTransitionOverrides {
                transition: Some(StateTransition::new(100)),
                transition_out: None,
            }),
        )
        .unwrap();
    l.at(0);
    l.at(100);
    l.select(None);
    l.at(100);
    assert_eq!(l.width(150), 90.);
    assert_eq!(l.width(200), 40.);
}

#[test]
fn builtin_press_and_disabled_timings_override_only_their_own_transitions() {
    let mut l = Lab::new();
    l.timing(400, None);
    l.sheet
        .set_geometry_styles(
            &l.editor.engine,
            l.a,
            StateGeometryStyles {
                hover: grow(140.),
                press: grow(240.),
                disabled: grow(50.),
                ..Default::default()
            },
        )
        .unwrap();
    for (key, ms) in [
        (StateKey::Hover, 100),
        (StateKey::Press, 200),
        (StateKey::Disabled, 0),
    ] {
        l.sheet
            .set_state_transitions(
                &l.editor.engine,
                l.a,
                key,
                Some(StateTransitionOverrides {
                    transition: Some(StateTransition::new(ms)),
                    transition_out: None,
                }),
            )
            .unwrap();
    }
    l.event(CanvasEvent::PointerMove(pointer()));
    l.at(0);
    assert_eq!(l.width(50), 90.);
    l.event(CanvasEvent::PointerDown(pointer()));
    l.at(50);
    assert_eq!(l.width(150), 165.);
    l.sheet.set_disabled(&l.editor.engine, l.a, true).unwrap();
    assert_eq!(l.width(150), 50.);
    l.sheet.set_disabled(&l.editor.engine, l.a, false).unwrap();
    assert_eq!(l.width(150), 240.);
}

#[test]
fn explicit_finish_and_reconfigure_finish_geometry_channels() {
    let mut l = Lab::new();
    l.named("wide", grow(140.));
    l.timing(100, None);
    l.at(0);
    l.at(40);
    l.sheet.finish_transitions();
    assert_eq!(l.width(40), 140.);
    assert!(!l.sheet.has_active_transitions());
    l.select(None);
    l.at(40);
    assert!(l.sheet.has_active_transitions());
    l.sheet
        .set_state_transitions(&l.editor.engine, l.a, StateKey::Named("wide".into()), None)
        .unwrap();
    assert_eq!(l.width(40), 40.);
}

#[test]
fn pure_engine_preview_reuses_layout_and_preserves_revision_handles_scene_and_last_change() {
    let mut l = Lab::new();
    l.editor
        .engine
        .install_layout_provider(FollowWidth)
        .unwrap();
    let scene = l.editor.scene().clone();
    let revision = l.editor.engine.revision();
    let change = l.editor.engine.last_change().clone();
    let candidate = l
        .editor
        .engine
        .preview_properties_batch_with_origin(
            ChangeOrigin::Animation,
            [(
                l.a,
                PropertyKey::Shape,
                serde_json::json!(Shape::from(display::Rect::new(90., 30.))),
            )],
        )
        .unwrap();
    assert_eq!(candidate.get(&"b".into()).unwrap().object.transform.tx, 90.);
    assert_eq!(l.editor.scene(), &scene);
    assert_eq!(l.editor.engine.revision(), revision);
    assert_eq!(l.editor.engine.last_change(), &change);
    assert_eq!(l.editor.engine.handle(&"a".into()), Some(l.a));
    assert!(!l.editor.undo());
}

#[test]
fn container_resize_updates_descendant_clips_and_real_paint_bounds_together() {
    let mut l = Lab::new();
    l.shape(&Shape::from(display::Frame::new(40., 30.).rounded(5.)));
    let b = l.editor.engine.handle(&"b".into()).unwrap();
    l.editor.engine.reparent(b, Some(l.a), None, false).unwrap();
    l.named("wide", grow(80.));
    let scene = l.at(0);
    let geometry = l.sheet.geometry_engine(&l.editor.engine).unwrap();
    assert_eq!(
        geometry
            .hit_test(Point::new(45., 5.), Default::default())
            .unwrap()
            .unwrap()
            .id
            .as_str(),
        "b"
    );
    assert!(
        l.editor
            .engine
            .hit_test(Point::new(45., 5.), Default::default())
            .unwrap()
            .is_none()
    );
    let painted = scene.paint_list().unwrap();
    let cached = geometry.paint_list().unwrap();
    assert_eq!(painted[1].clips, cached[1].clips);
    assert_eq!(
        scene.world_bounds(&"a".into()).unwrap(),
        geometry.world_bounds(&"a".into()).unwrap()
    );
    assert!(
        geometry
            .hit_test(Point::new(90., 5.), Default::default())
            .unwrap()
            .is_none()
    );
}

#[test]
fn detach_reattach_starts_geometry_from_current_base_and_reset_prunes_old_identity() {
    let mut l = Lab::new();
    l.named("wide", grow(140.));
    l.timing(100, None);
    l.at(0);
    assert_eq!(l.width(40), 80.);
    let (subtree, _) = l.editor.engine.detach(l.a).unwrap();
    assert!(l.sheet.geometry_engine(&l.editor.engine).is_none());
    l.editor.engine.attach(&subtree, None, None).unwrap();
    assert_eq!(l.width(50), 40.);
    assert_eq!(l.width(100), 90.);
    l.editor
        .engine
        .reset_scene(l.editor.scene().clone())
        .unwrap();
    assert!(l.sheet.geometry_engine(&l.editor.engine).is_none());
    assert_eq!(l.width(150), 40.);
    assert!(l.sheet.is_empty());
}

#[test]
fn provider_replacement_or_removal_invalidates_picking_without_scene_revision_change() {
    let mut l = Lab::new();
    l.named("wide", grow(100.));
    l.at(0);
    let revision = l.editor.engine.revision();
    let token = l
        .editor
        .engine
        .install_layout_provider(FollowWidth)
        .unwrap();
    assert_eq!(l.editor.engine.revision(), revision);
    assert!(l.sheet.geometry_engine(&l.editor.engine).is_none());
    l.at(1);
    assert!(l.sheet.geometry_engine(&l.editor.engine).is_some());
    assert!(l.editor.engine.remove_layout_provider(token));
    assert!(l.sheet.geometry_engine(&l.editor.engine).is_none());
}

#[test]
fn indirectly_resized_canvas_is_rejected_at_endpoint_preflight() {
    struct ResizeCanvas;
    impl LayoutProvider for ResizeCanvas {
        fn derive(&self, _: &SceneEngine, candidate: &Scene) -> Result<Scene, EngineError> {
            let width = candidate
                .get(&"a".into())
                .unwrap()
                .object
                .shape
                .local_bounds()
                .unwrap()
                .size
                .width;
            let mut next = candidate.clone();
            next.update_object(&"canvas".into(), |object| {
                if let Shape::Canvas(canvas) = &mut object.shape {
                    canvas.size.width = width;
                }
            })?;
            Ok(next)
        }
    }
    let mut l = Lab::new();
    l.editor
        .engine
        .transaction(|scene| {
            scene.insert(
                DisplayObject::new("canvas", display::Canvas::new(40., 30.)),
                None,
                None,
            )
        })
        .unwrap();
    l.editor
        .engine
        .install_layout_provider(ResizeCanvas)
        .unwrap();
    l.named("wide", grow(100.));
    l.timing(100, None);
    let original = l.editor.scene().clone();
    let error = l
        .sheet
        .presentation_at(&l.editor, &l.router, Duration::from_millis(10))
        .unwrap_err();
    assert!(error.to_string().contains("Canvas"));
    assert_eq!(l.sheet.transition_time(), Duration::ZERO);
    assert!(!l.sheet.has_active_transitions());
    assert_eq!(l.editor.scene(), &original);
}

#[test]
fn custom_registered_hit_function_is_preserved_in_geometry_engine() {
    use gpui_flow_engine::{ElementDefinition, ElementRegistry, PropertySchema};
    struct LeftHalf;
    impl ElementDefinition for LeftHalf {
        fn kind(&self) -> &str {
            "state.left_half"
        }
        fn schema(&self) -> PropertySchema {
            PropertySchema::default()
        }
        fn shape(&self, _: &serde_json::Value) -> Result<Shape, String> {
            let rect = display::Rect::new(40., 30.);
            Ok(display::Custom::new(self.kind(), rect.bounds(), vec![rect.flatten(0.1)]).into())
        }
        fn hit_test(&self, _: &serde_json::Value, p: Point, _: f32) -> Option<bool> {
            Some(p.x < 20.)
        }
    }
    let mut registry = ElementRegistry::default();
    registry.register(LeftHalf).unwrap();
    let mut scene = Scene::new();
    scene
        .insert(
            registry
                .create("state.left_half", "custom", serde_json::json!({}))
                .unwrap(),
            None,
            None,
        )
        .unwrap();
    let editor = SceneEditor::from_engine(SceneEngine::with_registry(scene, registry).unwrap());
    let handle = editor.engine.handle(&"custom".into()).unwrap();
    let mut sheet = SceneStateStyles::new(&editor.engine);
    sheet
        .bind(&editor.engine, handle, StateStyles::default())
        .unwrap();
    sheet
        .set_named_geometry(
            &editor.engine,
            handle,
            BTreeMap::from([(
                "move".into(),
                StateGeometry {
                    x: Some(100.),
                    ..Default::default()
                },
            )]),
        )
        .unwrap();
    sheet
        .select_state(&editor.engine, handle, Some("move"))
        .unwrap();
    sheet
        .presentation(&editor, &SceneEventRouter::new())
        .unwrap();
    let geometry = sheet.geometry_engine(&editor.engine).unwrap();
    assert!(
        geometry
            .hit_test(Point::new(110., 15.), Default::default())
            .unwrap()
            .is_some()
    );
    assert!(
        geometry
            .hit_test(Point::new(130., 15.), Default::default())
            .unwrap()
            .is_none()
    );
    assert!(
        sheet
            .set_geometry_styles(
                &editor.engine,
                handle,
                StateGeometryStyles {
                    hover: grow(50.),
                    ..Default::default()
                }
            )
            .is_err()
    );
}

#[test]
fn placeholder_geometry_is_explicit_but_placeholder_content_never_enters_reactive_measurement() {
    struct TextContentCheck;
    impl LayoutProvider for TextContentCheck {
        fn derive(&self, _: &SceneEngine, candidate: &Scene) -> Result<Scene, EngineError> {
            let Shape::Text(text) = &candidate.get(&"a".into()).unwrap().object.shape else {
                unreachable!()
            };
            if !text.content.is_empty() {
                return Err(EngineError::Layout(
                    "placeholder leaked into measure".into(),
                ));
            }
            Ok(candidate.clone())
        }
    }
    let mut l = Lab::new();
    l.shape(&Shape::from(display::Text::new("", Size::new(40., 30.))));
    l.editor
        .engine
        .install_layout_provider(TextContentCheck)
        .unwrap();
    l.sheet
        .set_placeholder(
            &l.editor.engine,
            l.a,
            Some(TextPlaceholder::new("Hint", StatePaint::default())),
        )
        .unwrap();
    l.sheet
        .set_geometry_styles(
            &l.editor.engine,
            l.a,
            StateGeometryStyles {
                placeholder: grow(100.),
                ..Default::default()
            },
        )
        .unwrap();
    let scene = l.at(0);
    let Shape::Text(presented) = &scene.get(&"a".into()).unwrap().object.shape else {
        unreachable!()
    };
    assert_eq!(presented.content, "Hint");
    assert_eq!(presented.size.width, 100.);
    let Shape::Text(hit) = &l
        .sheet
        .geometry_presentation(&l.editor.engine)
        .unwrap()
        .get(&"a".into())
        .unwrap()
        .object
        .shape
    else {
        unreachable!()
    };
    assert!(hit.content.is_empty());
    assert_eq!(hit.size.width, 100.);
}

#[test]
fn later_invalid_geometry_discards_an_earlier_new_channel_and_accepted_clock() {
    let mut l = Lab::new();
    let b = l.editor.engine.handle(&"b".into()).unwrap();
    l.sheet
        .bind(&l.editor.engine, b, StateStyles::default())
        .unwrap();
    l.sheet
        .set_named_geometry(
            &l.editor.engine,
            b,
            BTreeMap::from([(
                "far".into(),
                StateGeometry {
                    x: Some(90.),
                    ..Default::default()
                },
            )]),
        )
        .unwrap();
    l.sheet
        .select_state(&l.editor.engine, b, Some("far"))
        .unwrap();
    l.sheet
        .set_transitions(
            &l.editor.engine,
            b,
            StateTransitions {
                transition: StateTransition::new(100),
                transition_out: None,
            },
        )
        .unwrap();
    l.editor.engine.install_layout_provider(OwnWidth).unwrap();
    l.named("bad", grow(100.));
    let before = l.editor.scene().clone();
    assert!(
        l.sheet
            .presentation_at(&l.editor, &l.router, Duration::from_millis(50))
            .is_err()
    );
    assert_eq!(l.sheet.transition_time(), Duration::ZERO);
    assert!(!l.sheet.has_active_transitions());
    assert_eq!(l.editor.scene(), &before);
    l.select(None);
    l.at(50);
    assert_eq!(l.at(100).get(&"b".into()).unwrap().object.transform.tx, 65.);
}
