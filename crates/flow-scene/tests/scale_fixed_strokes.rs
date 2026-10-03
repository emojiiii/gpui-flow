use gpui_flow_scene::{display, *};

fn fixed(policy: StrokeScaleFixed, alignment: StrokeAlignment) -> Stroke {
    Stroke::new(Color::BLACK, 10.)
        .with_scale_fixed(policy)
        .with_alignment(alignment)
        .with_cap(StrokeCap::Butt)
        .with_join(StrokeJoin::Miter)
}
fn object(policy: StrokeScaleFixed, alignment: StrokeAlignment) -> DisplayObject {
    DisplayObject::new("r", display::Rect::new(40., 30.)).with_style(Style {
        fill: None,
        stroke: Some(fixed(policy, alignment)),
        ..Default::default()
    })
}
fn scene(policy: StrokeScaleFixed, alignment: StrokeAlignment) -> Scene {
    let mut scene = Scene::new();
    scene.insert(object(policy, alignment), None, None).unwrap();
    scene
}
#[test]
fn scalar_x_goldens_and_strict_either_axis_threshold() {
    use StrokeScaleFixed::{Fixed, Threshold};
    for (policy, x, y, w) in [
        (Fixed, 2., 4., 5.),
        (Fixed, 0.5, 0.5, 20.),
        (Threshold(1.), 0.5, 0.5, 10.),
        (Threshold(2.), 1., 1., 5.),
        (Threshold(1.), 0.5, 2., 20.),
        (Threshold(1.), 0.5, 1., 10.),
        (Threshold(2.), 2., 2., 5.),
        (Threshold(1.), 0.5, 1. + f32::EPSILON as f64, 20.),
        (Threshold(1.), 0.5, 1. - f32::EPSILON as f64, 10.),
    ] {
        let stroke = fixed(policy, StrokeAlignment::Center).with_dashes(vec![7., 3., 2.], -4.);
        let resolved = stroke
            .resolved(StrokeScaleData::exact(x, y).unwrap(), 1.)
            .unwrap();
        assert_eq!(resolved.width, w, "{policy:?} {x} {y}");
        assert_eq!(resolved.dash_pattern, stroke.dash_pattern);
        assert_eq!(resolved.dash_offset, stroke.dash_offset);
        assert!(resolved.uses_advanced_geometry());
        assert_eq!(stroke.width, 10.);
    }
}
#[test]
fn serde_defaults_aliases_and_derived_geometry_are_explicit() {
    let ordinary = Stroke::new(Color::BLACK, 3.);
    let mut json = serde_json::to_value(&ordinary).unwrap();
    assert!(json.get("scale_fixed").is_none());
    assert_eq!(
        serde_json::from_value::<Stroke>(json.clone()).unwrap(),
        ordinary
    );
    for (value, expected) in [
        (serde_json::json!(false), StrokeScaleFixed::Disabled),
        (serde_json::json!(0), StrokeScaleFixed::Disabled),
        (serde_json::json!(true), StrokeScaleFixed::Fixed),
        (serde_json::json!(2), StrokeScaleFixed::Threshold(2.)),
        (
            serde_json::json!("zoom-in"),
            StrokeScaleFixed::Threshold(1.),
        ),
    ] {
        json["scale_fixed"] = value;
        assert_eq!(
            serde_json::from_value::<Stroke>(json.clone())
                .unwrap()
                .scale_fixed,
            expected
        );
    }
    for bad in [
        serde_json::json!(-1),
        serde_json::json!("always"),
        serde_json::json!(null),
    ] {
        json["scale_fixed"] = bad;
        assert!(serde_json::from_value::<Stroke>(json.clone()).is_err());
    }
    for value in [-1., 0., f32::NAN, f32::INFINITY] {
        assert!(
            fixed(StrokeScaleFixed::Threshold(value), StrokeAlignment::Center)
                .resolved(StrokeScaleData::IDENTITY, 1.)
                .is_err()
        );
    }
    let scene = scene(StrokeScaleFixed::Fixed, StrokeAlignment::Outside);
    assert_eq!(Scene::from_json(&scene.to_json().unwrap()).unwrap(), scene);
    assert!(scene.resolved_for_view(2.).unwrap().to_json().is_err());
}
#[test]
fn hierarchy_provenance_admits_sheared_world_but_rejects_local_shear_atomically() {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("g", display::Group::new()).with_transform(Affine::scale(-2., 1.)),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            object(StrokeScaleFixed::Fixed, StrokeAlignment::Outside)
                .with_transform(Affine::rotation(45.)),
            Some("g".into()),
            None,
        )
        .unwrap();
    let world = scene.world_transform(&"r".into()).unwrap();
    assert!((world.a * world.c + world.b * world.d).abs() > 1.);
    let scale = scene.stroke_scale(&"r".into()).unwrap();
    assert!((scale.x - 2.).abs() < 1e-6 && (scale.y - 1.).abs() < 1e-6);
    assert!(
        (scene
            .resolved_object(&"r".into(), 1.)
            .unwrap()
            .style
            .stroke
            .unwrap()
            .width
            - 5.)
            .abs()
            < 1e-5
    );
    let before = scene.clone();
    assert!(
        scene
            .update_object(&"g".into(), |o| o.transform =
                Affine::new(1., 0., 0.1, 1., 0., 0.))
            .is_err()
    );
    assert_eq!(scene, before);
    // Invisible/transparent fixed styles still require valid transform provenance.
    scene
        .update_object(&"r".into(), |o| {
            o.visible = false;
            o.style.stroke.as_mut().unwrap().paint = Color::TRANSPARENT.into();
        })
        .unwrap();
    assert!(
        scene
            .update_object(&"g".into(), |o| o.transform =
                Affine::new(1., 0., 0.1, 1., 0., 0.))
            .is_err()
    );
    scene
        .update_object(&"r".into(), |o| {
            o.style.stroke.as_mut().unwrap().scale_fixed = StrokeScaleFixed::Disabled
        })
        .unwrap();
    scene
        .update_object(&"g".into(), |o| {
            o.transform = Affine::new(1., 0., 0.1, 1., 0., 0.)
        })
        .unwrap();
}
#[test]
fn orthogonality_tolerance_and_scale_failures_are_bounded() {
    let epsilon = StrokeScaleData::ORTHOGONAL_TOLERANCE as f32;
    assert!(
        StrokeScaleData::IDENTITY
            .then(Affine::new(1., 0., epsilon * 0.5, 1., 0., 0.))
            .is_ok()
    );
    assert!(
        StrokeScaleData::IDENTITY
            .then(Affine::new(1., 0., epsilon * 2., 1., 0., 0.))
            .is_err()
    );
    for bad in [Affine::scale(0., 1.), Affine::scale(f32::INFINITY, 1.)] {
        assert!(StrokeScaleData::IDENTITY.then(bad).is_err());
    }
    for camera in [0., -1., f32::NAN, f32::INFINITY] {
        assert!(
            scene(StrokeScaleFixed::Fixed, StrokeAlignment::Center)
                .validate_view(camera)
                .is_err()
        );
    }
}
#[test]
fn all_alignments_share_view_bounds_hits_and_mesh_with_intrinsic_empty_bounds() {
    for alignment in [
        StrokeAlignment::Center,
        StrokeAlignment::Inside,
        StrokeAlignment::Outside,
    ] {
        let scene = scene(StrokeScaleFixed::Fixed, alignment);
        for zoom in [0.5, 1., 2.] {
            let resolved = scene.resolved_for_view(zoom).unwrap();
            let o = &resolved.get(&"r".into()).unwrap().object;
            let mesh = o
                .shape
                .stroke_mesh_with_rule(o.style.stroke.as_ref().unwrap(), o.style.fill_rule)
                .unwrap();
            let bounds = scene
                .local_bounds_for_view(&"r".into(), zoom)
                .unwrap()
                .unwrap();
            for p in mesh.triangles().iter().flatten() {
                assert!(bounds.expanded(0.001).contains(*p));
            }
            for p in [
                Point::new(-6., 15.),
                Point::new(-2., 15.),
                Point::new(2., 15.),
                Point::new(6., 15.),
                Point::new(20., 15.),
            ] {
                assert_eq!(
                    scene
                        .hit_test_for_view(p, HitTestOptions::default(), zoom)
                        .unwrap()
                        .is_some(),
                    mesh.hit_test(p, 0.),
                    "{alignment:?} {zoom} {p:?}"
                );
            }
        }
    }
    let mut s = Scene::new();
    s.insert(
        DisplayObject::new("line", display::Line::new(Point::ZERO, Point::new(40., 0.)))
            .with_style(Style {
                fill: None,
                stroke: Some(fixed(StrokeScaleFixed::Fixed, StrokeAlignment::Inside)),
                ..Default::default()
            }),
        None,
        None,
    )
    .unwrap();
    let resolved = s.resolved_for_view(2.).unwrap();
    assert_eq!(
        resolved.local_bounds(&"line".into()).unwrap(),
        Some(Bounds::new(Point::ZERO, Size::new(40., 0.)))
    );
}
#[test]
fn actual_resolved_width_obeys_existing_alignment_precision_limits() {
    let mut s = scene(StrokeScaleFixed::Fixed, StrokeAlignment::Outside);
    s.update_object(&"r".into(), |o| {
        o.style.stroke.as_mut().unwrap().width = MIN_ALIGNED_STROKE_WIDTH
    })
    .unwrap();
    assert!(s.validate_view(1.).is_ok());
    assert!(s.validate_view(2.).is_err());
    assert!(s.validate_view(0.5).is_ok());
    assert_eq!(
        s.get(&"r".into())
            .unwrap()
            .object
            .style
            .stroke
            .as_ref()
            .unwrap()
            .width,
        MIN_ALIGNED_STROKE_WIDTH
    );
}

#[test]
fn derived_scenes_are_camera_bound_immutable_and_not_an_authored_marker_escape() {
    let authored = scene(StrokeScaleFixed::Fixed, StrokeAlignment::Outside);
    let derived = authored.resolved_for_view(2.).unwrap();
    assert_eq!(derived.resolved_stroke_camera(), Some(2.));
    assert_eq!(derived.resolved_for_view(2.).unwrap(), derived);
    assert!(derived.resolved_for_view(1.).is_err());
    assert!(derived.resolved_object(&"r".into(), 1.).is_err());
    assert!(derived.validate_view(1.).is_err());
    let mut clone = derived.clone();
    let mut ran = false;
    assert!(
        clone
            .update_object(&"r".into(), |o| {
                ran = true;
                o.transform = Affine::scale(2., 2.);
            })
            .is_err()
    );
    assert!(!ran);
    assert_eq!(clone, derived);
    assert!(clone.remove(&"r".into()).is_err());
    assert!(
        Scene::new()
            .insert(derived.get(&"r".into()).unwrap().object.clone(), None, None)
            .is_err()
    );
}

#[test]
fn transient_fixed_subtree_keeps_queries_but_cannot_produce_native_paint_items() {
    let mut authored = Scene::new();
    authored
        .insert(
            DisplayObject::new("parent", display::Group::new())
                .with_transform(Affine::scale(2., 2.)),
            None,
            None,
        )
        .unwrap();
    authored
        .insert(
            object(StrokeScaleFixed::Fixed, StrokeAlignment::Outside),
            Some("parent".into()),
            None,
        )
        .unwrap();
    let transfer = authored.transfer_subtree(&"r".into()).unwrap();
    assert_eq!(
        transfer
            .resolved_object(&"r".into(), 1.)
            .unwrap()
            .style
            .stroke
            .unwrap()
            .width,
        5.
    );
    assert_eq!(
        transfer.local_bounds(&"r".into()).unwrap(),
        authored.local_bounds(&"r".into()).unwrap()
    );
    assert!(
        transfer
            .hit_test(Point::new(-2., 15.), HitTestOptions::default())
            .unwrap()
            .is_some()
    );
    assert!(
        matches!(transfer.paint_list(), Err(SceneError::InvalidGeometry(message)) if message.contains("transient subtree"))
    );
    assert!(authored.paint_list().is_ok());
    let resolved = authored.resolved_for_view(2.).unwrap();
    assert!(resolved.paint_list().is_ok());
}

#[test]
fn rotations_do_not_create_a_numeric_threshold_crossing_from_f32_column_roundoff() {
    for angle in [0., 90., 180., -90.] {
        for threshold in [0.5, 1., 1.25, 1.5, 2.] {
            let local = Affine::rotation(angle) * Affine::scale(threshold * 0.5, threshold);
            let scale = StrokeScaleData::IDENTITY.then(local).unwrap();
            let stroke = Stroke::new(Color::BLACK, 8.)
                .with_scale_fixed(StrokeScaleFixed::Threshold(threshold));
            assert_eq!(
                stroke.resolved(scale, 1.).unwrap().width,
                8. / threshold,
                "angle={angle}, threshold={threshold}, scale={scale:?}"
            );
        }
    }
}

#[test]
fn nonunit_rotation_threshold_can_remain_ambiguous_after_f32_magnitude_rounding() {
    let local = Affine::rotation(4.) * Affine::scale(0.5, 1.25);
    let rounded_y = (f64::from(local.c).hypot(f64::from(local.d))) as f32;
    assert_eq!(rounded_y, f32::from_bits(1.25f32.to_bits() + 1));
    let scale = StrokeScaleData::IDENTITY.then(local).unwrap();
    let stroke = Stroke::new(Color::BLACK, 8.).with_scale_fixed(StrokeScaleFixed::Threshold(1.25));
    assert!(
        stroke
            .resolved(scale, 1.)
            .unwrap_err()
            .contains("ambiguous")
    );
    // The continuous true policy has no Y-threshold branch to become ambiguous.
    assert!(
        Stroke::new(Color::BLACK, 8.)
            .with_scale_fixed(StrokeScaleFixed::Fixed)
            .resolved(scale, 1.)
            .is_ok()
    );
}

#[test]
fn numeric_rotation_ambiguity_is_rejected_without_swallowing_exact_one_ulp_crossings() {
    for threshold in [0.5f32, 1., 1.25, 1.5, 2.] {
        let stroke =
            Stroke::new(Color::BLACK, 8.).with_scale_fixed(StrokeScaleFixed::Threshold(threshold));
        for angle in [4., 45., 60.] {
            let scale = StrokeScaleData::IDENTITY
                .then(Affine::rotation(angle) * Affine::scale(threshold * 0.5, threshold))
                .unwrap();
            assert!(
                stroke
                    .resolved(scale, 1.)
                    .unwrap_err()
                    .contains("ambiguous"),
                "{angle} {threshold}"
            );
        }
        for y in [
            f32::from_bits(threshold.to_bits() - 1),
            threshold,
            f32::from_bits(threshold.to_bits() + 1),
        ] {
            for angle in [0., 90.] {
                let scale = StrokeScaleData::IDENTITY
                    .then(Affine::rotation(angle) * Affine::scale(threshold * 0.5, y))
                    .unwrap();
                let expected = if y > threshold {
                    16. / threshold
                } else {
                    8. / threshold
                };
                assert_eq!(
                    stroke.resolved(scale, 1.).unwrap().width,
                    expected,
                    "{angle} {threshold} {y}"
                );
            }
        }
    }
}

#[test]
fn numeric_threshold_scale_precision_and_ambiguity_propagate_through_nested_ancestry() {
    for threshold in [0.5, 1., 1.25, 1.5, 2.] {
        let stroke =
            Stroke::new(Color::BLACK, 8.).with_scale_fixed(StrokeScaleFixed::Threshold(threshold));
        let parent = StrokeScaleData::IDENTITY
            .then(Affine::scale(-2., 4.))
            .unwrap();
        let quarter = parent
            .then(Affine::rotation(90.) * Affine::scale(threshold * 0.25, threshold * 0.25))
            .unwrap();
        assert_eq!(stroke.resolved(quarter, 1.).unwrap().width, 8. / threshold);
        let rotated = parent
            .then(Affine::rotation(60.) * Affine::scale(threshold * 0.25, threshold * 0.25))
            .unwrap();
        assert!(
            stroke
                .resolved(rotated, 1.)
                .unwrap_err()
                .contains("ambiguous")
        );
        assert!(stroke.resolved(rotated, 0.99).is_ok());
        assert!(stroke.resolved(rotated, 1.01).is_ok());
    }
    // Do not round the ancestry product: a real > threshold product of exact
    // f32 local scales still takes the strict high branch, even below one ULP.
    let parent = StrokeScaleData::IDENTITY
        .then(Affine::scale(1., 3.))
        .unwrap();
    let child = parent.then(Affine::scale(0.5, 1f32 / 3.)).unwrap();
    assert!(child.y > 1. && child.y < f32::from_bits(1f32.to_bits() + 1) as f64);
    assert_eq!(
        Stroke::new(Color::BLACK, 8.)
            .with_scale_fixed(StrokeScaleFixed::Threshold(1.))
            .resolved(child, 1.)
            .unwrap()
            .width,
        16.
    );
}

#[test]
fn ambiguous_numeric_rotation_edit_is_atomic_and_fixed_policy_remains_available() {
    let mut scene = scene(StrokeScaleFixed::Threshold(1.25), StrokeAlignment::Outside);
    scene
        .update_object(&"r".into(), |o| o.transform = Affine::scale(0.5, 1.25))
        .unwrap();
    let before = scene.clone();
    assert!(
        scene
            .update_object(&"r".into(), |o| o.transform =
                Affine::rotation(4.) * Affine::scale(0.5, 1.25))
            .is_err()
    );
    assert_eq!(scene, before);
    scene
        .update_object(&"r".into(), |o| {
            o.style.stroke.as_mut().unwrap().scale_fixed = StrokeScaleFixed::Fixed;
            o.transform = Affine::rotation(4.) * Affine::scale(0.5, 1.25);
        })
        .unwrap();
}
