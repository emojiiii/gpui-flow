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
fn unsupported_closed_degenerate_and_extreme_geometry_is_rejected_atomically() {
    let bad: Vec<Shape> = vec![
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
fn inactive_center_strokes_keep_legacy_skip_but_extreme_aligned_strokes_are_rejected() {
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

// The region oracle closes every contour for fill only, while the doubled
// centered mesh retains real open caps and dash restarts. Sampling away from
// Boolean-grid boundaries tests the partition without assuming triangulation.
fn assert_open_partition(shape: &Shape, mut base: Stroke, rule: FillRule) {
    base.alignment = StrokeAlignment::Inside;
    let inside = shape.stroke_mesh_with_rule(&base, rule).unwrap();
    base.alignment = StrokeAlignment::Outside;
    let outside = shape.stroke_mesh_with_rule(&base, rule).unwrap();
    base.alignment = StrokeAlignment::Center;
    base.width *= 2.;
    let doubled = shape.stroke_mesh_with_rule(&base, rule).unwrap();
    let region: Shape = display::Custom::new(
        "mask",
        shape.local_bounds().unwrap(),
        shape.flatten(STROKE_GEOMETRY_TOLERANCE),
    )
    .into();
    for y in (-10..55).step_by(2) {
        for x in (-10..65).step_by(2) {
            let p = Point::new(x as f32 + 0.371, y as f32 + 0.239);
            let ink = doubled.hit_test(p, 0.);
            let fill = region.contains_geometry(p, rule);
            assert_eq!(inside.hit_test(p, 0.), ink && fill, "inside {p:?}");
            assert_eq!(outside.hit_test(p, 0.), ink && !fill, "outside {p:?}");
        }
    }
}

#[test]
fn open_l_preserves_all_caps_joins_and_never_strokes_implicit_closure() {
    let shape: Shape = display::Path::from_svg("M0 0H40V40").unwrap().into();
    for cap in [StrokeCap::Butt, StrokeCap::Round, StrokeCap::Square] {
        for join in [StrokeJoin::Bevel, StrokeJoin::Round, StrokeJoin::Miter] {
            for rule in [FillRule::NonZero, FillRule::EvenOdd] {
                let base = stroke(StrokeAlignment::Inside)
                    .with_cap(cap)
                    .with_join(join);
                assert_open_partition(&shape, base.clone(), rule);
                for alignment in [StrokeAlignment::Inside, StrokeAlignment::Outside] {
                    let mesh = shape
                        .stroke_mesh_with_rule(&base.clone().with_alignment(alignment), rule)
                        .unwrap();
                    assert!(
                        !mesh.hit_test(Point::new(20., 20.), 0.),
                        "phantom closing ink"
                    );
                    assert!(!mesh.triangles().is_empty());
                }
            }
        }
    }
}

#[test]
fn open_quadratic_cubic_svg_arc_and_zero_signed_area_bowtie_use_fill_partition() {
    for data in [
        "M0 30Q20 -20 40 30",
        "M0 30C0 -10 40 -10 40 30",
        "M0 20A20 20 0 0 1 40 20",
        "M0 0L40 40L0 40L40 0",
    ] {
        let shape: Shape = display::Path::from_svg(data).unwrap().into();
        for rule in [FillRule::NonZero, FillRule::EvenOdd] {
            assert_open_partition(&shape, stroke(StrokeAlignment::Inside), rule);
        }
        if data.contains('Q') || data.contains('C') || data.contains('A') {
            let y = if data.contains('A') { 20. } else { 30. };
            for alignment in [StrokeAlignment::Inside, StrokeAlignment::Outside] {
                assert!(
                    !shape
                        .stroke_mesh(&stroke(alignment))
                        .unwrap()
                        .hit_test(Point::new(20., y), 0.)
                );
            }
        }
    }
}

#[test]
fn line_and_collinear_open_contours_have_empty_inside_and_full_double_outside() {
    let shapes: Vec<Shape> = vec![
        display::Line::new(Point::ZERO, Point::new(40., 0.)).into(),
        display::Polygon::polyline(vec![Point::ZERO, Point::new(20., 0.), Point::new(40., 0.)])
            .into(),
        display::Path::from_svg("M0 0H40H0H40").unwrap().into(),
        display::Polygon::polyline(vec![Point::ZERO, Point::new(7., 3.), Point::new(21., 9.)])
            .into(),
    ];
    for shape in shapes {
        for cap in [StrokeCap::Butt, StrokeCap::Round, StrokeCap::Square] {
            assert_open_partition(
                &shape,
                stroke(StrokeAlignment::Inside).with_cap(cap),
                FillRule::EvenOdd,
            );
            assert!(
                shape
                    .stroke_mesh(&stroke(StrokeAlignment::Inside).with_cap(cap))
                    .unwrap()
                    .triangles()
                    .is_empty()
            );
        }
    }
    let line: Shape = display::Line::new(Point::ZERO, Point::new(40., 0.)).into();
    let mesh = line
        .stroke_mesh(&stroke(StrokeAlignment::Outside).with_cap(StrokeCap::Butt))
        .unwrap();
    assert_eq!(
        mesh.bounds(),
        Some(Bounds::new(Point::new(0., -4.), Size::new(40., 8.)))
    );
    assert!((area(&mesh) - 320.).abs() < 0.01);
}

#[test]
fn empty_move_only_and_zero_length_open_paths_have_explicit_cap_behavior() {
    for shape in [
        Shape::from(display::Path::default()),
        display::Path::from_svg("M3 4 M9 8").unwrap().into(),
    ] {
        for cap in [StrokeCap::Butt, StrokeCap::Round, StrokeCap::Square] {
            for alignment in [StrokeAlignment::Inside, StrokeAlignment::Outside] {
                assert!(
                    shape
                        .stroke_mesh(&stroke(alignment).with_cap(cap))
                        .unwrap()
                        .triangles()
                        .is_empty()
                );
            }
        }
    }
    for shape in [
        Shape::from(display::Line::new(Point::ZERO, Point::ZERO)),
        display::Path::from_svg("M0 0L0 0L0 0").unwrap().into(),
    ] {
        for cap in [StrokeCap::Butt, StrokeCap::Round, StrokeCap::Square] {
            assert!(
                shape
                    .stroke_mesh(&stroke(StrokeAlignment::Inside).with_cap(cap))
                    .unwrap()
                    .triangles()
                    .is_empty()
            );
            let mesh = shape
                .stroke_mesh(&stroke(StrokeAlignment::Outside).with_cap(cap))
                .unwrap();
            assert_eq!(mesh.triangles().is_empty(), cap == StrokeCap::Butt);
            if cap != StrokeCap::Butt {
                assert_eq!(
                    mesh.bounds(),
                    Some(Bounds::new(Point::new(-4., -4.), Size::new(8., 8.)))
                );
                assert!(mesh.hit_test(Point::ZERO, 0.));
            }
        }
    }
}

#[test]
fn mixed_open_closed_and_line_contours_share_one_fill_rule_region() {
    let same: Shape = display::Path::from_svg("M0 0H40V40H0Z M10 10H30V30H10 M12 20H28")
        .unwrap()
        .into();
    let reversed: Shape = display::Path::from_svg("M0 0V40H40V0Z M10 30H30V10H10 M28 20H12")
        .unwrap()
        .into();
    for rule in [FillRule::NonZero, FillRule::EvenOdd] {
        assert_open_partition(&same, stroke(StrokeAlignment::Inside), rule);
        for alignment in [StrokeAlignment::Inside, StrokeAlignment::Outside] {
            let mesh = same
                .stroke_mesh_with_rule(&stroke(alignment), rule)
                .unwrap();
            let reversed = reversed
                .stroke_mesh_with_rule(&stroke(alignment), rule)
                .unwrap();
            assert!((area(&mesh) - area(&reversed)).abs() < 0.02);
            for p in [
                Point::new(20., 9.),
                Point::new(20., 11.),
                Point::new(20., 20.),
            ] {
                assert_eq!(mesh.hit_test(p, 0.), reversed.hit_test(p, 0.));
            }
            assert_eq!(
                mesh.hit_test(Point::new(20., 20.), 0.),
                (rule == FillRule::NonZero) == (alignment == StrokeAlignment::Inside)
            );
        }
    }
}

#[test]
fn open_primitives_pen_custom_and_absent_fill_keep_shared_mesh_and_serde() {
    let points = vec![Point::ZERO, Point::new(40., 0.), Point::new(40., 40.)];
    let shapes: Vec<Shape> = vec![
        display::Polygon::polyline(points.clone()).into(),
        display::Pen::new()
            .move_to(points[0])
            .line_to(points[1])
            .line_to(points[2])
            .into(),
        display::Ellipse::new(40., 30.).arc(0., 180.).into(),
        display::Custom::new(
            "open",
            Bounds::new(Point::ZERO, Size::new(40., 40.)),
            vec![Contour {
                points,
                closed: false,
            }],
        )
        .into(),
    ];
    for shape in shapes {
        for alignment in [StrokeAlignment::Inside, StrokeAlignment::Outside] {
            let expected = shape.stroke_mesh(&stroke(alignment)).unwrap();
            assert!(!expected.triangles().is_empty());
            let mut scene = Scene::new();
            scene
                .insert(
                    DisplayObject::new("open", shape.clone())
                        .with_style(style(alignment, FillRule::NonZero)),
                    None,
                    None,
                )
                .unwrap();
            for fill in [
                None,
                Some(Color::TRANSPARENT.into()),
                Some(Color::WHITE.into()),
            ] {
                scene
                    .update_object(&"open".into(), |o| o.style.fill = fill)
                    .unwrap();
                let object = &scene.get(&"open".into()).unwrap().object;
                assert_eq!(
                    object
                        .shape
                        .stroke_mesh_with_rule(
                            object.style.stroke.as_ref().unwrap(),
                            object.style.fill_rule
                        )
                        .unwrap(),
                    expected
                );
                assert_eq!(Scene::from_json(&scene.to_json().unwrap()).unwrap(), scene);
            }
        }
    }
}

#[test]
fn open_dashes_keep_odd_negative_phase_and_subpath_restarts() {
    let shape: Shape = display::Path::from_svg("M0 0H40V40 M50 0V40")
        .unwrap()
        .into();
    let base = stroke(StrokeAlignment::Inside)
        .with_cap(StrokeCap::Butt)
        .with_dashes(vec![6., 4., 2.], -7.);
    assert_open_partition(&shape, base, FillRule::EvenOdd);
    let lines: Shape = display::Path::from_svg("M0 0H40 M0 20H40").unwrap().into();
    let odd = stroke(StrokeAlignment::Outside)
        .with_cap(StrokeCap::Butt)
        .with_dashes(vec![6., 4., 2.], -7.);
    let even = odd.clone().with_dashes(vec![6., 4., 2., 6., 4., 2.], 17.);
    let a = lines.stroke_mesh(&odd).unwrap();
    let b = lines.stroke_mesh(&even).unwrap();
    for x in 0..40 {
        let p = Point::new(x as f32 + 0.5, 0.);
        assert_eq!(a.hit_test(p, 0.), b.hit_test(p, 0.));
        assert_eq!(a.hit_test(p, 0.), a.hit_test(p + Point::new(0., 20.), 0.));
    }
}

#[test]
fn open_precision_and_work_limits_reject_atomically_including_implicit_edges() {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("item", display::Rect::new(20., 20.)),
            None,
            None,
        )
        .unwrap();
    let before = scene.clone();
    for data in [
        "M0 0H0.00001",         // A real centerline edge disappears on the grid.
        "M0 0H20L10 0.00001",   // Noncollinear mask must not become empty.
        "M0 0H20V20L0.00001 0", // The implicit closing edge collapses.
        "M0 0H20V20H0 M0.0001 0.0001H19.9999V19.9999H0.0001",
        "M65535 0H65536", // Expanded Outside geometry exceeds the range.
    ] {
        let shape: Shape = display::Path::from_svg(data).unwrap().into();
        let error = scene
            .update_object(&"item".into(), |o| {
                o.shape = shape;
                o.style = style(StrokeAlignment::Outside, FillRule::EvenOdd);
            })
            .unwrap_err();
        assert!(error.to_string().contains("precision"), "{data}: {error}");
        assert_eq!(scene, before);
    }
    for count in [1500, MAX_ALIGNED_STROKE_EDGES + 1] {
        let points = (0..count).map(|i| Point::new(i as f32, 0.)).collect();
        let shape: Shape = display::Polygon::polyline(points).into();
        let error = scene
            .update_object(&"item".into(), |o| {
                o.shape = shape;
                o.style = style(StrokeAlignment::Inside, FillRule::NonZero);
            })
            .unwrap_err();
        assert!(error.to_string().contains("budget"), "{error}");
        assert_eq!(scene, before);
    }
    let shape: Shape = display::Line::new(Point::ZERO, Point::new(100., 0.)).into();
    assert!(
        shape
            .stroke_mesh(&stroke(StrokeAlignment::Outside).with_dashes(vec![0.0001, 0.0001], -1.))
            .unwrap_err()
            .contains("budget")
    );
}

#[test]
fn legitimate_all_gap_dashes_are_empty_but_lost_extrusions_are_rejected() {
    let shape: Shape = display::Line::new(Point::ZERO, Point::new(2., 0.)).into();
    for cap in [StrokeCap::Butt, StrokeCap::Round, StrokeCap::Square] {
        for alignment in [StrokeAlignment::Inside, StrokeAlignment::Outside] {
            let all_gap = stroke(alignment)
                .with_cap(cap)
                .with_dashes(vec![4., 12.], 6.);
            assert!(shape.stroke_mesh(&all_gap).unwrap().triangles().is_empty());
        }
    }
    // Source endpoints and nonzero width are admitted, but Lyon's documented
    // short-edge merge can erase this butt-capped segment before conversion.
    let tiny: Shape = display::Line::new(
        Point::new(60000., 60000.),
        Point::new(60_000.0 + 1.0 / 256.0, 60000.),
    )
    .into();
    for alignment in [StrokeAlignment::Inside, StrokeAlignment::Outside] {
        let stroke = Stroke::new(Color::BLACK, MIN_ALIGNED_STROKE_WIDTH)
            .with_cap(StrokeCap::Butt)
            .with_alignment(alignment);
        assert!(tiny.stroke_mesh(&stroke).unwrap_err().contains("precision"));
        assert!(
            tiny.stroke_mesh(&Stroke {
                width: MIN_ALIGNED_STROKE_WIDTH / 2.,
                ..stroke
            })
            .unwrap_err()
            .contains("precision")
        );
    }
}

#[test]
fn canceled_noncollinear_open_fill_masks_do_not_cancel_stroke_union() {
    let single: Shape = display::Path::from_svg("M0 0H40V40").unwrap().into();
    let duplicate: Shape = display::Path::from_svg("M0 0H40V40 M0 0H40V40")
        .unwrap()
        .into();
    let base = stroke(StrokeAlignment::Outside).with_cap(StrokeCap::Butt);
    let outside = duplicate
        .stroke_mesh_with_rule(&base, FillRule::EvenOdd)
        .unwrap();
    assert!(
        duplicate
            .stroke_mesh_with_rule(
                &base.clone().with_alignment(StrokeAlignment::Inside),
                FillRule::EvenOdd
            )
            .unwrap()
            .triangles()
            .is_empty()
    );
    let doubled = single
        .stroke_mesh(&Stroke {
            width: 8.,
            alignment: StrokeAlignment::Center,
            ..base
        })
        .unwrap();
    assert!((area(&outside) - area(&doubled)).abs() < 0.01);
    for y in -5..45 {
        for x in -5..45 {
            let p = Point::new(x as f32 + 0.37, y as f32 + 0.23);
            assert_eq!(outside.hit_test(p, 0.), doubled.hit_test(p, 0.));
        }
    }
}
