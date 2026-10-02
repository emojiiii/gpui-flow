use super::*;
use gpui_flow_scene::{
    Affine, Bounds, Color, DisplayObject, Point, Scene, Shape, Size, Stroke, Style, display,
};
use std::sync::{Arc, atomic::AtomicBool};
fn rect(x: f32, y: f32, w: f32, h: f32) -> Bounds {
    Bounds::new(Point::new(x, y), Size::new(w, h))
}
fn pixel(image: &StaticImage, x: u32, y: u32) -> [u8; 4] {
    image.premultiplied_rgba()[((y * image.width() + x) * 4) as usize..][..4]
        .try_into()
        .unwrap()
}
fn surface() -> CanvasSurface {
    CanvasSurface::new(display::Canvas::new(12., 8.), CompositeLimits::default()).unwrap()
}
#[test]
fn canvas_surface_starts_transparent_and_publishes_immutable_atomic_generations() {
    let mut s = surface();
    let old = s.snapshot();
    let g = s.generation();
    s.fill_rect(rect(2., 1., 5., 4.), Color::hex(0xff0000))
        .unwrap();
    assert!(s.generation() > g);
    assert_eq!(pixel(&old, 3, 2), [0; 4]);
    assert_eq!(pixel(&s.snapshot(), 3, 2), [255, 0, 0, 255]);
    let before = s.snapshot();
    let g = s.generation();
    assert!(
        s.fill_rect(rect(f32::NAN, 0., 1., 1.), Color::hex(0xffffff))
            .is_err()
    );
    assert!(Arc::ptr_eq(&before, &s.snapshot()));
    assert_eq!(s.generation(), g);
    s.clear().unwrap();
    assert_eq!(pixel(&s.snapshot(), 3, 2), [0; 4]);
}
#[test]
fn safe_resize_preserves_physical_intersection_and_default_resize_clears() {
    let mut s = surface();
    s.fill_rect(rect(0., 0., 12., 8.), Color::hex(0xff0000))
        .unwrap();
    let mut c = display::Canvas::new(20., 10.);
    c.safe_resize = true;
    s.reconfigure(c.clone()).unwrap();
    assert_eq!(pixel(&s.snapshot(), 11, 7), [255, 0, 0, 255]);
    assert_eq!(pixel(&s.snapshot(), 12, 7), [0; 4]);
    c.size = Size::new(4., 4.);
    s.reconfigure(c.clone()).unwrap();
    c.size = Size::new(20., 10.);
    s.reconfigure(c.clone()).unwrap();
    assert_eq!(pixel(&s.snapshot(), 3, 3), [255, 0, 0, 255]);
    assert_eq!(pixel(&s.snapshot(), 4, 3), [0; 4]);
    c.pixel_ratio = 2.;
    s.reconfigure(c.clone()).unwrap();
    assert_eq!(s.snapshot().width(), 40);
    assert_eq!(pixel(&s.snapshot(), 3, 3), [255, 0, 0, 255]);
    assert_eq!(pixel(&s.snapshot(), 6, 3), [0; 4]);
    c.safe_resize = false;
    c.size.width = 10.;
    s.reconfigure(c).unwrap();
    assert_eq!(pixel(&s.snapshot(), 1, 1), [0; 4]);
}
#[test]
fn logical_dpr_and_smoothing_choose_real_pixel_filtering() {
    let image = StaticImage::from_premultiplied_rgba(
        2,
        1,
        vec![255, 0, 0, 255, 0, 0, 255, 255],
        CompositeLimits::default(),
    )
    .unwrap();
    let mut a =
        CanvasSurface::new(display::Canvas::new(8., 2.), CompositeLimits::default()).unwrap();
    let mut cfg = a.config().clone();
    cfg.smooth = false;
    a.reconfigure(cfg).unwrap();
    a.draw_image(&image, rect(0., 0., 8., 2.), 1.).unwrap();
    assert_eq!(pixel(&a.snapshot(), 3, 1), [255, 0, 0, 255]);
    let mut b =
        CanvasSurface::new(display::Canvas::new(8., 2.), CompositeLimits::default()).unwrap();
    b.draw_image(&image, rect(0., 0., 8., 2.), 1.).unwrap();
    let p = pixel(&b.snapshot(), 3, 1);
    assert!(p[0] > 0 && p[2] > 0);
    let mut cfg = display::Canvas::new(4., 4.);
    cfg.pixel_ratio = 2.;
    let mut c = CanvasSurface::new(cfg, CompositeLimits::default()).unwrap();
    c.fill_rect(rect(1., 1., 1., 1.), Color::hex(0x00ff00))
        .unwrap();
    assert_eq!(pixel(&c.snapshot(), 2, 2), [0, 255, 0, 255]);
    assert_eq!(pixel(&c.snapshot(), 1, 1), [0; 4]);
}
#[test]
fn surface_admission_enforces_memory_and_work_before_pixel_changes() {
    let cfg = display::Canvas::new(10., 10.);
    let mut s = CanvasSurface::new(
        cfg.clone(),
        CompositeLimits {
            max_surface_bytes: 799,
            ..Default::default()
        },
    )
    .unwrap();
    let old = s.snapshot();
    assert!(matches!(
        s.fill_rect(rect(0., 0., 1., 1.), Color::hex(0xffffff)),
        Err(CompositeError::WorkingSetLimit)
    ));
    assert!(Arc::ptr_eq(&old, &s.snapshot()));
    let mut s = CanvasSurface::new(
        cfg,
        CompositeLimits {
            max_surface_writes: 250,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        s.fill_rect(rect(0., 0., 1., 1.), Color::hex(0xffffff)),
        Err(CompositeError::WorkLimit)
    );
    let mut bad = s.config().clone();
    bad.pixel_ratio = f32::INFINITY;
    assert!(s.reconfigure(bad).is_err());
    assert_eq!(s.generation(), 1);
}
#[test]
fn subtree_draw_removes_root_world_pose_and_honors_explicit_local_offset() {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("parent", display::Group::new())
                .with_transform(Affine::translation(300., 100.)),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("source", display::Rect::new(3., 2.))
                .with_transform(Affine::translation(80., 60.) * Affine::rotation(35.))
                .with_style(Style {
                    fill: Some(Color::hex(0x00ff00).into()),
                    ..Default::default()
                }),
            Some("parent".into()),
            None,
        )
        .unwrap();
    let source = capture_subtree(&scene, &"source".into(), CompositeLimits::default()).unwrap();
    let mut s = surface();
    s.draw(
        &source,
        Affine::translation(4., 3.),
        &ImageInputs::default(),
        &TextInputs::default(),
    )
    .unwrap();
    assert_eq!(pixel(&s.snapshot(), 5, 4), [0, 255, 0, 255]);
    assert_eq!(pixel(&s.snapshot(), 2, 2), [0; 4]);
}
#[test]
fn failed_subtree_draw_and_singular_transform_cannot_publish_partial_content() {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("root", display::Group::new()),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("shape", display::Rect::new(3., 3.)),
            Some("root".into()),
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("missing", display::Image::new("a.png", Size::new(4., 4.))),
            Some("root".into()),
            None,
        )
        .unwrap();
    let source = capture_subtree(&scene, &"root".into(), CompositeLimits::default()).unwrap();
    let mut s = surface();
    let old = s.snapshot();
    assert!(
        s.draw(
            &source,
            Affine::IDENTITY,
            &ImageInputs::default(),
            &TextInputs::default()
        )
        .is_err()
    );
    assert!(Arc::ptr_eq(&old, &s.snapshot()));
    assert!(
        s.draw(
            &source,
            Affine::scale(0., 1.),
            &ImageInputs::default(),
            &TextInputs::default()
        )
        .is_err()
    );
    assert_eq!(s.generation(), 1);
}
fn render(scene: &Scene, input: &ImageInputs) -> CpuFrame {
    let source = scene_units(scene, CompositeLimits::default())
        .unwrap()
        .into_iter()
        .find_map(|unit| {
            if let SceneUnit::Domain(d) = unit {
                Some(d)
            } else {
                None
            }
        })
        .unwrap();
    rasterize_with_images(
        &source,
        input,
        RasterViewport {
            world_to_screen: Affine::IDENTITY,
            canvas_bounds: rect(0., 0., 30., 30.),
            content_mask: rect(0., 0., 30., 30.),
            device_scale: 1.,
        },
        &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap()
}
fn at(frame: &CpuFrame, x: u32, y: u32) -> [u8; 4] {
    let x = x as f32 - frame.screen_bounds.origin.x;
    let y = y as f32 - frame.screen_bounds.origin.y;
    if x < 0. || y < 0. || x >= frame.width as f32 || y >= frame.height as f32 {
        return [0; 4];
    }
    frame.premul_rgba[((y as u32 * frame.width + x as u32) * 4) as usize..][..4]
        .try_into()
        .unwrap()
}
#[test]
fn canvas_content_is_between_fills_and_strokes_and_clips_to_real_corners() {
    let cfg = display::Canvas::new(20., 20.).with_corners([8., 0., 0., 0.]);
    let mut surface = CanvasSurface::new(cfg.clone(), CompositeLimits::default()).unwrap();
    surface
        .fill_rect(rect(0., 0., 10., 20.), Color::hex(0xff0000))
        .unwrap();
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("canvas", cfg)
                .with_transform(Affine::translation(2., 2.))
                .with_style(Style {
                    fill: Some(Color::hex(0x0000ff).into()),
                    stroke: Some(Stroke::new(Color::hex(0x00ff00), 4.)),
                    ..Default::default()
                }),
            None,
            None,
        )
        .unwrap();
    let mut inputs = ImageInputs::default();
    inputs.insert("canvas".into(), Ok(surface.snapshot()));
    let frame = render(&scene, &inputs);
    // (3,3) is inside the centered stroke; (2,2) is outside actual ink.
    assert_eq!(at(&frame, 2, 2), [0; 4]);
    assert_eq!(at(&frame, 8, 12), [255, 0, 0, 255]);
    assert_eq!(at(&frame, 16, 12), [0, 0, 255, 255]);
    assert_eq!(at(&frame, 21, 12), [0, 255, 0, 255]);
}
#[test]
fn canvas_missing_owned_pixels_rejects_instead_of_painting_a_rectangle() {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("canvas", display::Canvas::default()),
            None,
            None,
        )
        .unwrap();
    let units = scene_units(&scene, CompositeLimits::default()).unwrap();
    assert!(matches!(units[0], SceneUnit::Domain(_)));
    let SceneUnit::Domain(source) = &units[0] else {
        unreachable!()
    };
    assert!(matches!(
        rasterize(
            source,
            RasterViewport {
                world_to_screen: Affine::IDENTITY,
                canvas_bounds: rect(0., 0., 30., 30.),
                content_mask: rect(0., 0., 30., 30.),
                device_scale: 1.
            },
            &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
            &AtomicBool::new(false)
        ),
        Err(CompositeError::ImagePending(_))
    ));
    assert_eq!(source.canvases().len(), 1);
    assert!(source.images().is_empty());
    assert!(matches!(
        scene.get(&"canvas".into()).unwrap().object.shape,
        Shape::Canvas(_)
    ));
}

#[test]
fn canvas_scene_sampler_respects_smoothing_for_owned_content() {
    let input = StaticImage::from_premultiplied_rgba(
        2,
        1,
        vec![255, 0, 0, 255, 0, 0, 255, 255],
        CompositeLimits::default(),
    )
    .unwrap();
    let mut inputs = ImageInputs::default();
    inputs.insert("c".into(), Ok(Arc::new(input)));
    let mut scene = Scene::new();
    let mut c = display::Canvas::new(8., 2.);
    c.smooth = false;
    scene
        .insert(
            DisplayObject::new("c", c.clone()).with_style(Style {
                fill: None,
                ..Default::default()
            }),
            None,
            None,
        )
        .unwrap();
    let a = render(&scene, &inputs);
    assert_eq!(at(&a, 3, 1), [255, 0, 0, 255]);
    c.smooth = true;
    scene
        .update_object(&"c".into(), |o| o.shape = c.into())
        .unwrap();
    let b = render(&scene, &inputs);
    let p = at(&b, 3, 1);
    assert!(p[0] > 0 && p[2] > 0);
}
