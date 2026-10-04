use super::*;
use gpui_flow_scene::{
    Affine, BlendMode, Bounds, Color, Contour, DisplayObject, EraserMode, MaskMode, Point, Scene,
    Size, Style, display,
};
use std::sync::{Arc, atomic::AtomicBool};
fn bounds(w: f32, h: f32) -> Bounds {
    Bounds::new(Point::ZERO, Size::new(w, h))
}
fn custom() -> DisplayObject {
    DisplayObject::new(
        "custom",
        display::Custom::new(
            "host",
            bounds(8., 8.),
            vec![Contour {
                closed: true,
                points: vec![
                    Point::ZERO,
                    Point::new(4., 0.),
                    Point::new(4., 8.),
                    Point::new(0., 8.),
                ],
            }],
        ),
    )
}
fn scene() -> Scene {
    let mut s = Scene::new();
    s.insert(
        DisplayObject::new("g", display::Group::new()).with_style(Style {
            blend_mode: BlendMode::Normal,
            ..Default::default()
        }),
        None,
        None,
    )
    .unwrap();
    s
}
fn add(s: &mut Scene, o: DisplayObject) {
    s.insert(o, Some("g".into()), None).unwrap();
}
fn rect(id: &str, color: Color) -> DisplayObject {
    DisplayObject::new(id, display::Rect::new(8., 8.)).with_style(Style::fill(color))
}
fn source(s: &Scene) -> DomainSource {
    capture_subtree(s, &"g".into(), CompositeLimits::default()).unwrap()
}
fn image() -> Arc<CustomImage> {
    let mut rgba = Vec::new();
    for y in 0..8 {
        for x in 0..8 {
            rgba.extend_from_slice(&match (x < 4, y < 4) {
                (true, true) => [128, 0, 0, 128],
                (false, true) => [0; 4],
                (true, false) => [0, 0, 255, 255],
                (false, false) => [255; 4],
            });
        }
    }
    Arc::new(CustomImage {
        image: Arc::new(
            StaticImage::from_premultiplied_rgba(8, 8, rgba, CompositeLimits::default()).unwrap(),
        ),
        local_bounds: bounds(8., 8.),
    })
}
fn inputs(ink: Option<Arc<CustomImage>>) -> CustomInputs {
    let mut i = CustomInputs::default();
    i.insert("custom".into(), Ok(ink));
    i
}
fn viewport() -> RasterViewport {
    RasterViewport {
        world_to_screen: Affine::IDENTITY,
        canvas_bounds: bounds(16., 16.),
        content_mask: bounds(16., 16.),
        device_scale: 1.,
    }
}
fn render(s: &Scene, i: &CustomInputs) -> Result<Option<CpuFrame>, CompositeError> {
    rasterize_with_custom_inputs(
        &source(s),
        &ImageInputs::default(),
        &TextInputs::default(),
        i,
        viewport(),
        &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
        &AtomicBool::new(false),
    )
}
fn at(f: &CpuFrame, x: u32, y: u32) -> [u8; 4] {
    let x = x as f32 - f.screen_bounds.origin.x;
    let y = y as f32 - f.screen_bounds.origin.y;
    if x < 0. || y < 0. || x >= f.width as f32 || y >= f.height as f32 {
        return [0; 4];
    }
    f.premul_rgba[((y as u32 * f.width + x as u32) * 4) as usize..][..4]
        .try_into()
        .unwrap()
}
#[test]
fn custom_pixels_preserve_holes_and_apply_opacity_once_without_contour_fallback() {
    let mut s = scene();
    let mut o = custom();
    o.style.opacity = 0.5;
    add(&mut s, o);
    let f = render(&s, &inputs(Some(image()))).unwrap().unwrap();
    assert_eq!(at(&f, 1, 1), [64, 0, 0, 64]);
    assert_eq!(at(&f, 6, 1), [0; 4]);
    assert_eq!(at(&f, 6, 6), [128; 4]);
    assert!(
        matches!(render(&s,&CustomInputs::default()),Err(CompositeError::CustomBackendRequired(id)) if id=="custom".into())
    );
    assert!(render(&s, &inputs(None)).unwrap().is_none());
}
#[test]
fn custom_real_pixels_drive_blend_alpha_grayscale_self_paint_and_pixel_eraser() {
    for mode in [
        MaskMode::Alpha,
        MaskMode::Grayscale,
        MaskMode::Clipping,
        MaskMode::ClippingPath,
    ] {
        let mut s = scene();
        add(&mut s, custom().with_mask(mode));
        add(&mut s, rect("later", Color::WHITE));
        let f = render(&s, &inputs(Some(image()))).unwrap().unwrap();
        let expected = match mode {
            MaskMode::Alpha => [128; 4],
            MaskMode::Grayscale => [38; 4],
            MaskMode::Clipping => [192, 128, 128, 192],
            MaskMode::ClippingPath => [255; 4],
            _ => unreachable!(),
        };
        assert_eq!(at(&f, 1, 1), expected, "{mode:?}");
        assert_eq!(at(&f, 6, 1), [0; 4], "{mode:?}");
        // ClippingPath's own white pixels outside its contour are still painted.
        if mode == MaskMode::ClippingPath {
            assert_eq!(at(&f, 6, 6), [255; 4]);
        }
    }
    let mut s = scene();
    add(&mut s, rect("before", Color::WHITE));
    add(&mut s, custom().with_eraser(EraserMode::Pixel));
    let f = render(&s, &inputs(Some(image()))).unwrap().unwrap();
    assert_eq!(at(&f, 1, 1), [127; 4]);
    assert_eq!(at(&f, 6, 1), [255; 4]);
    assert_eq!(at(&f, 6, 6), [0; 4]);
    for (blend, expected) in [
        (BlendMode::Multiply, [128, 0, 127, 255]),
        (BlendMode::Screen, [128, 0, 255, 255]),
    ] {
        let mut s = scene();
        add(&mut s, rect("before", Color::rgb(0, 0, 255)));
        let mut c = custom();
        c.style.blend_mode = blend;
        add(&mut s, c);
        let f = render(&s, &inputs(Some(image()))).unwrap().unwrap();
        let expected = if blend == BlendMode::Multiply {
            [0, 0, 127, 255]
        } else {
            expected
        };
        assert_eq!(at(&f, 1, 1), expected);
        assert_eq!(at(&f, 6, 1), [0, 0, 255, 255]);
    }
}
#[test]
fn explicit_no_ink_keeps_mask_markers_and_clipping_path_geometry() {
    for (mask, eraser) in [
        (MaskMode::None, EraserMode::None),
        (MaskMode::None, EraserMode::Pixel),
        (MaskMode::Alpha, EraserMode::None),
        (MaskMode::Grayscale, EraserMode::None),
        (MaskMode::Clipping, EraserMode::None),
        (MaskMode::ClippingPath, EraserMode::None),
    ] {
        let mut s = scene();
        add(&mut s, rect("before", Color::rgb(255, 0, 0)));
        add(&mut s, custom().with_mask(mask).with_eraser(eraser));
        if mask != MaskMode::None {
            add(&mut s, rect("later", Color::rgb(0, 0, 255)));
        }
        let f = render(&s, &inputs(None)).unwrap().unwrap();
        assert_eq!(
            at(&f, 1, 1),
            if mask == MaskMode::ClippingPath {
                [0, 0, 255, 255]
            } else {
                [255, 0, 0, 255]
            },
            "{mask:?}/{eraser:?}"
        );
        assert_eq!(at(&f, 6, 1), [255, 0, 0, 255], "{mask:?}/{eraser:?}");
    }
}
#[test]
fn custom_bounds_and_lowered_limits_reject_atomically_and_capacity_is_retained() {
    let mut s = scene();
    add(&mut s, custom());
    for b in [
        Bounds::new(Point::new(-1., 0.), Size::new(8., 8.)),
        bounds(9., 8.),
        bounds(0., 8.),
        Bounds::new(Point::new(f32::NAN, 0.), Size::new(1., 1.)),
    ] {
        let bad = Arc::new(CustomImage {
            image: image().image.clone(),
            local_bounds: b,
        });
        assert!(matches!(
            render(&s, &inputs(Some(bad))),
            Err(CompositeError::InvalidGeometry(_))
        ));
    }
    let mut spare = Vec::with_capacity(512);
    spare.extend_from_slice(&[255; 4]);
    let cap = spare.capacity() as u64;
    let pixels = Arc::new(
        StaticImage::from_premultiplied_rgba(1, 1, spare, CompositeLimits::default()).unwrap(),
    );
    assert_eq!(pixels.byte_len(), cap);
    let mut customs = inputs(Some(Arc::new(CustomImage {
        image: pixels.clone(),
        local_bounds: bounds(1., 1.),
    })));
    customs.insert(
        "shared".into(),
        Ok(Some(Arc::new(CustomImage {
            image: pixels,
            local_bounds: bounds(1., 1.),
        }))),
    );
    assert_eq!(customs.byte_len(), cap);
    let limit = CompositeLimits {
        max_surface_bytes: cap - 1,
        ..Default::default()
    };
    let mut budget = PixelBudget::new(limit, 7).unwrap();
    assert_eq!(
        rasterize_with_custom_inputs(
            &source(&s),
            &ImageInputs::default(),
            &TextInputs::default(),
            &customs,
            viewport(),
            &mut budget,
            &AtomicBool::new(false)
        )
        .unwrap_err(),
        CompositeError::WorkingSetLimit
    );
    assert_eq!(budget.retained_bytes(), 7);
    let mut spare = Vec::with_capacity(100);
    spare.extend_from_slice(&[255; 4]);
    assert!(matches!(
        StaticImage::from_premultiplied_rgba(
            1,
            1,
            spare,
            CompositeLimits {
                max_surface_bytes: 4,
                ..Default::default()
            }
        ),
        Err(CompositeError::WorkingSetLimit)
    ));
    for rgba in [vec![255, 0, 0, 128], vec![0; 3]] {
        assert!(matches!(
            StaticImage::from_premultiplied_rgba(1, 1, rgba, CompositeLimits::default()),
            Err(CompositeError::InvalidImage)
        ));
    }
    let mut budget = PixelBudget::new(
        CompositeLimits {
            max_dimension: 4,
            ..Default::default()
        },
        0,
    )
    .unwrap();
    assert_eq!(
        rasterize_with_custom_inputs(
            &source(&s),
            &ImageInputs::default(),
            &TextInputs::default(),
            &inputs(Some(image())),
            viewport(),
            &mut budget,
            &AtomicBool::new(false)
        )
        .unwrap_err(),
        CompositeError::TooLarge
    );
}
#[test]
fn custom_uses_affine_ancestor_clip_and_node_grayscale() {
    let mut s = scene();
    let mut c = custom();
    c.transform = Affine::new(0., 1., -1., 0., 8., 0.);
    c.style.grayscale = 1.;
    add(&mut s, c);
    let mut src = source(&s);
    src.clips.push(gpui_flow_scene::ClipRegion {
        local_bounds: bounds(6., 6.),
        corner_radius: 1.,
        corner_radii: None,
        world_transform: Affine::IDENTITY,
    });
    let f = rasterize_with_custom_inputs(
        &src,
        &ImageInputs::default(),
        &TextInputs::default(),
        &inputs(Some(image())),
        viewport(),
        &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap();
    // Node grayscale uses Rec.709 encoded-byte weights (distinct from mask luminance):
    // round(128 * .2126) = 27; round(255 * .0722) = 18.
    assert_eq!(at(&f, 5, 1), [27, 27, 27, 128]);
    assert_eq!(at(&f, 7, 1), [0; 4]);
    assert_eq!(at(&f, 1, 1), [18, 18, 18, 255]);
}
#[test]
fn all_input_classes_and_retained_outputs_share_custom_budget() {
    let mut s = scene();
    add(&mut s, custom());
    let ink = image();
    let customs = inputs(Some(ink.clone()));
    let mut images = ImageInputs::default();
    images.insert("image".into(), Ok(ink.image.clone()));
    let mut texts = TextInputs::default();
    texts.insert(
        "text".into(),
        Ok(Some(Arc::new(TextImage {
            image: ink.image.clone(),
            local_bounds: bounds(8., 8.),
        }))),
    );
    let bytes = images.byte_len() + texts.byte_len() + customs.byte_len();
    let mut budget = PixelBudget::new(
        CompositeLimits {
            max_surface_bytes: bytes + 16,
            ..Default::default()
        },
        17,
    )
    .unwrap();
    assert_eq!(
        rasterize_with_custom_inputs(
            &source(&s),
            &images,
            &texts,
            &customs,
            viewport(),
            &mut budget,
            &AtomicBool::new(false)
        )
        .unwrap_err(),
        CompositeError::WorkingSetLimit
    );
    assert_eq!(budget.retained_bytes(), 17);
}

#[test]
fn custom_capture_charges_contours_even_with_explicit_pixel_capability() {
    for mask in [MaskMode::None, MaskMode::ClippingPath] {
        let mut s = scene();
        add(&mut s, custom().with_mask(mask));
        for limits in [
            CompositeLimits {
                max_points: 3,
                ..Default::default()
            },
            CompositeLimits {
                max_total_points: 3,
                ..Default::default()
            },
        ] {
            assert_eq!(
                capture_subtree(&s, &"g".into(), limits).unwrap_err(),
                CompositeError::InputLimit
            );
        }
        assert!(
            capture_subtree(
                &s,
                &"g".into(),
                CompositeLimits {
                    max_points: 4,
                    max_total_points: 4,
                    ..Default::default()
                }
            )
            .is_ok()
        );
    }
}

#[test]
fn custom_bounds_containment_does_not_round_or_overflow_f32_endpoints() {
    let pixels = image().image.clone();
    for (origin, declared_width, actual_width) in
        [(f32::MAX, f32::MAX / 4., f32::MAX / 2.), (1.0e12, 1., 2.)]
    {
        let declared = Bounds::new(Point::new(origin, 0.), Size::new(declared_width, 1.));
        let ink = CustomImage {
            image: pixels.clone(),
            local_bounds: Bounds::new(Point::new(origin, 0.), Size::new(actual_width, 1.)),
        };
        assert!(matches!(
            ink.validate(declared, CompositeLimits::default()),
            Err(CompositeError::InvalidGeometry(_))
        ));
    }
    let edge = CustomImage {
        image: pixels,
        local_bounds: Bounds::new(Point::new(1.0e20, 0.), Size::new(1., 1.)),
    };
    assert!(matches!(
        edge.validate(bounds(1.0e20, 1.), CompositeLimits::default()),
        Err(CompositeError::InvalidGeometry(_))
    ));
}
