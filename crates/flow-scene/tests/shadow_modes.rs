use gpui_flow_scene::{display, *};

fn modes(spread: f32) -> Shadow {
    Shadow {
        spread,
        r#box: true,
        scale_fixed: true,
        ..Shadow::new(3., -2., 4., Color::BLACK)
    }
}

#[test]
fn shadow_modes_default_and_strict_json_are_compatible() {
    let legacy = r#"{"x":3,"y":-2,"blur":4,"color":{"r":0,"g":0,"b":0,"a":255}}"#;
    let shadow: Shadow = serde_json::from_str(legacy).unwrap();
    assert_eq!(shadow, Shadow::new(3., -2., 4., Color::BLACK));
    let value = serde_json::to_value(modes(-4096.)).unwrap();
    assert_eq!(value["spread"], -4096.);
    assert_eq!(value["box"], true);
    assert_eq!(value["scale_fixed"], true);
    assert_eq!(
        serde_json::from_value::<Shadow>(value.clone()).unwrap(),
        modes(-4096.)
    );
    for (field, invalid) in [
        ("spread", serde_json::json!(true)),
        ("box", serde_json::json!(1)),
        ("box", serde_json::Value::Null),
        ("scale_fixed", serde_json::json!(1)),
        ("scale_fixed", serde_json::json!("zoom-in")),
        ("scaleFixed", serde_json::json!(true)),
        ("blend_mode", serde_json::json!("normal")),
    ] {
        let mut bad = value.clone();
        bad[field] = invalid;
        assert!(serde_json::from_value::<Shadow>(bad).is_err(), "{field}");
    }
}

#[test]
fn every_shadow_slot_validates_spread_atomically_including_hidden_transparent_layers() {
    let mut scene = Scene::new();
    let mut object = DisplayObject::new("r", display::Rect::new(20., 30.));
    object.visible = false;
    scene.insert(object, None, None).unwrap();
    for spread in [-4096., 4096.] {
        scene
            .update_object(&"r".into(), |o| {
                o.style.shadow = Some(modes(spread));
                o.style.shadow_layers = vec![modes(spread)];
                o.style.inner_shadow_layers = vec![modes(spread)];
            })
            .unwrap();
        assert_eq!(Scene::from_json(&scene.to_json().unwrap()).unwrap(), scene);
    }
    let before = scene.clone();
    for spread in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -4097., 4097.] {
        for slot in 0..3 {
            assert!(
                scene
                    .update_object(&"r".into(), |o| {
                        let bad = Shadow {
                            spread,
                            color: Color::TRANSPARENT,
                            ..modes(0.)
                        };
                        match slot {
                            0 => o.style.shadow = Some(bad),
                            1 => o.style.shadow_layers.push(bad),
                            _ => o.style.inner_shadow_layers.push(bad),
                        }
                    })
                    .is_err()
            );
            assert_eq!(scene, before);
        }
    }
    let mut value = serde_json::to_value(&scene).unwrap();
    value["nodes"]["r"]["object"]["style"]["inner_shadow_layers"][0]["spread"] =
        serde_json::json!(4097.);
    assert!(Scene::from_json(&serde_json::to_string(&value).unwrap()).is_err());
}

#[test]
fn fixed_container_reference_ignores_child_overflow_but_group_retains_it() {
    let mut scene = Scene::new();
    scene
        .insert(DisplayObject::new("g", display::Group::new()), None, None)
        .unwrap();
    scene
        .insert(
            DisplayObject::new("b", display::Box::new(20., 10.)).with_style(Style {
                fill: None,
                ..Default::default()
            }),
            Some("g".into()),
            None,
        )
        .unwrap();
    let mut child = DisplayObject::new("ink", display::Rect::new(10., 5.))
        .with_transform(Affine::translation(80., 0.));
    child.visible = false;
    child.style.shadow = Some(Shadow::new(200., 0., 20., Color::BLACK));
    child.style.blur = 10.;
    scene.insert(child, Some("b".into()), None).unwrap();
    let own = Bounds::new(Point::ZERO, Size::new(20., 10.));
    assert_eq!(
        scene.shadow_reference_bounds(&"b".into()).unwrap(),
        Some(own)
    );
    assert_eq!(
        scene.shadow_reference_bounds(&"g".into()).unwrap(),
        Some(Bounds::new(Point::ZERO, Size::new(90., 10.)))
    );
    // The new reference API must not redefine existing subtree geometry.
    assert_eq!(
        scene.local_bounds(&"b".into()).unwrap().unwrap().max_x(),
        90.
    );
    scene
        .update_object(&"b".into(), |o| {
            if let Shape::Box(b) = &mut o.shape {
                b.clip_children = true;
            }
        })
        .unwrap();
    assert_eq!(
        scene.shadow_reference_bounds(&"g".into()).unwrap(),
        Some(own)
    );
}

#[test]
fn group_reference_respects_child_clip_and_excludes_complete_eraser_subtree() {
    let mut scene = Scene::new();
    scene
        .insert(DisplayObject::new("g", display::Group::new()), None, None)
        .unwrap();
    scene
        .insert(
            DisplayObject::new("f", display::Frame::new(30., 20.))
                .with_transform(Affine::translation(10., 5.)),
            Some("g".into()),
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("ink", display::Rect::new(50., 10.))
                .with_transform(Affine::translation(-10., 0.)),
            Some("f".into()),
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("erase", display::Group::new()).with_eraser(EraserMode::Pixel),
            Some("g".into()),
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("distant", display::Rect::new(10., 10.))
                .with_transform(Affine::translation(1000., 1000.)),
            Some("erase".into()),
            None,
        )
        .unwrap();
    assert_eq!(
        scene.shadow_reference_bounds(&"g".into()).unwrap(),
        Some(Bounds::new(Point::new(10., 5.), Size::new(30., 20.)))
    );
    assert_eq!(
        scene
            .shadow_reference_bounds(&"erase".into())
            .unwrap()
            .unwrap()
            .origin,
        Point::new(1000., 1000.)
    );
}

#[test]
fn paintless_frame_reference_survives_clipped_out_ink_and_entering_child_halo() {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("f", display::Frame::new(40., 20.)).with_style(Style {
                fill: None,
                shadow: Some(modes(4.)),
                ..Default::default()
            }),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("r", display::Rect::new(10., 10.))
                .with_transform(Affine::translation(-30., 5.))
                .with_style(Style {
                    shadow: Some(Shadow::new(24., 0., 2., Color::BLACK)),
                    ..Default::default()
                }),
            Some("f".into()),
            None,
        )
        .unwrap();
    assert_eq!(
        scene.shadow_reference_bounds(&"f".into()).unwrap(),
        Some(Bounds::new(Point::ZERO, Size::new(40., 20.)))
    );
}

#[test]
fn shadow_reference_resolves_fixed_strokes_at_logical_zoom() {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("g", display::Group::new()).with_transform(Affine::scale(2., 2.)),
            None,
            None,
        )
        .unwrap();
    let mut style = Style::stroke(Color::BLACK, 8.);
    style.stroke.as_mut().unwrap().scale_fixed = StrokeScaleFixed::Fixed;
    scene
        .insert(
            DisplayObject::new("r", display::Rect::new(20., 10.)).with_style(style),
            Some("g".into()),
            None,
        )
        .unwrap();
    for camera in [0.5, 1., 2.] {
        let expected = scene
            .resolved_object(&"r".into(), camera)
            .unwrap()
            .paint_bounds();
        let leaf = scene
            .shadow_reference_bounds_for_view(&"r".into(), camera)
            .unwrap();
        assert_eq!(leaf, expected);
        assert_eq!(
            scene
                .shadow_reference_bounds_for_view(&"g".into(), camera)
                .unwrap(),
            expected
        );
        assert_eq!(leaf.unwrap().size.width, 20. + 8. / (2. * camera));
    }
    assert!(
        scene
            .shadow_reference_bounds_for_view(&"g".into(), 0.)
            .is_err()
    );
}

#[test]
fn text_reference_uses_alignment_geometry_and_empty_group_has_no_reference() {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("empty", display::Group::new()),
            None,
            None,
        )
        .unwrap();
    assert_eq!(
        scene.shadow_reference_bounds(&"empty".into()).unwrap(),
        None
    );
    let text = display::Text::new("i", Size::new(100., 40.));
    scene
        .insert(DisplayObject::new("text", text), None, None)
        .unwrap();
    assert_eq!(
        scene.shadow_reference_bounds(&"text".into()).unwrap(),
        Some(Bounds::new(Point::ZERO, Size::new(100., 40.)))
    );
}

#[test]
fn world_spread_envelope_contains_signed_projections_at_every_view_orientation_and_cap() {
    let reference = Bounds::new(Point::new(-10., 4.), Size::new(100., 40.));
    let world = Affine::translation(37., -21.) * Affine::rotation(31.) * Affine::scale(2., 2.);
    // An asymmetric descendant halo is deliberately far outside the reference.
    let source = world
        .transform_bounds(reference)
        .union(Bounds::new(Point::new(-130., 140.), Size::new(15., 30.)));
    let center = world.transform_point(reference.center());
    for spread in [-4096., -50., -30., -20., 0.001, 10., 4096.] {
        for scale_fixed in [false, true] {
            let shadow = Shadow {
                spread,
                scale_fixed,
                ..Shadow::new(11., -7., 0., Color::BLACK)
            };
            let envelope = shadow.expand_bounds_with_reference(source, Some(reference), world);
            assert!(envelope.is_valid());
            assert!(envelope.contains_rect(source));
            for camera in [0.5, 1., 2., 10.] {
                let f = if scale_fixed {
                    1. / (2.0_f64 * camera).max(1.)
                } else {
                    1.
                };
                let kx = 1. + 2. * spread as f64 * f / reference.size.width as f64;
                let ky = 1. + 2. * spread as f64 * f / reference.size.height as f64;
                let offset = world.transform_vector(Point::new(shadow.x, shadow.y));
                for angle in [0.0_f64, 0.23, 0.81, 1.61, 2.42, std::f64::consts::PI] {
                    let (s, c) = angle.sin_cos();
                    for (x, y) in [
                        (source.min_x(), source.min_y()),
                        (source.max_x(), source.min_y()),
                        (source.max_x(), source.max_y()),
                        (source.min_x(), source.max_y()),
                    ] {
                        let dx = x as f64 - center.x as f64;
                        let dy = y as f64 - center.y as f64;
                        // Camera rotates into canvas axes, signed spread scales
                        // there, then inverse camera returns to world space.
                        let px = (c * dx - s * dy) * kx;
                        let py = (s * dx + c * dy) * ky;
                        let projected = Point::new(
                            (center.x as f64 + c * px + s * py + offset.x as f64 * f) as f32,
                            (center.y as f64 - s * px + c * py + offset.y as f64 * f) as f32,
                        );
                        assert!(
                            envelope.contains(projected),
                            "spread={spread}, cap={f}, angle={angle}: {projected:?} outside {envelope:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn nonzero_spread_rejects_missing_or_degenerate_required_reference() {
    let source = Bounds::new(Point::ZERO, Size::new(10., 10.));
    let shadow = modes(2.);
    for reference in [
        None,
        Some(Bounds::new(Point::ZERO, Size::new(0., 10.))),
        Some(Bounds::new(Point::ZERO, Size::new(10., 0.))),
    ] {
        assert!(
            !shadow
                .expand_bounds_with_reference(source, reference, Affine::IDENTITY)
                .is_valid()
        );
    }
    assert!(
        !shadow
            .expand_bounds(source, Affine::rotation(45.))
            .is_valid()
    );
    assert!(
        Shadow {
            spread: 0.,
            ..shadow
        }
        .expand_bounds_with_reference(source, None, Affine::IDENTITY)
        .is_valid()
    );
    assert_eq!(
        Shadow {
            color: Color::TRANSPARENT,
            ..shadow
        }
        .expand_bounds_with_reference(source, None, Affine::IDENTITY),
        source
    );
}

#[test]
fn spread_bounds_keep_layer_independence_and_ignore_inner_box_flags() {
    let source = Bounds::new(Point::ZERO, Size::new(100., 40.));
    let first = modes(12.);
    let second = Shadow {
        spread: -30.,
        ..Shadow::new(-100., 5., 1., Color::BLACK)
    };
    let style = Style {
        shadow: Some(first),
        shadow_layers: vec![second],
        inner_shadow_layers: vec![modes(4096.)],
        ..Default::default()
    };
    let expected = first
        .expand_bounds_with_reference(source, Some(source), Affine::IDENTITY)
        .union(second.expand_bounds_with_reference(source, Some(source), Affine::IDENTITY));
    assert_eq!(
        style.expand_effect_bounds_with_reference(source, Some(source), Affine::IDENTITY),
        expected
    );
    let without_knockout = Shadow {
        r#box: false,
        ..first
    };
    assert_eq!(
        first.expand_bounds_with_reference(source, Some(source), Affine::IDENTITY),
        without_knockout.expand_bounds_with_reference(source, Some(source), Affine::IDENTITY)
    );
    let inner_only = Style {
        inner_shadow_layers: style.inner_shadow_layers,
        ..Default::default()
    };
    assert_eq!(
        inner_only.expand_effect_bounds_with_reference(source, None, Affine::IDENTITY),
        source
    );
}

#[test]
fn enormous_spread_projection_propagates_invalid_bounds() {
    let source = Bounds::new(Point::ZERO, Size::new(1e30, 1e30));
    let reference = Bounds::new(Point::ZERO, Size::new(1e-30, 1e-30));
    let style = Style {
        shadow_layers: vec![Shadow::new(0., 0., 0., Color::BLACK), modes(4096.)],
        ..Default::default()
    };
    assert!(
        !style
            .expand_effect_bounds_with_reference(source, Some(reference), Affine::IDENTITY)
            .is_valid()
    );
}

#[test]
fn strict_leafer_export_never_drops_new_shadow_modes() {
    for inner in [false, true] {
        let mut scene = Scene::new();
        let mut object = DisplayObject::new("r", display::Rect::new(20., 20.));
        if inner {
            object.style.inner_shadow_layers.push(modes(2.));
        } else {
            object.style.shadow = Some(modes(2.));
        }
        scene.insert(object, None, None).unwrap();
        assert!(scene.to_leafer_json().is_err());
    }
}
