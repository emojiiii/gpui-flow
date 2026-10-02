use super::*;
use crate::{ImageAlphaHit, ImageHitFallback, scene_view::image_hit::ImageHitPolicies};
use gpui_flow_scene::{
    Affine, DisplayId, DisplayObject, HitTestOptions, Point, Scene, Size, display,
};

fn setup(scene: Scene) -> (SceneEngine, SceneResourceAdapter, ImageHitPolicies) {
    let mut engine = SceneEngine::new(scene).unwrap();
    let mut resources = SceneResourceAdapter::new(&mut engine).unwrap();
    resources.tracker.synchronize(&mut engine).unwrap();
    (engine, resources, ImageHitPolicies::default())
}
fn scene() -> Scene {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("back", display::Rect::new(100., 100.)),
            None,
            None,
        )
        .unwrap();
    scene.insert(image(), None, None).unwrap();
    scene
}
fn image() -> DisplayObject {
    DisplayObject::new("image", display::Image::new("a.png", Size::new(100., 100.)))
}
fn strict() -> ImageAlphaHit {
    ImageAlphaHit {
        threshold: 0,
        pending: ImageHitFallback::PassThrough,
        failed: ImageHitFallback::PassThrough,
    }
}
fn install(p: &mut ImageHitPolicies, e: &SceneEngine, policy: ImageAlphaHit) {
    assert!(p.set(e, e.handle(&"image".into()).unwrap(), Some(policy)));
}
fn frame(alpha: u8) -> image::Frame {
    image::Frame::from_parts(
        image::RgbaImage::from_pixel(2, 2, image::Rgba([41, 37, 91, alpha])),
        0,
        0,
        image::Delay::from_numer_denom_ms(20, 1),
    )
}
fn load(
    e: &mut SceneEngine,
    r: &mut SceneResourceAdapter,
    frames: Vec<image::Frame>,
) -> ResourceTicket {
    r.tracker.synchronize(e).unwrap();
    let t = r
        .tracker
        .ticket(e, e.handle(&"image".into()).unwrap())
        .unwrap();
    r.settle(
        e,
        t,
        ResourceOutcome::Loaded,
        Some(SceneImagePresentation::Loaded(Arc::new(RenderImage::new(
            frames,
        )))),
        &mut vec![],
    )
    .unwrap();
    r.synchronize_playbacks();
    t
}
fn hit(
    p: &ImageHitPolicies,
    e: &SceneEngine,
    r: &SceneResourceAdapter,
    x: f32,
    y: f32,
) -> Option<DisplayId> {
    p.hit_test(e, Some(r), Point::new(x, y), HitTestOptions::default())
        .unwrap()
        .map(|h| h.id)
}
fn change_source(e: &mut SceneEngine, source: &str) {
    e.transaction(|s| {
        s.update_object(&"image".into(), |o| {
            if let Shape::Image(image) = &mut o.shape {
                image.source = source.into();
            }
        })
    })
    .unwrap();
}

#[test]
fn image_alpha_default_geometry_threshold_and_opt_out_preserve_painted_order() {
    let (mut e, mut r, mut p) = setup(scene());
    load(&mut e, &mut r, vec![frame(127)]);
    assert_eq!(hit(&p, &e, &r, 50., 50.), Some("image".into()));
    install(
        &mut p,
        &e,
        ImageAlphaHit {
            threshold: 126,
            ..strict()
        },
    );
    assert_eq!(hit(&p, &e, &r, 50., 50.), Some("image".into()));
    install(
        &mut p,
        &e,
        ImageAlphaHit {
            threshold: 127,
            ..strict()
        },
    );
    assert_eq!(hit(&p, &e, &r, 50., 50.), Some("back".into()));
    assert!(p.set(&e, e.handle(&"image".into()).unwrap(), None));
    assert_eq!(hit(&p, &e, &r, 50., 50.), Some("image".into()));
}

#[test]
fn image_alpha_hole_clicks_through_to_next_painted_object_not_background() {
    let mut scene = scene();
    let mut middle = DisplayObject::new("middle", display::Rect::new(100., 100.));
    middle.z_index = 1;
    scene.insert(middle, None, None).unwrap();
    scene
        .update_object(&"image".into(), |o| o.z_index = 2)
        .unwrap();
    let (mut e, mut r, mut p) = setup(scene);
    let mut pixels = image::RgbaImage::from_pixel(2, 2, image::Rgba([0, 0, 0, 255]));
    pixels.put_pixel(0, 0, image::Rgba([255, 255, 255, 0]));
    load(&mut e, &mut r, vec![image::Frame::new(pixels)]);
    install(&mut p, &e, strict());
    assert_eq!(hit(&p, &e, &r, 25., 25.), Some("middle".into()));
    assert_eq!(hit(&p, &e, &r, 75., 25.), Some("image".into()));
}

#[test]
fn image_alpha_pending_failed_cancelled_and_missing_adapter_have_explicit_fallbacks() {
    let (mut e, mut r, mut p) = setup(scene());
    install(&mut p, &e, strict());
    assert_eq!(hit(&p, &e, &r, 50., 50.), Some("back".into()));
    install(
        &mut p,
        &e,
        ImageAlphaHit {
            pending: ImageHitFallback::Geometry,
            ..strict()
        },
    );
    assert_eq!(hit(&p, &e, &r, 50., 50.), Some("image".into()));
    let t = r
        .tracker
        .ticket(&e, e.handle(&"image".into()).unwrap())
        .unwrap();
    r.settle(
        &mut e,
        t,
        ResourceOutcome::Failed("decode failed".into()),
        Some(SceneImagePresentation::Failed("decode failed".into())),
        &mut vec![],
    )
    .unwrap();
    assert_eq!(hit(&p, &e, &r, 50., 50.), Some("back".into()));
    install(&mut p, &e, ImageAlphaHit::default());
    assert_eq!(hit(&p, &e, &r, 50., 50.), Some("image".into()));
    r.set_image_resolver(&e, Rc::new(|_| None)).unwrap();
    r.tracker.synchronize(&mut e).unwrap();
    let t = r
        .tracker
        .ticket(&e, e.handle(&"image".into()).unwrap())
        .unwrap();
    r.settle(
        &mut e,
        t,
        ResourceOutcome::Cancelled(ResourceCancellation::NotAuthorized),
        Some(SceneImagePresentation::Cancelled),
        &mut vec![],
    )
    .unwrap();
    install(&mut p, &e, strict());
    assert_eq!(hit(&p, &e, &r, 50., 50.), Some("back".into()));
    assert_eq!(
        p.hit_test(&e, None, Point::new(50., 50.), HitTestOptions::default())
            .unwrap()
            .unwrap()
            .id,
        DisplayId::from("back")
    );
    install(&mut p, &e, ImageAlphaHit::default());
    assert_eq!(hit(&p, &e, &r, 50., 50.), Some("image".into()));
}

#[test]
fn image_alpha_uses_current_animation_frame_including_pause_and_seek() {
    let (mut e, mut r, mut p) = setup(scene());
    load(&mut e, &mut r, vec![frame(0), frame(255)]);
    install(&mut p, &e, strict());
    let h = e.handle(&"image".into()).unwrap();
    let visible = BTreeSet::from(["image".into()]);
    r.sample_images(&e, &visible, Duration::ZERO, true);
    assert_eq!(hit(&p, &e, &r, 50., 50.), Some("back".into()));
    r.sample_images(&e, &visible, Duration::from_millis(20), true);
    assert_eq!(r.image_frame(&e, h), 1);
    assert_eq!(hit(&p, &e, &r, 50., 50.), Some("image".into()));
    assert!(r.set_image_playing(&e, h, Duration::from_millis(20), false));
    r.sample_images(&e, &visible, Duration::from_secs(10), true);
    assert_eq!(hit(&p, &e, &r, 50., 50.), Some("image".into()));
    assert!(r.seek_image(&e, h, Duration::ZERO));
    assert_eq!(hit(&p, &e, &r, 50., 50.), Some("back".into()));
}

#[test]
fn image_alpha_source_a_b_a_cannot_resurrect_old_pixels_even_before_prepare() {
    let (mut e, mut r, mut p) = setup(scene());
    let old = load(&mut e, &mut r, vec![frame(255)]);
    install(&mut p, &e, strict());
    assert_eq!(hit(&p, &e, &r, 50., 50.), Some("image".into()));
    for source in ["b.png", "a.png"] {
        change_source(&mut e, source);
        assert_eq!(hit(&p, &e, &r, 50., 50.), Some("back".into()));
    }
    r.settle(
        &mut e,
        old,
        ResourceOutcome::Loaded,
        Some(SceneImagePresentation::Loaded(Arc::new(RenderImage::new(
            vec![frame(255)],
        )))),
        &mut vec![],
    )
    .unwrap();
    assert_eq!(hit(&p, &e, &r, 50., 50.), Some("back".into()));
    let new = load(&mut e, &mut r, vec![frame(0)]);
    assert_ne!(old, new);
    assert_eq!(hit(&p, &e, &r, 50., 50.), Some("back".into()));
}

#[test]
fn image_alpha_unmount_remount_expires_policy_and_loaded_generation() {
    let (mut e, mut r, mut p) = setup(scene());
    load(&mut e, &mut r, vec![frame(255)]);
    install(&mut p, &e, strict());
    let h = e.handle(&"image".into()).unwrap();
    let (subtree, _) = e.detach(h).unwrap();
    assert!(p.get(&e, h).is_none());
    assert!(!p.set(&e, h, Some(strict())));
    e.attach(&subtree, None, None).unwrap();
    assert_eq!(e.handle(&"image".into()), Some(h));
    assert!(p.get(&e, h).is_none());
    // Default geometric behavior returns until host opts this mount in again.
    assert_eq!(hit(&p, &e, &r, 50., 50.), Some("image".into()));
    install(&mut p, &e, strict());
    assert_eq!(hit(&p, &e, &r, 50., 50.), Some("back".into()));
}

#[test]
fn image_alpha_destroy_recreate_and_document_reset_reject_old_policy_and_pixels() {
    let (mut e, mut r, mut p) = setup(scene());
    load(&mut e, &mut r, vec![frame(255)]);
    install(&mut p, &e, strict());
    let h = e.handle(&"image".into()).unwrap();
    e.destroy(h).unwrap();
    e.transaction(|s| s.insert(image(), None, None)).unwrap();
    assert!(!p.set(&e, h, Some(strict())));
    let new = e.handle(&"image".into()).unwrap();
    assert_ne!(h, new);
    assert!(p.get(&e, new).is_none());
    install(&mut p, &e, strict());
    assert_eq!(hit(&p, &e, &r, 50., 50.), Some("back".into()));
    load(&mut e, &mut r, vec![frame(255)]);
    e.reset_scene(e.scene().clone()).unwrap();
    assert!(p.get(&e, new).is_none());
    install(&mut p, &e, strict());
    assert_eq!(hit(&p, &e, &r, 50., 50.), Some("back".into()));
}

#[test]
fn image_alpha_resolver_revocation_cannot_sample_previously_loaded_bytes() {
    let (mut e, mut r, mut p) = setup(scene());
    load(&mut e, &mut r, vec![frame(255)]);
    install(&mut p, &e, strict());
    r.set_image_resolver(&e, Rc::new(|_| None)).unwrap();
    assert_eq!(hit(&p, &e, &r, 50., 50.), Some("back".into()));
    r.dispose(&mut e).unwrap();
    assert_eq!(hit(&p, &e, &r, 50., 50.), Some("back".into()));
}

#[test]
fn image_alpha_rounded_and_nested_ancestor_clips_remain_geometric_gates() {
    let mut scene = scene();
    scene.remove(&"image".into()).unwrap();
    scene
        .insert(
            DisplayObject::new("outer", display::Frame::new(100., 100.).rounded(40.)),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("inner", display::Frame::new(80., 80.))
                .with_transform(Affine::translation(10., 10.)),
            Some("outer".into()),
            None,
        )
        .unwrap();
    let mut object = image();
    if let Shape::Image(image) = &mut object.shape {
        image.corner_radius = 40.;
    }
    scene.insert(object, Some("inner".into()), None).unwrap();
    let (mut e, mut r, mut p) = setup(scene);
    load(&mut e, &mut r, vec![frame(255)]);
    install(&mut p, &e, strict());
    assert_eq!(hit(&p, &e, &r, 50., 50.), Some("image".into()));
    assert_ne!(hit(&p, &e, &r, 11., 11.), Some("image".into()));
    assert_ne!(hit(&p, &e, &r, 95., 50.), Some("image".into()));
    assert_ne!(hit(&p, &e, &r, 1., 1.), Some("image".into()));
}

#[test]
fn image_alpha_inverse_affine_viewport_zoom_and_dpr_do_not_double_scale_sampling() {
    let mut scene = scene();
    let transform = Affine::translation(40., 70.)
        * Affine::rotation(37.)
        * Affine::skew(13., -7.)
        * Affine::scale(-2., 0.7);
    scene
        .update_object(&"image".into(), |o| o.transform = transform)
        .unwrap();
    let (mut e, mut r, mut p) = setup(scene);
    let mut pixels = image::RgbaImage::new(2, 2);
    pixels.put_pixel(1, 0, image::Rgba([0, 0, 0, 255]));
    load(&mut e, &mut r, vec![image::Frame::new(pixels)]);
    install(&mut p, &e, strict());
    for zoom in [0.25, 1., 2.5] {
        let viewport = gpui_flow_core::Viewport {
            zoom,
            pan: Point::new(100., -40.),
            ..Default::default()
        };
        for dpr in [1., 1.5, 2.] {
            for (local, expected) in [(Point::new(75., 25.), true), (Point::new(25., 25.), false)] {
                let physical = viewport.world_to_screen(transform.transform_point(local)) * dpr;
                let world = viewport.screen_to_world(physical / dpr);
                assert_eq!(
                    hit(&p, &e, &r, world.x, world.y) == Some("image".into()),
                    expected
                );
            }
        }
    }
}

#[test]
fn image_alpha_strict_geometry_stays_inside_even_with_bounds_policy_and_tolerance() {
    let mut scene = scene();
    scene
        .update_object(&"image".into(), |o| {
            o.hit_policy = gpui_flow_scene::HitPolicy::Bounds;
            if let Shape::Image(image) = &mut o.shape {
                image.corner_radius = 40.;
            }
        })
        .unwrap();
    let (mut e, mut r, mut p) = setup(scene);
    load(&mut e, &mut r, vec![frame(255)]);
    install(&mut p, &e, strict());
    assert_eq!(hit(&p, &e, &r, 1., 1.), Some("back".into()));
    let picked = p
        .hit_test(
            &e,
            Some(&r),
            Point::new(-1., 50.),
            HitTestOptions {
                tolerance: 4.,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(picked.is_none_or(|h| h.id != DisplayId::from("image")));
}

#[test]
fn image_alpha_install_query_and_sample_leave_document_revision_and_undo_untouched() {
    let (mut e, mut r, mut p) = setup(scene());
    let before = serde_json::to_string(e.scene()).unwrap();
    let revision = e.revision();
    load(&mut e, &mut r, vec![frame(128)]);
    install(&mut p, &e, strict());
    assert!(!p.set(&e, e.handle(&"back".into()).unwrap(), Some(strict())));
    for _ in 0..10 {
        hit(&p, &e, &r, 50., 50.);
    }
    assert_eq!(serde_json::to_string(e.scene()).unwrap(), before);
    assert_eq!(e.revision(), revision);
    let mut editor = gpui_flow_editor::SceneEditor::from_engine(e);
    assert!(!editor.undo());
    assert!(!editor.redo());
}

#[test]
fn image_alpha_crop_uses_decoded_candidates_with_opposite_stale_aspect_ratios() {
    use gpui_flow_scene::{Bounds, display::ImageFit};
    for (width, height, stale, point) in [
        (4, 2, Size::new(1., 100.), Point::new(10., 50.)),
        (2, 4, Size::new(100., 1.), Point::new(50., 10.)),
    ] {
        for crop in [
            None,
            Some(Bounds::new(Point::new(0.5, 0.), Size::new(0.5, 1.))),
        ] {
            for fit in [ImageFit::Contain, ImageFit::Cover, ImageFit::Fill] {
                let mut scene = scene();
                scene
                    .update_object(&"image".into(), |o| {
                        if let Shape::Image(i) = &mut o.shape {
                            i.natural_size = Some(stale);
                            i.source_crop = crop;
                            i.fit = fit;
                        }
                    })
                    .unwrap();
                let (mut e, mut r, mut p) = setup(scene);
                load(
                    &mut e,
                    &mut r,
                    vec![image::Frame::new(image::RgbaImage::from_pixel(
                        width,
                        height,
                        image::Rgba([0, 0, 0, 255]),
                    ))],
                );
                let canonical = hit(&p, &e, &r, point.x, point.y);
                if fit == ImageFit::Contain {
                    assert_eq!(
                        canonical,
                        Some("back".into()),
                        "fixture must expose metadata rejection"
                    );
                }
                install(&mut p, &e, strict());
                assert_eq!(hit(&p, &e, &r, point.x, point.y), Some("image".into()));
                if fit == ImageFit::Contain {
                    let letterbox = if width > height && crop.is_none() {
                        Some(Point::new(50., 10.))
                    } else if width < height {
                        Some(Point::new(10., 50.))
                    } else {
                        None
                    };
                    if let Some(point) = letterbox {
                        assert_eq!(hit(&p, &e, &r, point.x, point.y), Some("back".into()));
                    }
                }
                assert!(p.set(&e, e.handle(&"image".into()).unwrap(), None));
                assert_eq!(hit(&p, &e, &r, point.x, point.y), canonical);
            }
        }
    }
}

#[test]
fn image_alpha_crop_unavailable_geometry_fallback_never_expands_metadata_bounds() {
    let mut scene = scene();
    scene
        .update_object(&"image".into(), |o| {
            if let Shape::Image(i) = &mut o.shape {
                i.natural_size = Some(Size::new(1., 100.));
                i.source_crop = Some(gpui_flow_scene::Bounds::new(
                    Point::new(0.5, 0.),
                    Size::new(0.5, 1.),
                ));
            }
        })
        .unwrap();
    let (mut e, mut r, mut p) = setup(scene);
    install(&mut p, &e, ImageAlphaHit::default());
    // Pending retains the old narrow metadata box, even though ready pixels
    // will fill this square once the actual dimensions become available.
    assert_eq!(hit(&p, &e, &r, 10., 50.), Some("back".into()));
    assert_eq!(hit(&p, &e, &r, 50., 50.), Some("image".into()));
    load(
        &mut e,
        &mut r,
        vec![image::Frame::new(image::RgbaImage::from_pixel(
            4,
            2,
            image::Rgba([0, 0, 0, 255]),
        ))],
    );
    assert_eq!(hit(&p, &e, &r, 10., 50.), Some("image".into()));
    for source in ["b.png", "a.png"] {
        change_source(&mut e, source);
        assert_eq!(hit(&p, &e, &r, 10., 50.), Some("back".into()));
        assert_eq!(hit(&p, &e, &r, 50., 50.), Some("image".into()));
    }
    r.tracker.synchronize(&mut e).unwrap();
    let t = r
        .tracker
        .ticket(&e, e.handle(&"image".into()).unwrap())
        .unwrap();
    r.settle(
        &mut e,
        t,
        ResourceOutcome::Failed("decode failed".into()),
        Some(SceneImagePresentation::Failed("decode failed".into())),
        &mut vec![],
    )
    .unwrap();
    for resources in [Some(&r), None] {
        for (point, expected) in [
            (Point::new(10., 50.), "back"),
            (Point::new(50., 50.), "image"),
        ] {
            assert_eq!(
                p.hit_test(&e, resources, point, HitTestOptions::default())
                    .unwrap()
                    .unwrap()
                    .id,
                DisplayId::from(expected)
            );
        }
    }
}

#[test]
fn state_geometry_alpha_uses_presented_coordinates_and_canonical_resource_identity() {
    let mut source = scene();
    source
        .update_object(&"back".into(), |o| {
            o.shape = display::Rect::new(400., 100.).into()
        })
        .unwrap();
    source
        .update_object(&"image".into(), |o| {
            if let Shape::Image(image) = &mut o.shape {
                image.fit = display::ImageFit::Fill;
            }
        })
        .unwrap();
    let (mut canonical, mut resources, mut policies) = setup(source);
    install(&mut policies, &canonical, strict());
    load(
        &mut canonical,
        &mut resources,
        vec![image::Frame::new(image::RgbaImage::from_fn(
            2,
            2,
            |x, _| image::Rgba([10, 20, 30, if x == 0 { 255 } else { 0 }]),
        ))],
    );
    let before = canonical.scene().to_json().unwrap();
    let revision = canonical.revision();
    let mut presented = canonical.scene().clone();
    presented
        .update_object(&"image".into(), |o| {
            o.transform = Affine::translation(120., 0.);
            if let Shape::Image(image) = &mut o.shape {
                image.size.width = 200.;
            }
        })
        .unwrap();
    let geometry = SceneEngine::new(presented).unwrap();
    let query = |canonical: &SceneEngine, x| {
        policies
            .hit_test_with_geometry(
                canonical,
                &geometry,
                Some(&resources),
                Point::new(x, 50.),
                HitTestOptions::default(),
            )
            .unwrap()
            .unwrap()
            .id
    };
    assert_eq!(query(&canonical, 150.), DisplayId::from("image"));
    assert_eq!(query(&canonical, 270.), DisplayId::from("back"));
    assert_eq!(query(&canonical, 20.), DisplayId::from("back"));
    assert_eq!(canonical.revision(), revision);
    assert_eq!(canonical.scene().to_json().unwrap(), before);
    // A changed source cannot sample the still-retained old loaded frame.
    change_source(&mut canonical, "b.png");
    assert_eq!(query(&canonical, 150.), DisplayId::from("back"));
}

#[test]
fn state_geometry_without_alpha_policy_uses_existing_presented_engine_hits() {
    let (canonical, resources, policies) = setup(scene());
    let mut presented = canonical.scene().clone();
    presented
        .update_object(&"image".into(), |o| {
            o.transform = Affine::translation(120., 0.)
        })
        .unwrap();
    let geometry = SceneEngine::new(presented).unwrap();
    let hit = policies
        .hit_test_with_geometry(
            &canonical,
            &geometry,
            Some(&resources),
            Point::new(150., 50.),
            HitTestOptions::default(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(hit.id, DisplayId::from("image"));
    assert_eq!(hit.local_point, Point::new(30., 50.));
    assert!(
        canonical
            .hit_test(Point::new(150., 50.), HitTestOptions::default())
            .unwrap()
            .is_none()
    );
}

#[test]
fn coherent_cpu_alpha_hits_use_displayed_frame_and_reject_revoked_generation() {
    use crate::display::composite::RequestKey;
    use gpui_flow_composite::{CompositeLimits, RasterViewport, SceneUnit};
    let mut source = scene();
    source
        .update_object(&"image".into(), |object| {
            object.style.blend_mode = gpui_flow_scene::BlendMode::Normal
        })
        .unwrap();
    let (mut engine, mut resources, mut policies) = setup(source);
    load(&mut engine, &mut resources, vec![frame(0), frame(255)]);
    install(&mut policies, &engine, strict());
    let visible = BTreeSet::from(["image".into()]);
    resources.sample_images(&engine, &visible, Duration::ZERO, true);
    let bounds = gpui_flow_scene::Bounds::new(Point::ZERO, Size::new(100., 100.));
    let domains = gpui_flow_composite::scene_units(engine.scene(), CompositeLimits::default())
        .unwrap()
        .into_iter()
        .filter_map(|unit| match unit {
            SceneUnit::Domain(domain) => Some(*domain),
            _ => None,
        })
        .collect();
    let accepted = RequestKey::new(
        domains,
        &engine,
        RasterViewport {
            world_to_screen: Affine::IDENTITY,
            canvas_bounds: bounds,
            content_mask: bounds,
            device_scale: 1.,
        },
        1,
    )
    .with_resources(&engine, Some(&resources));
    assert_eq!(
        accepted.image_sample(&"image".into()).unwrap().frame_index,
        0
    );
    resources.sample_images(&engine, &visible, Duration::from_millis(20), true);
    let query = |key: Option<&RequestKey>| {
        policies
            .hit_test_presented(
                &engine,
                &engine,
                Some(&resources),
                Point::new(50., 50.),
                HitTestOptions::default(),
                key,
            )
            .unwrap()
            .unwrap()
            .id
    };
    assert_eq!(
        query(None),
        DisplayId::from("image"),
        "live/native frame is now opaque"
    );
    assert_eq!(
        query(Some(&accepted)),
        DisplayId::from("back"),
        "retained CPU pixels are still transparent"
    );
    assert!(accepted.presentation_current(
        &engine,
        Some(&resources),
        None,
        accepted.viewport,
        accepted.window
    ));
    let handle = engine.handle(&"image".into()).unwrap();
    assert!(resources.set_image_playing(&engine, handle, Duration::from_millis(20), false));
    assert!(!accepted.presentation_current(
        &engine,
        Some(&resources),
        None,
        accepted.viewport,
        accepted.window
    ));
    // Even direct callers cannot borrow the opaque live frame after the accepted
    // sample's resource/playback generation has been invalidated.
    assert_eq!(
        policies
            .hit_test_presented(
                &engine,
                &engine,
                Some(&resources),
                Point::new(50., 50.),
                HitTestOptions::default(),
                Some(&accepted)
            )
            .unwrap()
            .unwrap()
            .id,
        DisplayId::from("back")
    );
}
