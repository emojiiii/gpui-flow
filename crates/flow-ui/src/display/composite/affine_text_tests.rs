use super::*;
use gpui_flow_scene::{
    Affine, Bounds, Color, DisplayObject, Point, Scene, Shape, Size, Style, display,
};
fn scene(m: Affine) -> Scene {
    let mut s = Scene::new();
    let mut t = display::Text::new("Affine ffi café\n第二行 e\u{301}", Size::new(100., 52.));
    t.font_family = "DejaVu Sans".into();
    t.font_size = 16.;
    t.padding = [4.; 4];
    t.italic = true;
    t.decoration = display::TextDecoration::Underline;
    t.overflow = display::TextOverflow::Ellipsis;
    s.insert(
        DisplayObject::new("text", t)
            .with_style(Style::fill(Color::WHITE))
            .with_transform(m),
        None,
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
    let b = Bounds::new(Point::ZERO, Size::new(240., 200.));
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
#[test]
fn affine_text_rotation_reflection_shear_and_nonuniform_scales_use_real_ink() {
    let (_app, bridge) = gpui_flow_text::GpuiTextRasterizer::application(
        crate::display::native_text_test_platform(),
    );
    for m in [
        Affine::translation(90., 20.) * Affine::rotation(37.),
        Affine::translation(150., 20.) * Affine::scale(-1., 1.),
        Affine::translation(20., 20.) * Affine::scale(1.8, 0.65),
        Affine::translation(20., 20.) * Affine::skew(20., 10.),
    ] {
        let e = SceneEngine::new(scene(m)).unwrap();
        let f = render(&e, Some(&bridge)).unwrap().unwrap();
        let ink = f
            .premul_rgba
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[3] > 0)
            .count();
        assert!(ink > 150, "{m:?} {ink}");
        assert!(
            ink < (f.width * f.height) as usize / 2,
            "not a box substitute"
        );
        let inner = Bounds::new(Point::new(4., 4.), Size::new(92., 44.));
        let inverse = m.inverse().unwrap();
        for y in 0..f.height {
            for x in 0..f.width {
                if f.premul_rgba[((y * f.width + x) * 4 + 3) as usize] == 0 {
                    continue;
                }
                let local = inverse.transform_point(Point::new(
                    f.screen_bounds.origin.x + x as f32 + 0.5,
                    f.screen_bounds.origin.y + y as f32 + 0.5,
                ));
                assert!(
                    local.x >= inner.min_x() - 1.5
                        && local.x <= inner.max_x() + 1.5
                        && local.y >= inner.min_y() - 1.5
                        && local.y <= inner.max_y() + 1.5,
                    "own content clip: {local:?}"
                );
            }
        }
    }
}
#[test]
fn affine_text_keeps_bridge_and_style_capability_errors_explicit() {
    let (_app, bridge) = gpui_flow_text::GpuiTextRasterizer::application(
        crate::display::native_text_test_platform(),
    );
    let mut e =
        SceneEngine::new(scene(Affine::translation(80., 10.) * Affine::rotation(15.))).unwrap();
    assert!(matches!(
        render(&e, None),
        Err(CompositeError::TextBackendRequired(_))
    ));
    e.transaction(|s| {
        s.update_object(&"text".into(), |o| {
            o.style.stroke = Some(gpui_flow_scene::Stroke::new(Color::WHITE, 1.))
        })
    })
    .unwrap();
    assert!(matches!(
        render(&e, Some(&bridge)),
        Err(CompositeError::UnsupportedTextStyle(_))
    ));
}
#[test]
fn affine_text_transform_change_rejects_late_jobs_and_undo_restores_pixels() {
    let (_app, bridge) = gpui_flow_text::GpuiTextRasterizer::application(
        crate::display::native_text_test_platform(),
    );
    let mut e = gpui_flow_editor::SceneEditor::new(scene(
        Affine::translation(90., 10.) * Affine::rotation(25.),
    ))
    .unwrap();
    let baseline = render(&e.engine, Some(&bridge)).unwrap().unwrap();
    let original = e.scene().clone();
    let first = request(&e.engine, Some(&bridge));
    let mut c = CompositeController::default();
    c.request(first.clone());
    let work = c.take_work().unwrap();
    let late = work.run();
    e.transaction(|s| {
        s.update_object(&"text".into(), |o| {
            o.transform = Affine::translation(160., 15.) * Affine::scale(-1.2, 0.8)
        })
    })
    .unwrap();
    let next = request(&e.engine, Some(&bridge));
    assert!(!next.compatible(&first, &"text".into()));
    c.request(next.clone());
    assert!(work.cancel.load(Ordering::Relaxed));
    c.finish(first, late);
    assert!(c.frame(&"text".into()).is_none());
    let fresh = c.take_work().unwrap();
    c.finish(next, fresh.run());
    assert!(c.frame(&"text".into()).is_some());
    assert!(e.undo());
    assert_eq!(e.scene(), &original);
    let restored = render(&e.engine, Some(&bridge)).unwrap().unwrap();
    assert_eq!(baseline.premul_rgba, restored.premul_rgba);
    assert_eq!(baseline.screen_bounds, restored.screen_bounds);
}
#[test]
fn affine_text_oversampling_respects_existing_pixel_and_font_limits() {
    let (_app, bridge) = gpui_flow_text::GpuiTextRasterizer::application(
        crate::display::native_text_test_platform(),
    );
    let mut s = scene(Affine::translation(10., 10.) * Affine::scale(1., 1000.));
    s.update_object(&"text".into(), |o| {
        if let Shape::Text(t) = &mut o.shape {
            t.size = Size::new(10000., 52.);
            t.padding = [0.; 4];
        }
    })
    .unwrap();
    let e = SceneEngine::new(s).unwrap();
    assert!(render(&e, Some(&bridge)).is_err());
}
