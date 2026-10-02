use super::*;
use gpui_flow_scene::{
    Affine, BlendMode, Bounds, Color, DisplayObject, Point, Scene, Size, Style, display,
};
use std::sync::{Arc, atomic::AtomicBool};
fn rect(w: f32, h: f32) -> Bounds {
    Bounds::new(Point::ZERO, Size::new(w, h))
}
fn image(fit: display::ImageFit) -> display::Image {
    let mut i = display::Image::new("pixels", Size::new(40., 40.)).with_corners([20., 0., 15., 0.]);
    i.fit = fit;
    i.natural_size = Some(Size::new(80., 40.));
    i
}
fn inputs() -> ImageInputs {
    let mut i = ImageInputs::default();
    i.insert(
        "i".into(),
        Ok(Arc::new(
            StaticImage::from_premultiplied_rgba(
                80,
                40,
                [255, 0, 0, 255].repeat(80 * 40),
                CompositeLimits::default(),
            )
            .unwrap(),
        )),
    );
    i
}
fn scene(fit: display::ImageFit, parent: Option<bool>, transform: Affine) -> Scene {
    let mut s = Scene::new();
    if let Some(internal) = parent {
        s.insert(
            DisplayObject::new(
                "p",
                display::Frame::new(40., 40.).with_corners([0., 20., 0., 0.]),
            )
            .with_transform(transform)
            .with_style(Style {
                fill: None,
                blend_mode: if internal {
                    BlendMode::Normal
                } else {
                    BlendMode::PassThrough
                },
                ..Default::default()
            }),
            None,
            None,
        )
        .unwrap();
    }
    s.insert(
        DisplayObject::new("i", image(fit))
            .with_transform(if parent.is_some() {
                Affine::IDENTITY
            } else {
                transform
            })
            .with_style(Style {
                blend_mode: if parent == Some(true) {
                    BlendMode::PassThrough
                } else {
                    BlendMode::Normal
                },
                ..Default::default()
            }),
        parent.map(|_| "p".into()),
        None,
    )
    .unwrap();
    s
}
fn source(s: &Scene) -> DomainSource {
    scene_units(s, CompositeLimits::default())
        .unwrap()
        .into_iter()
        .find_map(|u| {
            if let SceneUnit::Domain(d) = u {
                Some(*d)
            } else {
                None
            }
        })
        .unwrap()
}
fn render(
    s: &Scene,
    limits: CompositeLimits,
) -> (Result<Option<CpuFrame>, CompositeError>, PixelBudget) {
    let mut b = PixelBudget::new(limits, 0).unwrap();
    let r = rasterize_with_images(
        &source(s),
        &inputs(),
        RasterViewport {
            world_to_screen: Affine::IDENTITY,
            canvas_bounds: rect(120., 100.),
            content_mask: rect(120., 100.),
            device_scale: 1.,
        },
        &mut b,
        &AtomicBool::new(false),
    );
    (r, b)
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
fn image_corners_preserve_real_fit_letterboxes_in_all_three_modes() {
    for fit in [
        display::ImageFit::Contain,
        display::ImageFit::Cover,
        display::ImageFit::Fill,
    ] {
        let f = render(
            &scene(fit, None, Affine::IDENTITY),
            CompositeLimits::default(),
        )
        .0
        .unwrap()
        .unwrap();
        assert_eq!(at(&f, 0, 11), [0; 4]);
        assert_eq!(at(&f, 10, 11), [255, 0, 0, 255]);
        assert_eq!(at(&f, 39, 11), [255, 0, 0, 255]);
        assert_eq!(
            at(&f, 20, 2),
            if fit == display::ImageFit::Contain {
                [0; 4]
            } else {
                [255, 0, 0, 255]
            }
        );
    }
}
#[test]
fn image_own_and_ancestor_corners_intersect_equally_inside_or_outside_domain() {
    let a = render(
        &scene(display::ImageFit::Fill, Some(false), Affine::IDENTITY),
        CompositeLimits::default(),
    )
    .0
    .unwrap()
    .unwrap();
    let b = render(
        &scene(display::ImageFit::Fill, Some(true), Affine::IDENTITY),
        CompositeLimits::default(),
    )
    .0
    .unwrap()
    .unwrap();
    for y in 0..45 {
        for x in 0..45 {
            assert_eq!(at(&a, x, y), at(&b, x, y), "{x},{y}");
        }
    }
    assert_eq!(at(&a, 1, 1), [0; 4]);
    assert_eq!(at(&a, 39, 1), [0; 4]);
    assert_eq!(at(&a, 1, 39), [255, 0, 0, 255]);
}
#[test]
fn image_corner_masks_follow_parent_affine_once() {
    let m = Affine::translation(40., 15.) * Affine::rotation(25.) * Affine::scale(1.3, 0.8);
    let f = render(
        &scene(display::ImageFit::Fill, Some(true), m),
        CompositeLimits::default(),
    )
    .0
    .unwrap()
    .unwrap();
    let p = m.transform_point(Point::new(20., 20.));
    assert_eq!(
        at(&f, p.x.floor() as u32, p.y.floor() as u32),
        [255, 0, 0, 255]
    );
    let p = m.transform_point(Point::new(1., 1.));
    assert_eq!(at(&f, p.x.floor() as u32, p.y.floor() as u32), [0; 4]);
}
#[test]
fn image_mask_ledger_admits_exact_work_and_peak_before_painting() {
    for internal in [false, true] {
        let s = scene(display::ImageFit::Fill, Some(internal), Affine::IDENTITY);
        let (r, b) = render(&s, CompositeLimits::default());
        r.unwrap().unwrap();
        let bytes = b.peak_bytes();
        let writes = b.surface_writes();
        for limits in [
            CompositeLimits {
                max_surface_bytes: bytes - 1,
                ..Default::default()
            },
            CompositeLimits {
                max_surface_writes: writes - 1,
                ..Default::default()
            },
        ] {
            let (r, b) = render(&s, limits);
            assert!(matches!(
                r,
                Err(CompositeError::WorkingSetLimit | CompositeError::WorkLimit)
            ));
            assert_eq!(
                b.surface_writes(),
                0,
                "internal={internal}, limits={limits:?}, peak={bytes}, writes={writes}, result={r:?}"
            );
            assert_eq!(b.retained_bytes(), 0);
        }
    }
}
#[test]
fn default_native_fast_path_and_geometry_domains_are_selected_deliberately() {
    for fit in [
        display::ImageFit::Contain,
        display::ImageFit::Cover,
        display::ImageFit::Fill,
    ] {
        for rounded in [false, true] {
            let mut s = Scene::new();
            let mut i = image(fit);
            if !rounded {
                i = i.rounded(0.);
            }
            s.insert(DisplayObject::new("i", i), None, None).unwrap();
            let u = scene_units(&s, CompositeLimits::default()).unwrap();
            assert_eq!(
                matches!(u[0], SceneUnit::Domain(_)),
                rounded && fit == display::ImageFit::Contain
            );
        }
    }
    for m in [
        Affine::rotation(30.),
        Affine::scale(-1., 1.),
        Affine::scale(2., 1.),
        Affine::skew(10., 0.),
    ] {
        let mut s = Scene::new();
        s.insert(
            DisplayObject::new("i", image(display::ImageFit::Fill).rounded(0.)).with_transform(m),
            None,
            None,
        )
        .unwrap();
        assert!(matches!(
            scene_units(&s, CompositeLimits::default()).unwrap()[0],
            SceneUnit::Domain(_)
        ));
    }
}
#[test]
fn rounded_ancestor_selects_image_and_text_domains_without_native_aabb_fallback() {
    let mut s = Scene::new();
    s.insert(
        DisplayObject::new("p", display::Frame::new(40., 40.).rounded(10.)).with_style(Style {
            fill: None,
            ..Default::default()
        }),
        None,
        None,
    )
    .unwrap();
    s.insert(
        DisplayObject::new("i", image(display::ImageFit::Fill).rounded(0.)),
        Some("p".into()),
        None,
    )
    .unwrap();
    s.insert(
        DisplayObject::new("t", display::Text::new("text", Size::new(20., 20.))),
        Some("p".into()),
        None,
    )
    .unwrap();
    let u = scene_units(&s, CompositeLimits::default()).unwrap();
    assert!(
        u.iter()
            .any(|u| matches!(u,SceneUnit::Domain(d)if d.root_id().as_str()=="i"))
    );
    assert!(
        u.iter()
            .any(|u| matches!(u,SceneUnit::Domain(d)if d.root_id().as_str()=="t"))
    );
}
#[test]
fn own_corner_shadow_uses_real_fit_alpha_and_missing_pixels_fail_atomically() {
    let mut s = scene(display::ImageFit::Contain, None, Affine::IDENTITY);
    s.update_object(&"i".into(), |o| {
        o.style.shadow = Some(gpui_flow_scene::Shadow::new(0., 0., 0., Color::BLACK))
    })
    .unwrap();
    let f = render(&s, CompositeLimits::default()).0.unwrap().unwrap();
    assert_eq!(at(&f, 0, 11), [0; 4]);
    assert_eq!(at(&f, 20, 2), [0; 4]);
    let mut budget = PixelBudget::new(CompositeLimits::default(), 0).unwrap();
    assert!(matches!(
        rasterize(
            &source(&s),
            RasterViewport {
                world_to_screen: Affine::IDENTITY,
                canvas_bounds: rect(120., 100.),
                content_mask: rect(120., 100.),
                device_scale: 1.
            },
            &mut budget,
            &AtomicBool::new(false)
        ),
        Err(CompositeError::ImagePending(_))
    ));
    assert_eq!(budget.surface_writes(), 0);
}

#[test]
fn source_crop_maps_real_decoded_pixels_through_fit_rounding_affine_and_ancestor_clip() {
    let mut inputs = ImageInputs::default();
    let bytes: Vec<u8> = (0..40)
        .flat_map(|_| {
            (0..80).flat_map(|x| {
                if x < 40 {
                    [255, 0, 0, 255]
                } else {
                    [0, 255, 0, 255]
                }
            })
        })
        .collect();
    inputs.insert(
        "i".into(),
        Ok(Arc::new(
            StaticImage::from_premultiplied_rgba(80, 40, bytes, CompositeLimits::default())
                .unwrap(),
        )),
    );
    for fit in [
        display::ImageFit::Contain,
        display::ImageFit::Cover,
        display::ImageFit::Fill,
    ] {
        for transform in [
            Affine::IDENTITY,
            Affine::translation(60., 5.) * Affine::scale(-1.2, 1.4),
        ] {
            let mut s = scene(fit, Some(true), transform);
            s.update_object(&"i".into(), |o| {
                if let gpui_flow_scene::Shape::Image(image) = &mut o.shape {
                    image.natural_size = Some(Size::new(1., 700.));
                    image.source_crop = Some(Bounds::new(Point::new(0.5, 0.), Size::new(0.5, 1.)));
                }
            })
            .unwrap();
            let mut budget = PixelBudget::new(CompositeLimits::default(), 0).unwrap();
            let frame = rasterize_with_images(
                &source(&s),
                &inputs,
                RasterViewport {
                    world_to_screen: Affine::IDENTITY,
                    canvas_bounds: rect(120., 100.),
                    content_mask: rect(120., 100.),
                    device_scale: 1.,
                },
                &mut budget,
                &AtomicBool::new(false),
            )
            .unwrap()
            .unwrap();
            let center = transform.transform_point(Point::new(20., 20.));
            assert_eq!(
                at(&frame, center.x.floor() as u32, center.y.floor() as u32),
                [0, 255, 0, 255]
            );
            let corner = transform.transform_point(Point::new(1., 1.));
            assert_eq!(
                at(&frame, corner.x.floor() as u32, corner.y.floor() as u32),
                [0; 4]
            );
        }
    }
}

#[test]
fn explicit_crop_always_uses_bounded_cpu_and_fractional_pixels_match_unsnapped_reference() {
    // Source rectangle is (.65, .63, 2.55, 1.71) pixels. Native GPUI's
    // integer atlas rectangle would instead select (1, 1, 3, 2).
    let mut inputs = ImageInputs::default();
    let bytes: Vec<u8> = (0..3)
        .flat_map(|y| {
            (0..5).flat_map(move |x| [(x * 50) as u8, (y * 90) as u8, ((x + y) * 30) as u8, 255])
        })
        .collect();
    inputs.insert(
        "i".into(),
        Ok(Arc::new(
            StaticImage::from_premultiplied_rgba(5, 3, bytes, CompositeLimits::default()).unwrap(),
        )),
    );
    let raster = |scene: &Scene| {
        let mut budget = PixelBudget::new(CompositeLimits::default(), 0).unwrap();
        rasterize_with_images(
            &source(scene),
            &inputs,
            RasterViewport {
                world_to_screen: Affine::IDENTITY,
                canvas_bounds: rect(40., 24.),
                content_mask: rect(40., 24.),
                device_scale: 1.,
            },
            &mut budget,
            &AtomicBool::new(false),
        )
        .unwrap()
        .unwrap()
    };
    for fit in [
        display::ImageFit::Contain,
        display::ImageFit::Cover,
        display::ImageFit::Fill,
    ] {
        let mut image = display::Image::new("pixels", Size::new(40., 24.));
        image.fit = fit;
        image.natural_size = Some(Size::new(999., 1.));
        let crop = Bounds::new(Point::new(0.13, 0.21), Size::new(0.51, 0.57));
        image.source_crop = Some(crop);
        let mut actual = Scene::new();
        actual
            .insert(DisplayObject::new("i", image.clone()), None, None)
            .unwrap();
        assert!(matches!(
            scene_units(&actual, CompositeLimits::default()).unwrap()[0],
            SceneUnit::Domain(_)
        ));
        assert!(matches!(
            scene_units(
                &actual,
                CompositeLimits {
                    max_domains: 0,
                    ..Default::default()
                }
            )
            .unwrap()[0],
            SceneUnit::Rejected(CompositeIssue {
                error: CompositeError::InputLimit,
                ..
            })
        ));
        let pixels = raster(&actual);

        // Independent reference: place the entire source behind an outer Frame,
        // deriving its affine placement directly from fractional source pixels.
        // Its placement has no source_crop and is derived without Image::mapping.
        let (x, y, w, h) = (
            0.13f32 as f64 * 5.,
            0.21f32 as f64 * 3.,
            0.51f32 as f64 * 5.,
            0.57f32 as f64 * 3.,
        );
        let (sx, sy) = match fit {
            display::ImageFit::Fill => (40. / w, 24. / h),
            display::ImageFit::Contain => {
                let s = (40. / w).min(24. / h);
                (s, s)
            }
            display::ImageFit::Cover => {
                let s = (40. / w).max(24. / h);
                (s, s)
            }
        };
        let destination = Bounds::new(
            Point::new(((40. - w * sx) / 2.) as f32, ((24. - h * sy) / 2.) as f32),
            Size::new((w * sx) as f32, (h * sy) as f32),
        );
        let mut reference = Scene::new();
        reference
            .insert(
                DisplayObject::new("frame", display::Frame::new(40., 24.)).with_style(Style {
                    fill: None,
                    blend_mode: BlendMode::Normal,
                    ..Default::default()
                }),
                None,
                None,
            )
            .unwrap();
        reference
            .insert(
                DisplayObject::new(
                    "crop-clip",
                    display::Frame::new(destination.size.width, destination.size.height),
                )
                .with_transform(Affine::translation(
                    destination.origin.x,
                    destination.origin.y,
                ))
                .with_style(Style {
                    fill: None,
                    ..Default::default()
                }),
                Some("frame".into()),
                None,
            )
            .unwrap();
        let mut full = display::Image::new("pixels", Size::new((5. * sx) as f32, (3. * sy) as f32));
        full.fit = display::ImageFit::Fill;
        reference
            .insert(
                DisplayObject::new("i", full)
                    .with_transform(Affine::translation((-x * sx) as f32, (-y * sy) as f32)),
                Some("crop-clip".into()),
                None,
            )
            .unwrap();
        let expected = raster(&reference);
        for py in 0..24 {
            for px in 0..40 {
                let a = at(&pixels, px, py);
                let b = at(&expected, px, py);
                assert!(
                    a.iter().zip(b).all(|(a, b)| a.abs_diff(b) <= 2),
                    "fit={fit:?}, pixel={px},{py}: {a:?} vs {b:?}"
                );
            }
        }
        image.source_crop = Some(Bounds::new(
            Point::new(1. / 5., 1. / 3.),
            Size::new(3. / 5., 2. / 3.),
        ));
        let mut snapped = Scene::new();
        snapped
            .insert(DisplayObject::new("i", image), None, None)
            .unwrap();
        let rounded = raster(&snapped);
        assert!(
            (0..24).any(|y| (0..40).any(|x| at(&pixels, x, y)
                .iter()
                .zip(at(&rounded, x, y))
                .any(|(a, b)| a.abs_diff(b) > 15))),
            "fixture must detect source-texel snapping"
        );
    }
}
