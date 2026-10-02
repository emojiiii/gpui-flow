use super::*;
use gpui_flow_scene::{
    Affine, BlendMode, Bounds, Color, DisplayObject, Point, Scene, Shape, Size, Style, display,
};
fn scene() -> Scene {
    let mut s = Scene::new();
    s.insert(
        DisplayObject::new(
            "frame",
            display::Frame::new(80., 80.).with_corners([35., 0., 30., 0.]),
        )
        .with_transform(Affine::translation(5., 5.))
        .with_style(Style {
            fill: None,
            ..Default::default()
        }),
        None,
        None,
    )
    .unwrap();
    s.insert(
        DisplayObject::new(
            "inner",
            display::Box::new(80., 80.)
                .clipped(true)
                .with_corners([0., 35., 0., 0.]),
        )
        .with_style(Style {
            fill: None,
            ..Default::default()
        }),
        Some("frame".into()),
        None,
    )
    .unwrap();
    let mut t = display::Text::new("██████\n██████\n██████\n██████", Size::new(80., 80.));
    t.font_family = "DejaVu Sans".into();
    t.font_size = 20.;
    t.line_height = 1.;
    s.insert(
        DisplayObject::new("text", t).with_style(Style::fill(Color::WHITE)),
        Some("inner".into()),
        None,
    )
    .unwrap();
    s
}
fn request(
    e: &SceneEngine,
    bridge: Option<&Arc<gpui_flow_text::GpuiTextRasterizer>>,
) -> Arc<RequestKey> {
    let domains = gpui_flow_composite::scene_units(e.scene(), CompositeLimits::default())
        .unwrap()
        .into_iter()
        .filter_map(|u| match u {
            gpui_flow_composite::SceneUnit::Domain(d) => Some(*d),
            _ => None,
        })
        .collect();
    let b = Bounds::new(Point::ZERO, Size::new(100., 100.));
    Arc::new(
        RequestKey::new(
            domains,
            e,
            RasterViewport {
                world_to_screen: Affine::IDENTITY,
                canvas_bounds: b,
                content_mask: b,
                device_scale: 1.,
            },
            1,
        )
        .with_text_backend(bridge),
    )
}
fn render(
    e: &SceneEngine,
    bridge: Option<&Arc<gpui_flow_text::GpuiTextRasterizer>>,
) -> Result<Option<CpuFrame>, CompositeError> {
    let mut c = CompositeController::default();
    c.request(request(e, bridge));
    c.take_work().unwrap().run().remove(0).1
}
fn alpha(f: &CpuFrame, x: u32, y: u32) -> u8 {
    let x = x as f32 - f.screen_bounds.origin.x;
    let y = y as f32 - f.screen_bounds.origin.y;
    if x < 0. || y < 0. || x >= f.width as f32 || y >= f.height as f32 {
        return 0;
    }
    f.premul_rgba[((y as u32 * f.width + x as u32) * 4 + 3) as usize]
}
#[test]
fn automatic_text_clip_domains_preserve_real_glyph_pixels_and_padding() {
    let (_app, bridge) =
        gpui_flow_text::GpuiTextRasterizer::application(gpui_kit::platform::current_platform(true));
    let mut e = SceneEngine::new(scene()).unwrap();
    let clipped = render(&e, Some(&bridge)).unwrap().unwrap();
    assert_eq!(alpha(&clipped, 6, 6), 0);
    assert_eq!(alpha(&clipped, 83, 6), 0);
    assert!(
        clipped
            .premul_rgba
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[3] > 0)
            .count()
            > 2000
    );
    let mut explicit = e.scene().clone();
    explicit
        .update_object(&"frame".into(), |o| o.style.blend_mode = BlendMode::Normal)
        .unwrap();
    let forced = render(&SceneEngine::new(explicit).unwrap(), Some(&bridge))
        .unwrap()
        .unwrap();
    for y in 0..90 {
        for x in 0..90 {
            // Inline mask multiplication and post-domain apply_mask can round
            // one premultiplied-alpha byte differently. Geometry must agree.
            assert!(
                alpha(&clipped, x, y).abs_diff(alpha(&forced, x, y)) <= 1,
                "{x},{y}: {} != {}",
                alpha(&clipped, x, y),
                alpha(&forced, x, y)
            );
        }
    }
    e.transaction(|s| {
        s.update_object(&"text".into(), |o| {
            if let Shape::Text(t) = &mut o.shape {
                t.padding = [15.; 4];
            }
        })
    })
    .unwrap();
    let padded = render(&e, Some(&bridge)).unwrap().unwrap();
    assert!(
        padded
            .premul_rgba
            .as_chunks::<4>()
            .0
            .iter()
            .any(|p| p[3] > 0)
    );
    for y in 0..100 {
        for x in 0..20 {
            assert_eq!(alpha(&padded, x, y), 0);
        }
    }
}
#[test]
fn rounded_text_clips_keep_missing_bridge_and_unsupported_transform_explicit() {
    let (_app, bridge) =
        gpui_flow_text::GpuiTextRasterizer::application(gpui_kit::platform::current_platform(true));
    let mut e = SceneEngine::new(scene()).unwrap();
    assert!(
        matches!(render(&e,None),Err(CompositeError::TextBackendRequired(id)) if id == "text".into())
    );
    e.transaction(|s| s.update_object(&"text".into(), |o| o.transform = Affine::rotation(10.)))
        .unwrap();
    assert!(matches!(
        render(&e, Some(&bridge)),
        Err(CompositeError::UnsupportedTransform)
    ));
}
#[test]
fn ancestor_corner_edit_and_undo_discard_late_text_clip_jobs() {
    let (_app, bridge) =
        gpui_flow_text::GpuiTextRasterizer::application(gpui_kit::platform::current_platform(true));
    let mut editor = gpui_flow_editor::SceneEditor::new(scene()).unwrap();
    let before = editor.scene().clone();
    let first = request(&editor.engine, Some(&bridge));
    let mut c = CompositeController::default();
    c.request(first.clone());
    let work = c.take_work().unwrap();
    let late = work.run();
    assert!(late[0].1.as_ref().unwrap().is_some());
    editor
        .transaction(|s| {
            s.update_object(&"frame".into(), |o| {
                if let Shape::Frame(f) = &mut o.shape {
                    f.corner_radii = Some([0., 35., 0., 30.].into());
                }
            })
        })
        .unwrap();
    let next = request(&editor.engine, Some(&bridge));
    assert!(!next.compatible(&first, &"text".into()));
    c.request(next.clone());
    assert!(work.cancel.load(Ordering::Relaxed));
    c.finish(first, late);
    assert!(c.frame(&"text".into()).is_none());
    let fresh = c.take_work().unwrap();
    c.finish(next, fresh.run());
    assert!(c.frame(&"text".into()).is_some());
    assert!(editor.undo());
    assert_eq!(editor.scene(), &before);
    let restored = request(&editor.engine, Some(&bridge));
    c.request(restored.clone());
    assert!(c.frame(&"text".into()).is_none());
    let fresh = c.take_work().unwrap();
    c.finish(restored, fresh.run());
    assert!(c.frame(&"text".into()).is_some());
}

#[test]
fn upright_glyphs_under_affine_ancestor_clips_keep_real_mask_geometry() {
    let (_app, bridge) =
        gpui_flow_text::GpuiTextRasterizer::application(gpui_kit::platform::current_platform(true));
    let mut s = scene();
    // Binary-exact shear cancellation keeps Text strictly axis aligned.
    let parent = Affine::new(1., 0.25, 0., 1., 0., 0.);
    s.update_object(&"frame".into(), |o| {
        o.transform = Affine::translation(5., 5.) * parent
    })
    .unwrap();
    s.update_object(&"text".into(), |o| o.transform = parent.inverse().unwrap())
        .unwrap();
    let e = SceneEngine::new(s).unwrap();
    let f = render(&e, Some(&bridge)).unwrap().unwrap();
    assert!(f.premul_rgba.as_chunks::<4>().0.iter().any(|p| p[3] > 0));
}

#[test]
fn ellipsis_edit_invalidates_glyph_jobs_without_mutating_source_text() {
    let (_app, bridge) =
        gpui_flow_text::GpuiTextRasterizer::application(gpui_kit::platform::current_platform(true));
    let mut e = SceneEngine::new(scene()).unwrap();
    let first = request(&e, Some(&bridge));
    let mut c = CompositeController::default();
    c.request(first.clone());
    let work = c.take_work().unwrap();
    let late = work.run();
    let content = if let Shape::Text(t) = &e.scene().get(&"text".into()).unwrap().object.shape {
        t.content.clone()
    } else {
        unreachable!()
    };
    e.transaction(|s| {
        s.update_object(&"text".into(), |o| {
            if let Shape::Text(t) = &mut o.shape {
                t.overflow = display::TextOverflow::Ellipsis;
            }
        })
    })
    .unwrap();
    let next = request(&e, Some(&bridge));
    assert!(!next.compatible(&first, &"text".into()));
    c.request(next.clone());
    assert!(work.cancel.load(Ordering::Relaxed));
    c.finish(first, late);
    assert!(c.frame(&"text".into()).is_none());
    let fresh = c.take_work().unwrap();
    c.finish(next, fresh.run());
    assert!(c.frame(&"text".into()).is_some());
    let Shape::Text(t) = &e.scene().get(&"text".into()).unwrap().object.shape else {
        unreachable!()
    };
    assert_eq!(t.content, content);
}
