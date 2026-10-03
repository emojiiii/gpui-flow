use gpui_flow_editor::SceneEditor;
use gpui_flow_interaction::scene::{SceneCommand, ScenePluginHost, SceneResizeGeometry};
use gpui_flow_scene::{display, *};
#[test]
fn navigation_rejection_retains_view_and_history_then_recovers() {
    let mut s = Scene::new();
    s.insert(
        DisplayObject::new("r", display::Rect::new(40., 30.)).with_style(Style {
            fill: None,
            stroke: Some(
                Stroke::new(Color::BLACK, MIN_ALIGNED_STROKE_WIDTH)
                    .with_scale_fixed(StrokeScaleFixed::Fixed)
                    .with_alignment(StrokeAlignment::Outside),
            ),
            ..Default::default()
        }),
        None,
        None,
    )
    .unwrap();
    let mut e = SceneEditor::new(s).unwrap();
    let mut host = ScenePluginHost::with_defaults();
    let before = e.viewport;
    let doc = e.scene().to_json().unwrap();
    assert!(
        host.execute(
            &mut e,
            SceneCommand::Zoom {
                position: Point::ZERO,
                zoom: 2.
            }
        )
        .is_err()
    );
    assert_eq!(e.viewport, before);
    assert_eq!(e.engine.revision(), 0);
    assert!(!e.undo());
    host.execute(
        &mut e,
        SceneCommand::Zoom {
            position: Point::ZERO,
            zoom: 0.5,
        },
    )
    .unwrap();
    assert_eq!(e.viewport.zoom, 0.5);
    assert_eq!(e.scene().to_json().unwrap(), doc);
    assert_eq!(e.engine.revision(), 0);
    let handles = SceneResizeGeometry::for_target(&e, &"r".into()).unwrap();
    assert_eq!(
        Some(handles.bounds),
        e.scene().local_bounds_for_view(&"r".into(), 0.5).unwrap()
    );
    host.execute(
        &mut e,
        SceneCommand::Zoom {
            position: Point::ZERO,
            zoom: 1.,
        },
    )
    .unwrap();
    assert_eq!(e.viewport.zoom, 1.);
}

fn square(policy: StrokeScaleFixed) -> SceneEditor {
    let mut s = Scene::new();
    s.insert(
        DisplayObject::new("r", display::Rect::new(100., 100.)).with_style(Style {
            fill: None,
            stroke: Some(
                Stroke::new(Color::BLACK, 10.)
                    .with_scale_fixed(policy)
                    .with_alignment(StrokeAlignment::Outside)
                    .with_join(StrokeJoin::Miter),
            ),
            ..Default::default()
        }),
        None,
        None,
    )
    .unwrap();
    let mut e = SceneEditor::new(s).unwrap();
    e.selection.select("r".into(), false);
    e
}
#[test]
fn fit_solves_actual_fixed_width_and_numeric_threshold_with_and_without_constraints() {
    use gpui_flow_interaction::scene::plugins::*;
    for constraints in [false, true] {
        for (policy, expected) in [
            (StrokeScaleFixed::Fixed, 0.4),
            (StrokeScaleFixed::Threshold(1.), 0.5),
        ] {
            let mut e = square(policy);
            let mut host = ScenePluginHost::with_defaults();
            if constraints {
                host.install(SceneViewportConstraintsPlugin::default())
                    .unwrap();
            }
            let doc = e.scene().to_json().unwrap();
            host.execute(
                &mut e,
                SceneCommand::Fit {
                    canvas: Size::new(60., 60.),
                    padding: 0.,
                },
            )
            .unwrap();
            assert!(
                (e.viewport.zoom - expected).abs() < 1e-5,
                "{policy:?} {}",
                e.viewport.zoom
            );
            let bounds = e
                .scene()
                .visible_bounds_for_view(e.viewport.zoom)
                .unwrap()
                .unwrap();
            let screen = e.viewport.world_rect_to_screen(bounds);
            assert!(
                screen.min_x() >= -0.0001
                    && screen.min_y() >= -0.0001
                    && screen.max_x() <= 60.0001
                    && screen.max_y() <= 60.0001
            );
            assert_eq!(e.scene().to_json().unwrap(), doc);
            assert_eq!(e.engine.revision(), 0);
            assert!(!e.undo());
            let before = e.viewport;
            assert!(
                host.execute(
                    &mut e,
                    SceneCommand::Fit {
                        canvas: Size::new(5., 5.),
                        padding: 0.
                    }
                )
                .is_err()
            );
            assert_eq!(e.viewport, before);
        }
    }
}
#[test]
fn fixed_stroke_drag_constraints_and_edge_snapping_use_current_view_bounds() {
    use gpui_flow_interaction::scene::{plugins::*, selection_bounds};
    let mut e = square(StrokeScaleFixed::Fixed);
    e.viewport.zoom = 0.5;
    assert_eq!(
        selection_bounds(&e).unwrap().unwrap(),
        Bounds::new(Point::new(-20., -20.), Size::new(140., 140.))
    );
    let mut host = ScenePluginHost::with_defaults();
    host.install(SceneDragConstraintsPlugin {
        constraints: SceneDragConstraints {
            bounds: Some(DragBounds::World(Bounds::new(
                Point::ZERO,
                Size::new(140., 140.),
            ))),
            ..Default::default()
        },
    })
    .unwrap();
    host.execute(
        &mut e,
        SceneCommand::MoveSelectionTo {
            anchor: Point::new(-10., -10.),
        },
    )
    .unwrap();
    let bounds = selection_bounds(&e).unwrap().unwrap();
    assert_eq!(bounds.origin, Point::ZERO);
    assert_eq!(bounds.size, Size::new(140., 140.));
    let mut e = square(StrokeScaleFixed::Fixed);
    e.viewport.zoom = 0.5;
    e.transaction(|s| {
        s.insert(
            DisplayObject::new("target", display::Rect::new(30., 30.))
                .with_transform(Affine::translation(200., 0.))
                .with_style(Style {
                    fill: None,
                    stroke: Some(
                        Stroke::new(Color::BLACK, 10.)
                            .with_scale_fixed(StrokeScaleFixed::Fixed)
                            .with_alignment(StrokeAlignment::Outside)
                            .with_join(StrokeJoin::Miter),
                    ),
                    ..Default::default()
                }),
            None,
            None,
        )
    })
    .unwrap();
    let mut host = ScenePluginHost::with_defaults();
    host.install(SceneEdgeSnappingPlugin {
        threshold_pixels: 3.,
        include_centers: false,
    })
    .unwrap();
    host.execute(
        &mut e,
        SceneCommand::MoveSelectionTo {
            anchor: Point::new(42., -20.),
        },
    )
    .unwrap();
    // Moving right edge 182 snaps to the target's resolved left edge 180.
    assert!((selection_bounds(&e).unwrap().unwrap().max_x() - 180.).abs() < 0.001);
}

#[test]
fn fixed_fit_can_use_a_valid_interior_when_an_extreme_probe_is_inadmissible() {
    let mut e = square(StrokeScaleFixed::Fixed);
    e.transaction(|s| {
        s.update_object(&"r".into(), |o| {
            o.style.stroke.as_mut().unwrap().width = MIN_ALIGNED_STROKE_WIDTH
        })
    })
    .unwrap();
    assert!(e.scene().validate_view(e.viewport.max_zoom).is_err());
    let mut host = ScenePluginHost::with_defaults();
    host.execute(
        &mut e,
        SceneCommand::Fit {
            canvas: Size::new(75., 75.),
            padding: 0.,
        },
    )
    .unwrap();
    assert!(e.viewport.zoom > 0.7 && e.viewport.zoom < 0.75);
    let bounds = e
        .scene()
        .visible_bounds_for_view(e.viewport.zoom)
        .unwrap()
        .unwrap();
    assert!(bounds.size.width * e.viewport.zoom <= 75.0001);
    e.scene().validate_view(e.viewport.zoom).unwrap();
}

#[test]
fn fixed_fit_respects_the_y_only_numeric_threshold_discontinuity() {
    let mut e = square(StrokeScaleFixed::Threshold(1.));
    e.transaction(|s| s.update_object(&"r".into(), |o| o.transform = Affine::scale(0.5, 2.)))
        .unwrap();
    let mut host = ScenePluginHost::with_defaults();
    host.execute(
        &mut e,
        SceneCommand::Fit {
            canvas: Size::new(100., 150.),
            padding: 0.,
        },
    )
    .unwrap();
    assert!((e.viewport.zoom - 0.5).abs() < 0.00001 && e.viewport.zoom <= 0.5);
    let bounds = e
        .scene()
        .visible_bounds_for_view(e.viewport.zoom)
        .unwrap()
        .unwrap();
    assert!(bounds.size.height * e.viewport.zoom <= 150.0001);
    let above = f32::from_bits(0.5f32.to_bits() + 1);
    assert!(
        e.scene()
            .visible_bounds_for_view(above)
            .unwrap()
            .unwrap()
            .size
            .height
            * above
            > 150.
    );
}

#[test]
fn fixed_fit_helper_rejects_derived_snapshot_instead_of_reusing_its_width_at_a_new_zoom() {
    let e = square(StrokeScaleFixed::Fixed);
    let derived = e.scene().resolved_for_view(2.).unwrap();
    assert!(
        gpui_flow_interaction::scene::fit_scene_viewport(
            &derived,
            e.viewport,
            Size::new(60., 60.),
            0.
        )
        .is_err()
    );
}

#[test]
fn ambiguous_rotated_numeric_zoom_is_rejected_atomically_and_recovers_outside_the_band() {
    let mut e = square(StrokeScaleFixed::Threshold(1.));
    e.transaction(|s| {
        s.update_object(&"r".into(), |o| {
            o.transform = Affine::rotation(60.) * Affine::scale(0.5, 2.)
        })
    })
    .unwrap();
    let mut host = ScenePluginHost::with_defaults();
    let before = e.viewport;
    let revision = e.engine.revision();
    let authored = e.scene().clone();
    let error = host
        .execute(
            &mut e,
            SceneCommand::Zoom {
                position: Point::ZERO,
                zoom: 0.5,
            },
        )
        .unwrap_err();
    assert!(error.to_string().contains("ambiguous"));
    assert_eq!(e.viewport, before);
    assert_eq!(e.engine.revision(), revision);
    assert_eq!(e.scene(), &authored);
    host.execute(
        &mut e,
        SceneCommand::Zoom {
            position: Point::ZERO,
            zoom: 0.49,
        },
    )
    .unwrap();
    assert_eq!(e.viewport.zoom, 0.49);
    assert_eq!(e.engine.revision(), revision);
}
