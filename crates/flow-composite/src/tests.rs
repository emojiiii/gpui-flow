use super::*;
use gpui_flow_scene::{
    Affine, BlendMode, Bounds, Color, DisplayObject, MaskMode, Point, Scene, Size, Style, display,
};
use std::sync::atomic::AtomicBool;
fn group(id: &str, opacity: f32) -> DisplayObject {
    DisplayObject::new(id, display::Group::new()).with_style(Style {
        opacity,
        blend_mode: BlendMode::Normal,
        ..Default::default()
    })
}
fn rect(id: &str, x: f32, color: Color) -> DisplayObject {
    DisplayObject::new(id, display::Rect::new(20., 20.))
        .with_transform(Affine::translation(x, 0.))
        .with_style(Style::fill(color))
}
fn source(scene: &Scene) -> DomainSource {
    match scene_units(scene, CompositeLimits::default())
        .unwrap()
        .remove(0)
    {
        SceneUnit::Domain(d) => *d,
        u => panic!("{u:?}"),
    }
}
fn viewport() -> RasterViewport {
    RasterViewport {
        world_to_screen: Affine::IDENTITY,
        canvas_bounds: Bounds::new(Point::ZERO, Size::new(100., 100.)),
        content_mask: Bounds::new(Point::ZERO, Size::new(100., 100.)),
        device_scale: 1.,
    }
}
fn render(scene: &Scene) -> CpuFrame {
    rasterize(
        &source(scene),
        viewport(),
        &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap()
}
fn at(f: &CpuFrame, x: u32, y: u32) -> [u8; 4] {
    let x = x - (f.screen_bounds.origin.x as u32);
    let y = y - (f.screen_bounds.origin.y as u32);
    f.premul_rgba[((y * f.width + x) * 4) as usize..][..4]
        .try_into()
        .unwrap()
}
fn insert(scene: &mut Scene, o: DisplayObject, parent: &str) {
    scene.insert(o, Some(parent.into()), None).unwrap();
}
#[test]
fn group_opacity_is_once_across_overlapping_children() {
    let mut s = Scene::new();
    s.insert(group("g", 0.5), None, None).unwrap();
    insert(&mut s, rect("a", 0., Color::hex(0xff0000)), "g");
    insert(&mut s, rect("b", 10., Color::hex(0x0000ff)), "g");
    let f = render(&s);
    assert_eq!(at(&f, 5, 5), [128, 0, 0, 128]);
    assert_eq!(at(&f, 15, 5), [0, 0, 128, 128]);
}
#[test]
fn nested_opacity_multiplies_once_per_group() {
    let mut s = Scene::new();
    s.insert(group("a", 0.5), None, None).unwrap();
    insert(&mut s, group("b", 0.5), "a");
    insert(&mut s, rect("r", 0., Color::WHITE), "b");
    assert_eq!(at(&render(&s), 5, 5), [64, 64, 64, 64]);
}
#[test]
fn multiply_and_screen_read_the_internal_backdrop() {
    for (blend, expected) in [
        (BlendMode::Multiply, [0, 0, 0, 255]),
        (BlendMode::Screen, [255, 0, 255, 255]),
    ] {
        let mut s = Scene::new();
        s.insert(group("g", 1.), None, None).unwrap();
        insert(&mut s, rect("a", 0., Color::hex(0xff0000)), "g");
        let mut b = rect("b", 0., Color::hex(0x0000ff));
        b.style.blend_mode = blend;
        insert(&mut s, b, "g");
        assert_eq!(at(&render(&s), 5, 5), expected);
    }
}
#[test]
fn alpha_mask_is_hidden_and_masks_following_siblings_once() {
    let mut s = Scene::new();
    s.insert(group("g", 1.), None, None).unwrap();
    let mut mask = rect("mask", 0., Color::WHITE).with_mask(MaskMode::Alpha);
    mask.style.opacity = 0.5;
    insert(&mut s, mask, "g");
    insert(&mut s, rect("a", 10., Color::hex(0xff0000)), "g");
    insert(&mut s, rect("b", 10., Color::hex(0x0000ff)), "g");
    let f = render(&s);
    assert_eq!(at(&f, 5, 5), [0, 0, 0, 0]);
    assert_eq!(at(&f, 15, 5), [0, 0, 128, 128]);
    assert_eq!(at(&f, 25, 5), [0, 0, 0, 0]);
}
#[test]
fn external_blend_rejected_and_unsupported_descendant_rejects_whole_domain() {
    let mut s = Scene::new();
    let mut r = rect("r", 0., Color::WHITE);
    r.style.blend_mode = BlendMode::Multiply;
    s.insert(r, None, None).unwrap();
    assert!(matches!(
        &scene_units(&s, CompositeLimits::default()).unwrap()[0],
        SceneUnit::Rejected(CompositeIssue {
            error: CompositeError::UnsupportedExternalBackdrop,
            ..
        })
    ));
    let mut s = Scene::new();
    s.insert(group("g", 0.5), None, None).unwrap();
    insert(
        &mut s,
        DisplayObject::new("t", display::Text::new("hello", Size::new(40., 20.))),
        "g",
    );
    assert!(matches!(
        &scene_units(&s, CompositeLimits::default()).unwrap()[0],
        SceneUnit::Rejected(CompositeIssue {
            error: CompositeError::UnsupportedShape(_),
            ..
        })
    ));
}
#[test]
fn pixel_budget_failure_returns_no_partial_frame() {
    let mut s = Scene::new();
    s.insert(group("g", 1.), None, None).unwrap();
    insert(&mut s, rect("r", 0., Color::WHITE), "g");
    let limits = CompositeLimits {
        max_surface_bytes: 100,
        ..Default::default()
    };
    let mut budget = PixelBudget::new(limits, 0).unwrap();
    assert!(matches!(
        rasterize(
            &source(&s),
            viewport(),
            &mut budget,
            &AtomicBool::new(false)
        ),
        Err(CompositeError::WorkingSetLimit)
    ));
    assert_eq!(budget.retained_bytes(), 0);
}
#[test]
fn normal_group_isolates_while_pass_through_shares_backdrop() {
    for (blend, expected) in [
        (BlendMode::Normal, [0, 0, 255, 255]),
        (BlendMode::PassThrough, [0, 0, 0, 255]),
    ] {
        let mut s = Scene::new();
        s.insert(group("outer", 1.), None, None).unwrap();
        insert(&mut s, rect("red", 0., Color::hex(0xff0000)), "outer");
        let mut inner = group("inner", 1.);
        inner.style.blend_mode = blend;
        insert(&mut s, inner, "outer");
        let mut blue = rect("blue", 0., Color::hex(0x0000ff));
        blue.style.blend_mode = BlendMode::Multiply;
        insert(&mut s, blue, "inner");
        assert_eq!(at(&render(&s), 5, 5), expected);
    }
}
#[test]
fn opaque_mask_blend_flush_uses_parent_backdrop_but_translucent_mask_isolates_run() {
    for (opacity, expected) in [(1., [0, 0, 0, 255]), (0.5, [127, 0, 128, 255])] {
        let mut s = Scene::new();
        s.insert(group("g", 1.), None, None).unwrap();
        insert(&mut s, rect("back", 0., Color::hex(0xff0000)), "g");
        let mut mask = rect("m", 0., Color::WHITE).with_mask(MaskMode::Alpha);
        mask.style.opacity = opacity;
        insert(&mut s, mask, "g");
        let mut blue = rect("blue", 0., Color::hex(0x0000ff));
        blue.style.blend_mode = BlendMode::Multiply;
        insert(&mut s, blue, "g");
        assert_eq!(at(&render(&s), 5, 5), expected);
    }
}
#[test]
fn new_mask_ends_previous_run_and_equal_z_preserves_insertion_order() {
    let mut s = Scene::new();
    s.insert(group("g", 1.), None, None).unwrap();
    insert(
        &mut s,
        rect("m1", 0., Color::WHITE).with_mask(MaskMode::Alpha),
        "g",
    );
    insert(&mut s, rect("red", 0., Color::hex(0xff0000)), "g");
    insert(
        &mut s,
        rect("m2", 10., Color::WHITE).with_mask(MaskMode::Alpha),
        "g",
    );
    insert(&mut s, rect("blue", 10., Color::hex(0x0000ff)), "g");
    let f = render(&s);
    assert_eq!(at(&f, 5, 5), [255, 0, 0, 255]);
    assert_eq!(at(&f, 15, 5), [0, 0, 255, 255]);
    assert_eq!(at(&f, 25, 5), [0, 0, 255, 255]);
}
#[test]
fn primitive_layer_alpha_is_once() {
    let mut s = Scene::new();
    let mut r = rect("r", 0., Color::hex(0xff0000));
    r.style.fill_layers.push(Color::hex(0x0000ff).into());
    r.style.opacity = 0.5;
    s.insert(r, None, None).unwrap();
    assert_eq!(at(&render(&s), 5, 5), [0, 0, 128, 128]);
}
#[test]
fn fractional_dpr_grid_affine_and_inherited_rounded_clips() {
    let mut s = Scene::new();
    let mut frame = DisplayObject::new("frame", display::Frame::new(40., 40.));
    if let gpui_flow_scene::Shape::Frame(f) = &mut frame.shape {
        f.corner_radius = 12.;
    }
    s.insert(frame, None, None).unwrap();
    insert(&mut s, group("g", 0.5), "frame");
    let mut r = rect("r", 0., Color::WHITE);
    r.transform = Affine::translation(30., 0.) * Affine::scale(-2., 2.);
    insert(&mut s, r, "g");
    let unit = scene_units(&s, CompositeLimits::default())
        .unwrap()
        .into_iter()
        .find_map(|u| {
            if let SceneUnit::Domain(d) = u {
                Some(d)
            } else {
                None
            }
        })
        .unwrap();
    let mut v = viewport();
    v.device_scale = 1.5;
    v.world_to_screen = Affine::translation(0.3, 0.2);
    let f = rasterize(
        &unit,
        v,
        &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap();
    assert_eq!((f.screen_bounds.origin.x * 1.5).fract(), 0.);
    assert_eq!((f.screen_bounds.origin.y * 1.5).fract(), 0.);
    assert_eq!(&f.premul_rgba[..4], &[0, 0, 0, 0]);
    assert!(f.premul_rgba.as_chunks::<4>().0.iter().any(|p| p[3] == 128));
}
#[test]
fn gradient_mask_consumes_pixel_alpha() {
    let mut s = Scene::new();
    s.insert(group("g", 1.), None, None).unwrap();
    let mut m = rect("m", 0., Color::WHITE).with_mask(MaskMode::Alpha);
    m.style.fill = Some(gpui_flow_scene::Paint::LinearGradient {
        start: Point::ZERO,
        end: Point::new(20., 0.),
        stops: vec![
            gpui_flow_scene::GradientStop {
                offset: 0.,
                color: Color::TRANSPARENT,
            },
            gpui_flow_scene::GradientStop {
                offset: 1.,
                color: Color::WHITE,
            },
        ],
    });
    insert(&mut s, m, "g");
    insert(&mut s, rect("r", 0., Color::WHITE), "g");
    let f = render(&s);
    assert!(at(&f, 2, 5)[3] < at(&f, 17, 5)[3]);
}
#[test]
fn empty_offscreen_cancelled_and_work_limited_outputs_are_atomic() {
    let mut s = Scene::new();
    s.insert(group("g", 1.), None, None).unwrap();
    let mut b = PixelBudget::new(CompositeLimits::default(), 0).unwrap();
    assert!(
        rasterize(&source(&s), viewport(), &mut b, &AtomicBool::new(false))
            .unwrap()
            .is_none()
    );
    insert(&mut s, rect("r", 200., Color::WHITE), "g");
    assert!(
        rasterize(&source(&s), viewport(), &mut b, &AtomicBool::new(false))
            .unwrap()
            .is_none()
    );
    assert!(matches!(
        rasterize(&source(&s), viewport(), &mut b, &AtomicBool::new(true)),
        Err(CompositeError::Superseded)
    ));
    let mut v = viewport();
    v.world_to_screen = Affine::translation(-200., 0.);
    let mut b = PixelBudget::new(
        CompositeLimits {
            max_surface_writes: 1,
            ..Default::default()
        },
        0,
    )
    .unwrap();
    assert!(matches!(
        rasterize(&source(&s), v, &mut b, &AtomicBool::new(false)),
        Err(CompositeError::WorkLimit)
    ));
    assert_eq!(b.retained_bytes(), 0);
}
#[test]
fn zero_scale_and_excessive_control_points_are_explicit_errors() {
    let mut s = Scene::new();
    s.insert(group("g", 1.), None, None).unwrap();
    insert(&mut s, rect("r", 0., Color::WHITE), "g");
    let mut v = viewport();
    v.device_scale = 0.;
    let mut b = PixelBudget::new(CompositeLimits::default(), 0).unwrap();
    assert!(matches!(
        rasterize(&source(&s), v, &mut b, &AtomicBool::new(false)),
        Err(CompositeError::UnsupportedTransform)
    ));
    let units = scene_units(
        &s,
        CompositeLimits {
            max_nodes: 1,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(matches!(
        &units[0],
        SceneUnit::Rejected(CompositeIssue {
            error: CompositeError::InputLimit,
            ..
        })
    ));
}
#[test]
fn domain_key_equality_sees_child_paint_and_mask_reordering() {
    let mut s = Scene::new();
    s.insert(group("g", 1.), None, None).unwrap();
    insert(&mut s, rect("a", 0., Color::WHITE), "g");
    let first = source(&s);
    s.update_object(&"a".into(), |o| o.style.fill = Some(Color::BLACK.into()))
        .unwrap();
    assert_ne!(first, source(&s));
}
