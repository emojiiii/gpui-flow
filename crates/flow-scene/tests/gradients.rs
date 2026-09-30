use gpui_flow_scene::*;
fn p(x: f32, y: f32) -> Point {
    Point::new(x, y)
}
fn stop(offset: f32, color: Color) -> GradientStop {
    GradientStop { offset, color }
}
fn linear() -> Paint {
    Paint::LinearGradient {
        start: p(0.0, 0.0),
        end: p(10.0, 0.0),
        stops: vec![
            stop(0.0, Color::rgb(255, 0, 0)),
            stop(1.0, Color::rgb(0, 0, 255)),
        ],
    }
}
fn rect(w: f32, h: f32) -> [[Point; 3]; 2] {
    [
        [p(0.0, 0.0), p(w, 0.0), p(w, h)],
        [p(0.0, 0.0), p(w, h), p(0.0, h)],
    ]
}
fn close(a: f32, b: f32) {
    assert!((a - b).abs() < 0.002, "{a} != {b}");
}
#[test]
fn linear_stops_interpolate_and_pad_beyond_endpoints() {
    let paint = linear();
    let sampler = PaintSampler::new(&paint).unwrap();
    assert_eq!(
        sampler.sample(p(-5.0, 10.0)).to_color(),
        Color::rgb(255, 0, 0)
    );
    assert_eq!(
        sampler.sample(p(15.0, -5.0)).to_color(),
        Color::rgb(0, 0, 255)
    );
    let middle = sampler.sample(p(5.0, 999.0));
    close(middle.r, 0.5);
    close(middle.b, 0.5);
}
#[test]
fn arbitrary_stop_count_and_duplicate_offsets_make_hard_edges() {
    let paint = Paint::LinearGradient {
        start: p(0.0, 0.0),
        end: p(10.0, 0.0),
        stops: vec![
            stop(0.0, Color::BLACK),
            stop(0.25, Color::rgb(255, 0, 0)),
            stop(0.5, Color::rgb(255, 0, 0)),
            stop(0.5, Color::rgb(0, 0, 255)),
            stop(1.0, Color::WHITE),
        ],
    };
    let sampler = PaintSampler::new(&paint).unwrap();
    assert_eq!(
        sampler.sample(p(4.99, 0.0)).to_color(),
        Color::rgb(255, 0, 0)
    );
    assert_eq!(
        sampler.sample(p(5.0, 0.0)).to_color(),
        Color::rgb(0, 0, 255)
    );
    assert_eq!(
        sampler.sample(p(2.5, 0.0)).to_color(),
        Color::rgb(255, 0, 0)
    );
}
#[test]
fn premultiplied_interpolation_does_not_add_transparent_color_halos() {
    let paint = Paint::LinearGradient {
        start: p(0.0, 0.0),
        end: p(10.0, 0.0),
        stops: vec![
            stop(0.0, Color::rgba(0, 0, 255, 0)),
            stop(1.0, Color::rgb(255, 0, 0)),
        ],
    };
    let sample = PaintSampler::new(&paint).unwrap().sample(p(5.0, 0.0));
    close(sample.r, 1.0);
    close(sample.b, 0.0);
    close(sample.a, 0.5);
}
#[test]
fn radial_gradient_samples_euclidean_radius_in_local_coordinates() {
    let paint = Paint::RadialGradient {
        center: p(5.0, 5.0),
        radius: 10.0,
        stops: vec![stop(0.0, Color::WHITE), stop(1.0, Color::BLACK)],
    };
    let sampler = PaintSampler::new(&paint).unwrap();
    close(sampler.sample(p(8.0, 9.0)).r, 0.5);
    assert_eq!(sampler.sample(p(5.0, 5.0)).to_color(), Color::WHITE);
    assert_eq!(sampler.sample(p(30.0, 5.0)).to_color(), Color::BLACK);
}
#[test]
fn invalid_paint_cannot_create_a_sampler() {
    let paint = Paint::LinearGradient {
        start: Point::ZERO,
        end: Point::ZERO,
        stops: vec![],
    };
    assert!(PaintSampler::new(&paint).is_err());
    let paint = linear();
    assert_eq!(
        PaintSampler::new(&paint).unwrap().sample(p(f32::NAN, 0.0)),
        ColorSample::TRANSPARENT
    );
}
#[test]
fn device_resolution_raster_has_expected_color_channels_and_opacity() {
    let image = rasterize_paint(
        &linear(),
        &rect(10.0, 2.0),
        Affine::IDENTITY,
        RasterOptions {
            scale_factor: 2.0,
            opacity: 0.5,
            ..Default::default()
        },
    )
    .unwrap()
    .unwrap();
    assert_eq!((image.width, image.height), (20, 4));
    let left = image.pixel(0, 1).unwrap();
    assert!(left.r > 240 && left.b < 15);
    assert_eq!(left.a, 128);
    let right = image.pixel(19, 1).unwrap();
    assert!(right.b > 240 && right.r < 15);
    assert_eq!(image.bgra[0], image.pixel(0, 0).unwrap().b);
}
#[test]
fn triangle_union_does_not_double_alpha_on_shared_or_overlapping_edges() {
    let paint = Paint::solid(Color::rgba(255, 0, 0, 128));
    let mut triangles = rect(8.0, 8.0).to_vec();
    triangles.extend(rect(8.0, 8.0));
    let image = rasterize_paint(
        &paint,
        &triangles,
        Affine::IDENTITY,
        RasterOptions::default(),
    )
    .unwrap()
    .unwrap();
    for y in 0..8 {
        for x in 0..8 {
            assert_eq!(image.pixel(x, y).unwrap().a, 128);
        }
    }
}
#[test]
fn opposite_triangle_winding_rasterizes_identically() {
    let first = rect(8.0, 8.0);
    let reversed = first.map(|[a, b, c]| [c, b, a]);
    let a = rasterize_paint(
        &linear(),
        &first,
        Affine::IDENTITY,
        RasterOptions::default(),
    )
    .unwrap();
    let b = rasterize_paint(
        &linear(),
        &reversed,
        Affine::IDENTITY,
        RasterOptions::default(),
    )
    .unwrap();
    assert_eq!(a, b);
}
#[test]
fn affine_gradient_mapping_handles_rotation_and_nonuniform_scale() {
    let transform =
        Affine::translation(40.0, 20.0) * Affine::rotation(90.0) * Affine::scale(2.0, 3.0);
    let triangles = rect(10.0, 5.0).map(|t| t.map(|p| transform.transform_point(p)));
    let image = rasterize_paint(
        &linear(),
        &triangles,
        transform,
        RasterOptions {
            samples_per_axis: 1,
            ..Default::default()
        },
    )
    .unwrap()
    .unwrap();
    let x = image.width / 2;
    let top = image.pixel(x, 1).unwrap();
    let bottom = image.pixel(x, image.height - 2).unwrap();
    assert!(top.r > top.b);
    assert!(bottom.b > bottom.r);
}
#[test]
fn radial_transform_remains_elliptical_in_screen_space() {
    let paint = Paint::RadialGradient {
        center: p(10.0, 10.0),
        radius: 10.0,
        stops: vec![stop(0.0, Color::WHITE), stop(1.0, Color::BLACK)],
    };
    let transform = Affine::scale(2.0, 1.0);
    let triangles = rect(20.0, 20.0).map(|t| t.map(|p| transform.transform_point(p)));
    let image = rasterize_paint(&paint, &triangles, transform, RasterOptions::default())
        .unwrap()
        .unwrap();
    let horizontal = image.pixel(29, 10).unwrap().r as i32;
    let vertical = image.pixel(20, 14).unwrap().r as i32;
    assert!((horizontal - vertical).abs() < 16);
}
#[test]
fn viewport_crop_is_applied_before_allocation_and_at_subpixel_edges() {
    let image = rasterize_paint(
        &linear(),
        &rect(1e6, 1e6),
        Affine::IDENTITY,
        RasterOptions {
            clip_bounds: Some(Bounds::new(p(1.5, 1.5), Size::new(2.0, 2.0))),
            max_pixels: 9,
            ..Default::default()
        },
    )
    .unwrap()
    .unwrap();
    assert_eq!((image.width, image.height), (3, 3));
    assert_eq!(image.bounds.origin, p(1.0, 1.0));
    assert_eq!(image.pixel(0, 0).unwrap().a, 64);
    assert_eq!(image.pixel(1, 1).unwrap().a, 255);
}
#[test]
fn size_budget_refuses_allocation_without_silent_downsampling() {
    let error = rasterize_paint(
        &linear(),
        &rect(1000.0, 1000.0),
        Affine::IDENTITY,
        RasterOptions {
            max_pixels: 100,
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(matches!(
        error,
        GradientError::TooLarge {
            width: 1000,
            height: 1000,
            max_pixels: 100
        }
    ));
}
#[test]
fn singular_transform_and_invalid_options_are_reported() {
    assert!(matches!(
        rasterize_paint(
            &linear(),
            &rect(10.0, 10.0),
            Affine::scale(0.0, 1.0),
            RasterOptions::default()
        ),
        Err(GradientError::NonInvertibleTransform)
    ));
    for options in [
        RasterOptions {
            scale_factor: 0.0,
            ..Default::default()
        },
        RasterOptions {
            samples_per_axis: 3,
            ..Default::default()
        },
        RasterOptions {
            opacity: 2.0,
            ..Default::default()
        },
    ] {
        assert!(matches!(
            rasterize_paint(&linear(), &rect(2.0, 2.0), Affine::IDENTITY, options),
            Err(GradientError::InvalidOptions)
        ));
    }
}
#[test]
fn empty_and_fully_clipped_geometry_needs_no_texture() {
    assert!(
        rasterize_paint(&linear(), &[], Affine::IDENTITY, RasterOptions::default())
            .unwrap()
            .is_none()
    );
    assert!(
        rasterize_paint(
            &linear(),
            &rect(5.0, 5.0),
            Affine::IDENTITY,
            RasterOptions {
                clip_bounds: Some(Bounds::new(p(10.0, 10.0), Size::new(2.0, 2.0))),
                ..Default::default()
            }
        )
        .unwrap()
        .is_none()
    );
}
#[test]
fn fractional_triangle_edges_have_coverage_antialiasing() {
    let image = rasterize_paint(
        &Paint::solid(Color::WHITE),
        &[[p(0.0, 0.0), p(4.0, 0.0), p(0.0, 4.0)]],
        Affine::IDENTITY,
        RasterOptions::default(),
    )
    .unwrap()
    .unwrap();
    let edge = image.pixel(2, 1).unwrap();
    assert!(edge.a > 0 && edge.a < 255);
    assert_eq!(image.pixel(3, 3).unwrap().a, 0);
}

#[test]
fn thin_images_still_respect_texture_axis_limit() {
    assert!(matches!(
        rasterize_paint(
            &linear(),
            &rect(5000.0, 1.0),
            Affine::IDENTITY,
            RasterOptions::default()
        ),
        Err(GradientError::TooLarge { .. })
    ));
}

#[test]
#[ignore = "small manual performance measurement; not a timing-dependent CI assertion"]
fn measure_interactive_radial_raster() {
    let paint = Paint::RadialGradient {
        center: p(250.0, 150.0),
        radius: 250.0,
        stops: vec![
            stop(0.0, Color::WHITE),
            stop(0.4, Color::hex(0x8b5cf6)),
            stop(1.0, Color::hex(0x0f172a)),
        ],
    };
    let started = std::time::Instant::now();
    let image = rasterize_paint(
        &paint,
        &rect(500.0, 300.0),
        Affine::IDENTITY,
        RasterOptions::default(),
    )
    .unwrap()
    .unwrap();
    println!(
        "500×300 radial, {}x{} AA: {:?}, {} bytes",
        RasterOptions::default().samples_per_axis,
        RasterOptions::default().samples_per_axis,
        started.elapsed(),
        image.byte_len()
    );
    assert_eq!(image.byte_len(), 600_000);
}
