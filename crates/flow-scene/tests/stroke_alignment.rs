use gpui_flow_scene::{display, *};

fn stroke(alignment: StrokeAlignment) -> Stroke {
    Stroke::new(Color::BLACK, 4.)
        .with_join(StrokeJoin::Miter)
        .with_alignment(alignment)
}
fn style(alignment: StrokeAlignment, fill_rule: FillRule) -> Style {
    Style {
        fill: None,
        stroke: Some(stroke(alignment)),
        fill_rule,
        ..Default::default()
    }
}
fn area(mesh: &StrokeMesh) -> f64 {
    mesh.triangles()
        .iter()
        .map(|[a, b, c]| {
            ((b.x as f64 - a.x as f64) * (c.y as f64 - a.y as f64)
                - (b.y as f64 - a.y as f64) * (c.x as f64 - a.x as f64))
                .abs()
                * 0.5
        })
        .sum()
}
#[test]
fn rectangle_alignment_has_exact_area_bounds_and_painted_hits() {
    let shape: Shape = display::Rect::new(20., 20.).into();
    for (alignment, expected_area, lo, hi) in [
        (StrokeAlignment::Inside, 256., 0., 20.),
        (StrokeAlignment::Center, 320., -2., 22.),
        (StrokeAlignment::Outside, 384., -4., 24.),
    ] {
        let mesh = shape
            .stroke_mesh_with_rule(&stroke(alignment), FillRule::NonZero)
            .unwrap();
        assert!(
            (area(&mesh) - expected_area).abs() < 0.01,
            "{alignment:?}: {}",
            area(&mesh)
        );
        assert_eq!(
            mesh.bounds(),
            Some(Bounds::new(Point::new(lo, lo), Size::new(hi - lo, hi - lo)))
        );
        let object = DisplayObject::new("rect", shape.clone())
            .with_style(style(alignment, FillRule::NonZero));
        if alignment != StrokeAlignment::Center {
            assert_eq!(object.paint_bounds(), mesh.bounds());
        }
        for at in [
            Point::new(-3., 10.),
            Point::new(-1., 10.),
            Point::new(1., 10.),
            Point::new(3., 10.),
            Point::new(10., 10.),
        ] {
            assert_eq!(shape.hit_test(at, &object.style, 0.), mesh.hit_test(at, 0.));
        }
    }
}
#[test]
fn fill_paint_presence_does_not_change_alignment_and_json_preserves_it() {
    let mut scene = Scene::new();
    let object = DisplayObject::new("rect", display::Rect::new(20., 20.))
        .with_style(style(StrokeAlignment::Outside, FillRule::EvenOdd));
    let expected = object.paint_bounds();
    scene.insert(object, None, None).unwrap();
    for fill in [
        None,
        Some(Color::TRANSPARENT.into()),
        Some(Color::WHITE.into()),
    ] {
        scene
            .update_object(&"rect".into(), |o| o.style.fill = fill)
            .unwrap();
        assert_eq!(
            scene.get(&"rect".into()).unwrap().object.paint_bounds(),
            expected
        );
        assert_eq!(Scene::from_json(&scene.to_json().unwrap()).unwrap(), scene);
    }
    let mut json = serde_json::to_value(stroke(StrokeAlignment::Outside)).unwrap();
    json.as_object_mut().unwrap().remove("alignment");
    assert_eq!(
        serde_json::from_value::<Stroke>(json).unwrap().alignment,
        StrokeAlignment::Center
    );
}
#[test]
fn holes_follow_fill_rule_and_reversed_winding_without_triangle_parity_gaps() {
    let same: Shape = display::Path::from_svg("M0 0H40V40H0Z M10 10H30V30H10Z")
        .unwrap()
        .into();
    let opposite: Shape = display::Path::from_svg("M0 0H40V40H0Z M10 10V30H30V10Z")
        .unwrap()
        .into();
    let reversed: Shape = display::Path::from_svg("M0 0V40H40V0Z M10 10H30V30H10Z")
        .unwrap()
        .into();
    for alignment in [StrokeAlignment::Inside, StrokeAlignment::Outside] {
        let a = same
            .stroke_mesh_with_rule(&stroke(alignment), FillRule::EvenOdd)
            .unwrap();
        let b = opposite
            .stroke_mesh_with_rule(&stroke(alignment), FillRule::NonZero)
            .unwrap();
        let c = reversed
            .stroke_mesh_with_rule(&stroke(alignment), FillRule::NonZero)
            .unwrap();
        assert!((area(&a) - area(&b)).abs() < 0.02);
        assert!((area(&a) - area(&c)).abs() < 0.02);
        for p in [
            Point::new(9., 20.),
            Point::new(11., 20.),
            Point::new(20., 20.),
            Point::new(-2., 20.),
            Point::new(2., 20.),
        ] {
            assert_eq!(a.hit_test(p, 0.), b.hit_test(p, 0.));
            assert_eq!(a.hit_test(p, 0.), c.hit_test(p, 0.));
        }
    }
    let inside = same
        .stroke_mesh_with_rule(&stroke(StrokeAlignment::Inside), FillRule::NonZero)
        .unwrap();
    let outside = same
        .stroke_mesh_with_rule(&stroke(StrokeAlignment::Outside), FillRule::NonZero)
        .unwrap();
    assert!(inside.hit_test(Point::new(11., 20.), 0.));
    assert!(inside.hit_test(Point::new(9., 20.), 0.));
    assert!(!outside.hit_test(Point::new(11., 20.), 0.));
    assert!(!outside.hit_test(Point::new(9., 20.), 0.));
}
#[test]
fn concave_curved_arc_and_self_intersecting_closed_geometry_share_partition() {
    let shapes: Vec<Shape> = vec![
        display::Star::new(5, 20., 9.).into(),
        display::Rect::new(40., 30.).rounded(8.).into(),
        display::Path::from_svg("M40 20 A20 20 0 1 1 0 20 A20 20 0 1 1 40 20Z")
            .unwrap()
            .into(),
        display::Path::from_svg("M0 0L40 40L0 40L40 0Z")
            .unwrap()
            .into(),
        display::Path::from_svg("M0 0H40V10H10V40H0Z")
            .unwrap()
            .into(),
    ];
    for shape in shapes {
        for rule in [FillRule::NonZero, FillRule::EvenOdd] {
            // Aligned strokes use the canonical 0.1-unit contour approximation,
            // while analytic rounded primitive picking remains exact.
            let region: Shape = display::Custom::new(
                "region",
                shape.local_bounds().unwrap(),
                shape.flatten(STROKE_GEOMETRY_TOLERANCE),
            )
            .into();
            let inside = shape
                .stroke_mesh_with_rule(&stroke(StrokeAlignment::Inside), rule)
                .unwrap();
            let outside = shape
                .stroke_mesh_with_rule(&stroke(StrokeAlignment::Outside), rule)
                .unwrap();
            assert!(!inside.triangles().is_empty());
            assert!(!outside.triangles().is_empty());
            for y in -24..45 {
                for x in -24..45 {
                    let p = Point::new(x as f32 + 0.37, y as f32 + 0.23);
                    let i = inside.hit_test(p, 0.);
                    let o = outside.hit_test(p, 0.);
                    assert!(!(i && o), "overlap at {p:?}");
                    if i {
                        assert!(region.contains_geometry(p, rule), "inside at {p:?}");
                    }
                    if o {
                        assert!(!region.contains_geometry(p, rule), "outside at {p:?}");
                    }
                }
            }
        }
    }
}
#[test]
fn dashed_compound_contours_keep_phase_caps_and_gaps_after_alignment() {
    let shape: Shape = display::Path::from_svg("M0 0H20V20H0Z M40 0H60V20H40Z")
        .unwrap()
        .into();
    for alignment in [StrokeAlignment::Inside, StrokeAlignment::Outside] {
        let stroke = stroke(alignment)
            .with_cap(StrokeCap::Butt)
            .with_dashes(vec![6., 4.], 0.);
        let mesh = shape.stroke_mesh(&stroke).unwrap();
        let y = if alignment == StrokeAlignment::Inside {
            1.
        } else {
            -1.
        };
        for x in [0., 40.] {
            assert!(mesh.hit_test(Point::new(x + 3., y), 0.));
            assert!(!mesh.hit_test(Point::new(x + 8., y), 0.));
        }
    }
}
#[test]
fn unsupported_open_degenerate_and_extreme_geometry_is_rejected_atomically() {
    let bad: Vec<Shape> = vec![
        display::Line::new(Point::ZERO, Point::new(20., 0.)).into(),
        display::Path::from_svg("M0 0H20V20").unwrap().into(),
        display::Path::from_svg("M0 0H20V20Z M30 30H40")
            .unwrap()
            .into(),
        display::Path::from_svg("M0 0H20Z").unwrap().into(),
        display::Path::from_svg("M0 0H20H40Z").unwrap().into(),
        display::Rect::new(100000., 20.).into(),
        display::Text::new("no", Size::new(20., 20.)).into(),
        display::Image::new("a.png", Size::new(20., 20.)).into(),
        display::Group::new().into(),
    ];
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("item", display::Rect::new(20., 20.)),
            None,
            None,
        )
        .unwrap();
    let before = scene.clone();
    for shape in bad {
        for alignment in [StrokeAlignment::Inside, StrokeAlignment::Outside] {
            assert!(
                scene
                    .update_object(&"item".into(), |o| {
                        o.shape = shape.clone();
                        o.style = style(alignment, FillRule::NonZero);
                    })
                    .is_err(),
                "{shape:?}"
            );
            assert_eq!(scene, before);
        }
    }
    let shape: Shape = display::Rect::new(20., 20.).into();
    assert!(
        shape
            .stroke_mesh(
                &Stroke::new(Color::BLACK, MIN_ALIGNED_STROKE_WIDTH / 2.)
                    .with_alignment(StrokeAlignment::Inside)
            )
            .is_err()
    );
}
#[test]
fn boolean_work_is_bounded_before_intersection_or_output_growth() {
    let shape: Shape = display::Star::new(1024, 20., 9.).into();
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("item", display::Rect::new(20., 20.)),
            None,
            None,
        )
        .unwrap();
    let before = scene.clone();
    let error = scene
        .update_object(&"item".into(), |o| {
            o.shape = shape;
            o.style = style(StrokeAlignment::Outside, FillRule::EvenOdd);
        })
        .unwrap_err();
    assert!(error.to_string().contains("budget"), "{error}");
    assert_eq!(scene, before);
    let shape: Shape = display::Rect::new(100., 100.).into();
    let dashed = stroke(StrokeAlignment::Outside).with_dashes(vec![0.5, 0.5], 0.);
    assert!(shape.stroke_mesh(&dashed).unwrap_err().contains("budget"));
}

#[test]
fn tiny_holes_narrow_gaps_and_collapsed_edges_are_rejected_without_mutation() {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("item", display::Rect::new(20., 20.)),
            None,
            None,
        )
        .unwrap();
    let before = scene.clone();
    for path in [
        "M0 0H20V20H0Z M10 10h0.00001v0.00001h-0.00001Z",
        "M0 0H20V20H0Z M0.0001 0.0001H19.9999V19.9999H0.0001Z",
        "M0 0H20V20H0.00001L0 20Z",
    ] {
        let shape: Shape = display::Path::from_svg(path).unwrap().into();
        let error = scene
            .update_object(&"item".into(), |o| {
                o.shape = shape;
                o.style = style(StrokeAlignment::Inside, FillRule::EvenOdd);
            })
            .unwrap_err();
        assert!(error.to_string().contains("precision"), "{error}");
        assert_eq!(scene, before);
    }
}
#[test]
fn supported_large_coordinates_remain_finite_and_paint_the_correct_side() {
    let shape: Shape = display::Path::from_svg("M60000 60000h20v20h-20Z")
        .unwrap()
        .into();
    for alignment in [StrokeAlignment::Inside, StrokeAlignment::Outside] {
        let mesh = shape.stroke_mesh(&stroke(alignment)).unwrap();
        assert!(mesh.triangles().iter().flatten().all(|p| p.is_finite()));
        assert_eq!(
            mesh.hit_test(Point::new(60002., 60010.), 0.),
            alignment == StrokeAlignment::Inside
        );
        assert_eq!(
            mesh.hit_test(Point::new(59998., 60010.), 0.),
            alignment == StrokeAlignment::Outside
        );
    }
}

#[test]
fn closed_primitives_pen_and_custom_fallback_admit_aligned_strokes() {
    let shapes: Vec<Shape> = vec![
        display::Ellipse::new(30., 20.).into(),
        display::Box::new(30., 20.).rounded(4.).into(),
        display::Frame::new(30., 20.).rounded(4.).into(),
        display::Pen::new()
            .move_to(Point::ZERO)
            .line_to(Point::new(20., 0.))
            .line_to(Point::new(20., 20.))
            .line_to(Point::new(0., 20.))
            .close()
            .into(),
        display::Custom::new(
            "closed",
            Bounds::new(Point::ZERO, Size::new(20., 20.)),
            vec![Contour {
                points: vec![
                    Point::ZERO,
                    Point::new(20., 0.),
                    Point::new(20., 20.),
                    Point::new(0., 20.),
                ],
                closed: true,
            }],
        )
        .into(),
    ];
    for shape in shapes {
        for alignment in [StrokeAlignment::Inside, StrokeAlignment::Outside] {
            let mesh = shape.stroke_mesh(&stroke(alignment)).unwrap();
            assert!(area(&mesh) > 0.);
            assert!(mesh.bounds().unwrap().is_valid());
        }
    }
}

#[test]
fn inactive_center_strokes_keep_legacy_skip_but_open_aligned_strokes_are_rejected() {
    let shape: Shape = display::Pen::new()
        .move_to(Point::ZERO)
        .cubic_to(
            Point::new(1e12, 1e12),
            Point::new(-1e12, 1e12),
            Point::new(10., 0.),
        )
        .into();
    for stroke in [
        Stroke::new(Color::BLACK, 0.),
        Stroke::new(Color::TRANSPARENT, 4.),
    ] {
        assert!(shape.stroke_mesh(&stroke).unwrap().triangles().is_empty());
        assert!(
            shape
                .stroke_mesh(&stroke.with_alignment(StrokeAlignment::Inside))
                .is_err()
        );
    }
}
