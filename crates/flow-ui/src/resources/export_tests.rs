use super::*;
use crate::{display::composite_text::canvas_inputs, scene_view::export::export_images};
use gpui_flow_composite::{
    CanvasPixels, CompositeError, CompositeLimits, ImageInputs, SceneExportFormat, SceneExportPlan,
    SceneExportPreparation, SceneExportRequest, TextInputs,
};
use gpui_flow_scene::{Bounds, Color, DisplayObject, Point, Scene, Size, Style, display};
use std::sync::atomic::AtomicBool;

fn request() -> SceneExportRequest {
    SceneExportRequest {
        world_bounds: Bounds::new(Point::ZERO, Size::new(4., 4.)),
        scale: 1.,
        pixel_ratio: 1.,
        background: Color::TRANSPARENT,
        format: SceneExportFormat::Png,
    }
}
fn setup(two: bool) -> (SceneEngine, SceneResourceAdapter) {
    let mut s = Scene::new();
    for id in if two { vec!["a", "b"] } else { vec!["a"] } {
        s.insert(
            DisplayObject::new(id, display::Image::new("ready", Size::new(4., 4.))).with_style(
                Style {
                    fill: None,
                    ..Default::default()
                },
            ),
            None,
            None,
        )
        .unwrap();
    }
    let mut e = SceneEngine::new(s).unwrap();
    let mut r = SceneResourceAdapter::new(&mut e).unwrap();
    r.tracker.synchronize(&mut e).unwrap();
    (e, r)
}
fn image() -> Arc<RenderImage> {
    Arc::new(RenderImage::new(
        [image::Rgba([0, 0, 255, 255]), image::Rgba([255, 0, 0, 255])]
            .map(|c| {
                image::Frame::from_parts(
                    image::RgbaImage::from_pixel(2, 2, c),
                    0,
                    0,
                    image::Delay::from_numer_denom_ms(20, 1),
                )
            })
            .to_vec(),
    ))
}
fn load(e: &mut SceneEngine, r: &mut SceneResourceAdapter, id: &str, pixels: Arc<RenderImage>) {
    let t = r.tracker.ticket(e, e.handle(&id.into()).unwrap()).unwrap();
    r.settle(
        e,
        t,
        ResourceOutcome::Loaded,
        Some(SceneImagePresentation::Loaded(pixels)),
        &mut vec![],
    )
    .unwrap();
    r.synchronize_playbacks();
}
fn plan(e: &SceneEngine) -> SceneExportPlan {
    SceneExportPlan::new(e.scene(), request(), CompositeLimits::default()).unwrap()
}
fn inputs(
    p: &SceneExportPlan,
    e: &SceneEngine,
    r: Option<&SceneResourceAdapter>,
) -> Result<(ImageInputs, u64), CompositeError> {
    export_images(
        p.source(),
        e,
        r,
        p.limits(),
        32 * 1024 * 1024,
        32_000_000,
        &AtomicBool::new(false),
    )
}
fn rgba(p: &SceneExportPlan, e: &SceneEngine, r: &SceneResourceAdapter) -> Vec<u8> {
    let (im, work) = inputs(p, e, Some(r)).unwrap();
    let output = p
        .encode_with_inputs(
            &im,
            &TextInputs::default(),
            SceneExportPreparation {
                pixel_work: work,
                retained_bytes: 0,
            },
            &AtomicBool::new(false),
        )
        .unwrap();
    image::load_from_memory(&output.bytes)
        .unwrap()
        .into_rgba8()
        .into_raw()
}
#[test]
fn export_deduplicates_identical_decoded_frame_and_admits_before_copy() {
    let (mut e, mut r) = setup(true);
    let shared = image();
    load(&mut e, &mut r, "a", shared.clone());
    load(&mut e, &mut r, "b", shared);
    let p = plan(&e);
    let (im, work) = inputs(&p, &e, Some(&r)).unwrap();
    assert_eq!(im.byte_len(), 16);
    assert_eq!(work, 4);
    assert!(matches!(
        export_images(
            p.source(),
            &e,
            Some(&r),
            p.limits(),
            15,
            99,
            &AtomicBool::new(false)
        ),
        Err(CompositeError::WorkingSetLimit)
    ));
    assert!(matches!(
        export_images(
            p.source(),
            &e,
            Some(&r),
            p.limits(),
            99,
            3,
            &AtomicBool::new(false)
        ),
        Err(CompositeError::WorkLimit)
    ));
    assert!(matches!(
        export_images(
            p.source(),
            &e,
            Some(&r),
            CompositeLimits {
                max_dimension: 1,
                ..p.limits()
            },
            99,
            99,
            &AtomicBool::new(false)
        ),
        Err(CompositeError::TooLarge)
    ));
}
#[test]
fn export_uses_current_animation_frame_without_changing_playback_or_document() {
    let (mut e, mut r) = setup(false);
    load(&mut e, &mut r, "a", image());
    let p = plan(&e);
    let before = e.scene().to_json().unwrap();
    let revision = e.revision();
    assert_eq!(&rgba(&p, &e, &r)[..4], &[255, 0, 0, 255]);
    let h = e.handle(&"a".into()).unwrap();
    assert!(r.seek_image(&e, h, Duration::from_millis(20)));
    assert_eq!(&rgba(&p, &e, &r)[..4], &[0, 0, 255, 255]);
    assert_eq!(r.image_frame(&e, h), 1);
    assert_eq!(e.revision(), revision);
    assert_eq!(e.scene().to_json().unwrap(), before);
}
#[test]
fn export_missing_pending_failed_cancelled_and_stale_sources_never_fall_back() {
    let (mut e, mut r) = setup(false);
    let p = plan(&e);
    assert!(matches!(
        inputs(&p, &e, None),
        Err(CompositeError::ImagePending(_))
    ));
    assert!(matches!(
        inputs(&p, &e, Some(&r)),
        Err(CompositeError::ImagePending(_))
    ));
    let t = r
        .tracker
        .ticket(&e, e.handle(&"a".into()).unwrap())
        .unwrap();
    r.settle(
        &mut e,
        t,
        ResourceOutcome::Failed("fixture".into()),
        Some(SceneImagePresentation::Failed("fixture".into())),
        &mut vec![],
    )
    .unwrap();
    assert!(matches!(
        inputs(&p, &e, Some(&r)),
        Err(CompositeError::ImageUnavailable { .. })
    ));
    let (mut e, mut r) = setup(false);
    load(&mut e, &mut r, "a", image());
    let old = plan(&e);
    e.transaction(|s| {
        s.update_object(&"a".into(), |o| {
            if let Shape::Image(i) = &mut o.shape {
                i.source = "pending".into();
            }
        })
    })
    .unwrap();
    assert!(matches!(
        inputs(&old, &e, Some(&r)),
        Err(CompositeError::ImageUnavailable { .. })
    ));
    assert!(matches!(
        inputs(&plan(&e), &e, Some(&r)),
        Err(CompositeError::ImagePending(_))
    ));
    r.tracker.synchronize(&mut e).unwrap();
    let t = r
        .tracker
        .ticket(&e, e.handle(&"a".into()).unwrap())
        .unwrap();
    r.settle(
        &mut e,
        t,
        ResourceOutcome::Cancelled(ResourceCancellation::NotAuthorized),
        Some(SceneImagePresentation::Cancelled),
        &mut vec![],
    )
    .unwrap();
    assert!(matches!(
        inputs(&plan(&e), &e, Some(&r)),
        Err(CompositeError::ImageUnavailable { .. })
    ));
}
#[test]
fn export_rejects_cancelled_work_before_pixel_conversion() {
    let (mut e, mut r) = setup(false);
    load(&mut e, &mut r, "a", image());
    let p = plan(&e);
    assert!(matches!(
        export_images(
            p.source(),
            &e,
            Some(&r),
            p.limits(),
            99,
            99,
            &AtomicBool::new(true)
        ),
        Err(CompositeError::Superseded)
    ));
}
#[test]
fn export_canvas_resize_requires_current_ticket_and_keeps_owned_pixels_immutable() {
    let mut s = Scene::new();
    s.insert(
        DisplayObject::new("canvas", display::Canvas::new(4., 4.)).with_style(Style {
            fill: None,
            ..Default::default()
        }),
        None,
        None,
    )
    .unwrap();
    let mut e = SceneEngine::new(s).unwrap();
    let mut r = SceneResourceAdapter::new(&mut e).unwrap();
    r.prepare_canvases(&mut e).unwrap();
    let h = e.handle(&"canvas".into()).unwrap();
    r.edit_canvas(&mut e, h, |c| {
        c.write_pixels(
            &CanvasPixels::from_rgba(
                4,
                4,
                [0, 255, 0, 255].repeat(16),
                CompositeLimits::default(),
            )?,
            0,
            0,
        )
        .map(|_| ())
    })
    .unwrap();
    let snap = r.canvas_snapshot(&e, h).unwrap();
    assert_eq!(&rgba(&plan(&e), &e, &r)[..4], &[0, 255, 0, 255]);
    assert!(Arc::ptr_eq(&snap, &r.canvas_snapshot(&e, h).unwrap()));
    e.transaction(|s| {
        s.update_object(&"canvas".into(), |o| {
            if let Shape::Canvas(c) = &mut o.shape {
                c.size = Size::new(2., 2.);
            }
        })
    })
    .unwrap();
    assert!(matches!(
        inputs(&plan(&e), &e, Some(&r)),
        Err(CompositeError::ImageUnavailable { .. })
    ));
    r.prepare_canvases(&mut e).unwrap();
    assert_eq!(&rgba(&plan(&e), &e, &r)[..4], &[0, 0, 0, 0]);
}
#[test]
fn export_text_requires_real_font_bridge_and_does_not_fabricate_bounds_pixels() {
    let mut s = Scene::new();
    s.insert(
        DisplayObject::new("text", display::Text::new("real", Size::new(4., 4.)))
            .with_style(Style::fill(Color::WHITE)),
        None,
        None,
    )
    .unwrap();
    let p = SceneExportPlan::new(&s, request(), CompositeLimits::default()).unwrap();
    assert!(matches!(
        p.encode_with_text(&ImageInputs::default(), 0, &AtomicBool::new(false), |r| {
            canvas_inputs(r, None)
        }),
        Err(CompositeError::TextBackendRequired(_))
    ));
}

#[test]
fn scene_export_native_glyphs_preserve_case_paragraph_affine_and_font_epochs() {
    use crate::display::composite_text::TextBackendSnapshot;
    use gpui_flow_scene::Affine;
    let (_app, bridge) =
        gpui_flow_text::GpuiTextRasterizer::application(gpui_kit::platform::current_platform(true));
    let backend = TextBackendSnapshot::new(&bridge);
    let mut s = Scene::new();
    let mut text = display::Text::new("Straße\nnext", Size::new(140., 80.));
    text.text_case = display::TextCase::Upper;
    text.para_spacing = 5.;
    text.font_size = 20.;
    s.insert(
        DisplayObject::new("text", text)
            .with_transform(Affine::translation(15., 10.) * Affine::rotation(6.))
            .with_style(Style::fill(Color::WHITE)),
        None,
        None,
    )
    .unwrap();
    let canonical = s.to_json().unwrap();
    for dpr in [1., 2.] {
        let request = SceneExportRequest {
            world_bounds: Bounds::new(Point::ZERO, Size::new(190., 120.)),
            pixel_ratio: dpr,
            ..request()
        };
        let encode = |scene: &Scene, format| {
            let p = SceneExportPlan::new(
                scene,
                SceneExportRequest { format, ..request },
                CompositeLimits::default(),
            )
            .unwrap();
            p.encode_with_text(&ImageInputs::default(), 0, &AtomicBool::new(false), |r| {
                canvas_inputs(r, Some(&backend))
            })
            .unwrap()
        };
        let png = encode(&s, SceneExportFormat::Png);
        let webp = encode(&s, SceneExportFormat::WebPLossless);
        let pixels = image::load_from_memory(&png.bytes).unwrap().into_rgba8();
        let other = image::load_from_memory(&webp.bytes).unwrap().into_rgba8();
        assert_eq!(pixels, other);
        let ink = pixels.pixels().filter(|p| p[3] > 0).count();
        assert!(ink > 20 && ink < (pixels.width() * pixels.height() / 2) as usize);
        let mut explicit = s.clone();
        explicit
            .update_object(&"text".into(), |o| {
                if let Shape::Text(t) = &mut o.shape {
                    t.content = "STRASSE\nNEXT".into();
                    t.text_case = display::TextCase::None;
                }
            })
            .unwrap();
        assert_eq!(
            image::load_from_memory(&encode(&explicit, SceneExportFormat::Png).bytes)
                .unwrap()
                .into_rgba8(),
            pixels
        );
    }
    assert_eq!(s.to_json().unwrap(), canonical);
    bridge.invalidate();
    let p = SceneExportPlan::new(
        &s,
        SceneExportRequest {
            world_bounds: Bounds::new(Point::ZERO, Size::new(190., 120.)),
            ..request()
        },
        CompositeLimits::default(),
    )
    .unwrap();
    assert!(matches!(
        p.encode_with_text(&ImageInputs::default(), 0, &AtomicBool::new(false), |r| {
            canvas_inputs(r, Some(&backend))
        }),
        Err(CompositeError::Superseded)
    ));
}
