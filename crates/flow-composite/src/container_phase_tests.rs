use super::*;
use gpui_flow_scene::{
    Affine, BlendMode, Bounds, Color, ContainerPaintOrder as Order, DisplayObject, MaskMode,
    PaintPhase, Point, Scene, Shape, Size, Stroke, Style, display,
};
use std::sync::atomic::AtomicBool;

fn insert(s: &mut Scene, o: DisplayObject, parent: &str) {
    s.insert(o, Some(parent.into()), None).unwrap();
}
fn rectangle(id: &str, width: f32, height: f32, color: Color) -> DisplayObject {
    DisplayObject::new(id, display::Rect::new(width, height)).with_style(Style::fill(color))
}
fn viewport() -> RasterViewport {
    RasterViewport {
        world_to_screen: Affine::IDENTITY,
        canvas_bounds: Bounds::new(Point::ZERO, Size::new(100., 100.)),
        content_mask: Bounds::new(Point::ZERO, Size::new(100., 100.)),
        device_scale: 1.,
    }
}
fn source(s: &Scene) -> DomainSource {
    let units = scene_units(s, CompositeLimits::default()).unwrap();
    assert_eq!(units.len(), 1);
    match units.into_iter().next().unwrap() {
        SceneUnit::Domain(d) => *d,
        other => panic!("{other:?}"),
    }
}
fn render(s: &Scene) -> CpuFrame {
    rasterize(
        &source(s),
        viewport(),
        &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap()
}
fn pixel(f: &CpuFrame, x: f32, y: f32, dpr: f32) -> [u8; 4] {
    let x = ((x - f.screen_bounds.origin.x) * dpr).floor() as usize;
    let y = ((y - f.screen_bounds.origin.y) * dpr).floor() as usize;
    f.premul_rgba[(y * f.width as usize + x) * 4..][..4]
        .try_into()
        .unwrap()
}
fn set_order(o: &mut DisplayObject, order: Order) {
    match &mut o.shape {
        Shape::Box(s) => s.paint_order = order,
        Shape::Frame(s) => s.paint_order = order,
        _ => panic!("container expected"),
    }
}
fn fixture(shape: Shape) -> Scene {
    let mut s = Scene::new();
    s.insert(
        DisplayObject::new("container", shape)
            .with_transform(Affine::translation(10., 10.))
            .with_style(Style {
                fill: Some(Color::rgb(255, 0, 0).into()),
                stroke: Some(Stroke::new(Color::rgb(0, 255, 0), 8.)),
                opacity: 0.5,
                ..Default::default()
            }),
        None,
        None,
    )
    .unwrap();
    insert(
        &mut s,
        rectangle("child", 20., 20., Color::rgb(0, 0, 255)),
        "container",
    );
    s
}
#[test]
fn after_fill_pixels_preserve_legacy_and_apply_complete_container_opacity_once() {
    for shape in [
        display::Box::new(30., 30.).into(),
        display::Frame::new(30., 30.).into(),
    ] {
        let mut s = fixture(shape);
        assert_eq!(pixel(&render(&s), 12., 15., 1.), [0, 0, 128, 128]);
        s.update_object(&"container".into(), |o| {
            set_order(o, Order::StrokeAfterChildren)
        })
        .unwrap();
        let f = render(&s);
        assert_eq!(pixel(&f, 12., 15., 1.), [0, 128, 0, 128]);
        assert_eq!(pixel(&f, 8., 15., 1.), [0, 128, 0, 128]);
        assert_eq!(pixel(&f, 34., 25., 1.), [128, 0, 0, 128]);
        assert_eq!(pixel(&f, 25., 25., 1.), [0, 0, 128, 128]);
    }
}
#[test]
fn translucent_layers_are_submitted_once_in_fill_child_stroke_order() {
    let mut s = fixture(
        display::Frame::new(30., 30.)
            .with_paint_order(Order::StrokeAfterChildren)
            .into(),
    );
    let colors = [
        Color::rgba(255, 0, 0, 128),
        Color::rgba(0, 255, 0, 128),
        Color::rgba(0, 0, 255, 128),
        Color::rgba(255, 255, 0, 128),
        Color::rgba(0, 0, 0, 64),
    ];
    s.update_object(&"container".into(), |o| {
        o.style.fill = Some(Color::TRANSPARENT.into());
        o.style.fill_layers = vec![
            colors[0].into(),
            Color::TRANSPARENT.into(),
            colors[1].into(),
        ];
        o.style.stroke = Some(Stroke::new(Color::TRANSPARENT, 8.));
        o.style.stroke_layers = vec![Stroke::new(colors[3], 8.), Stroke::new(colors[4], 6.)];
    })
    .unwrap();
    s.update_object(&"child".into(), |o| o.style.fill = Some(colors[2].into()))
        .unwrap();
    let mut expected = [0f64; 4];
    for c in colors {
        let alpha = c.a as f64 / 255.;
        for (out, value) in expected[..3].iter_mut().zip([c.r, c.g, c.b]) {
            *out = value as f64 / 255. * alpha + *out * (1. - alpha);
        }
        expected[3] = alpha + expected[3] * (1. - alpha);
    }
    let actual = pixel(&render(&s), 11., 18., 1.);
    for (actual, expected) in actual.into_iter().zip(expected) {
        assert!(
            (actual as f64 - expected * 255. * 0.5).abs() <= 2.,
            "{actual} vs {expected}"
        );
    }
}
#[test]
fn native_phase_stream_is_nested_stable_and_domains_replace_all_their_phases() {
    let mut s = Scene::new();
    s.insert(
        DisplayObject::new(
            "outer",
            display::Frame::new(50., 50.).with_paint_order(Order::StrokeAfterChildren),
        ),
        None,
        None,
    )
    .unwrap();
    insert(
        &mut s,
        DisplayObject::new(
            "inner",
            display::Box::new(30., 30.).with_paint_order(Order::StrokeAfterChildren),
        ),
        "outer",
    );
    insert(&mut s, rectangle("child", 10., 10., Color::WHITE), "inner");
    let phase = |id: &str, phase| SceneUnit::NativePhase {
        id: id.into(),
        phase,
    };
    assert_eq!(
        scene_units(&s, CompositeLimits::default()).unwrap(),
        vec![
            phase("outer", PaintPhase::Fill),
            phase("inner", PaintPhase::Fill),
            SceneUnit::Native("child".into()),
            phase("inner", PaintPhase::Stroke),
            phase("outer", PaintPhase::Stroke)
        ]
    );
    s.update_object(&"inner".into(), |o| o.style.opacity = 0.5)
        .unwrap();
    let units = scene_units(&s, CompositeLimits::default()).unwrap();
    assert_eq!(units.len(), 3);
    assert_eq!(units[0], phase("outer", PaintPhase::Fill));
    assert!(
        matches!(&units[1], SceneUnit::Domain(d) if d.ids() == vec![&"inner".into(), &"child".into()])
    );
    assert_eq!(units[2], phase("outer", PaintPhase::Stroke));
    s.update_object(&"outer".into(), |o| {
        o.style.blend_mode = BlendMode::Multiply
    })
    .unwrap();
    assert!(matches!(
        scene_units(&s, CompositeLimits::default())
            .unwrap()
            .as_slice(),
        [SceneUnit::Rejected(_)]
    ));
}
#[test]
fn nested_after_fill_containers_paint_parent_stroke_last() {
    let mut s = fixture(
        display::Frame::new(30., 30.)
            .with_paint_order(Order::StrokeAfterChildren)
            .into(),
    );
    s.update_object(&"child".into(), |o| {
        o.shape = display::Box::new(20., 20.)
            .with_paint_order(Order::StrokeAfterChildren)
            .into();
        o.style.stroke = Some(Stroke::new(Color::WHITE, 6.));
    })
    .unwrap();
    insert(
        &mut s,
        rectangle("grandchild", 20., 20., Color::BLACK),
        "child",
    );
    let f = render(&s);
    assert_eq!(pixel(&f, 11., 18., 1.), [0, 128, 0, 128]);
    assert_eq!(pixel(&f, 28., 18., 1.), [128, 128, 128, 128]);
    assert_eq!(pixel(&f, 20., 20., 1.), [0, 0, 0, 128]);
}
#[test]
fn container_alpha_mask_includes_late_strokes_after_clipped_children() {
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
    insert(
        &mut s,
        DisplayObject::new(
            "mask",
            display::Frame::new(30., 30.).with_paint_order(Order::StrokeAfterChildren),
        )
        .with_mask(MaskMode::Alpha)
        .with_transform(Affine::translation(10., 10.))
        .with_style(Style {
            fill: None,
            stroke: Some(Stroke::new(Color::rgba(255, 255, 255, 128), 8.)),
            ..Default::default()
        }),
        "g",
    );
    insert(
        &mut s,
        rectangle("mask-child", 20., 20., Color::rgba(255, 255, 255, 128)),
        "mask",
    );
    insert(
        &mut s,
        rectangle("content", 60., 60., Color::rgb(255, 0, 0)),
        "g",
    );
    let f = render(&s);
    assert_eq!(pixel(&f, 8., 18., 1.), [128, 0, 0, 128]);
    let p = pixel(&f, 12., 18., 1.);
    assert!((p[3] as i32 - 192).abs() <= 1);
    assert_eq!(p[0], p[3]);
    assert_eq!(pixel(&f, 25., 25., 1.), [128, 0, 0, 128]);
}
#[test]
fn child_mask_run_never_masks_the_containers_late_stroke() {
    let mut s = fixture(
        display::Frame::new(30., 30.)
            .with_paint_order(Order::StrokeAfterChildren)
            .into(),
    );
    s.update_object(&"child".into(), |o| {
        o.shape = display::Group::new().into();
        o.style.fill = None;
    })
    .unwrap();
    insert(
        &mut s,
        rectangle("mask", 10., 10., Color::WHITE)
            .with_transform(Affine::translation(10., 10.))
            .with_mask(MaskMode::Alpha),
        "child",
    );
    insert(&mut s, rectangle("masked", 30., 30., Color::BLACK), "child");
    let f = render(&s);
    assert_eq!(pixel(&f, 12., 18., 1.), [0, 128, 0, 128]);
    assert_eq!(pixel(&f, 25., 25., 1.), [0, 0, 0, 128]);
}
#[test]
fn phase_only_edit_hard_invalidates_root_and_nested_domain_placement() {
    let mut s = fixture(display::Frame::new(30., 30.).into());
    let a = source(&s);
    s.update_object(&"container".into(), |o| {
        set_order(o, Order::StrokeAfterChildren)
    })
    .unwrap();
    let b = source(&s);
    assert!(!a.placement_eq(&b));
    s.update_object(&"child".into(), |o| {
        o.shape = display::Box::new(20., 20.).into()
    })
    .unwrap();
    let c = source(&s);
    s.update_object(&"child".into(), |o| {
        set_order(o, Order::StrokeAfterChildren)
    })
    .unwrap();
    assert!(!c.placement_eq(&source(&s)));
}
#[test]
fn late_stroke_bounds_survive_offscreen_centerline_and_child_culling() {
    let mut s = fixture(
        display::Frame::new(10., 30.)
            .with_paint_order(Order::StrokeAfterChildren)
            .into(),
    );
    s.update_object(&"container".into(), |o| {
        o.transform = Affine::translation(-12., 10.)
    })
    .unwrap();
    let f = render(&s);
    assert_eq!(pixel(&f, 0., 20., 1.), [0, 128, 0, 128]);
    assert!(f.width <= 3);
}
#[test]
fn late_stroke_work_is_charged_and_failure_rolls_back_budget() {
    let s = fixture(
        display::Frame::new(30., 30.)
            .with_paint_order(Order::StrokeAfterChildren)
            .into(),
    );
    let limits = CompositeLimits {
        max_surface_writes: 500,
        ..Default::default()
    };
    let mut budget = PixelBudget::new(limits, 16).unwrap();
    assert_eq!(
        rasterize(
            &source(&s),
            viewport(),
            &mut budget,
            &AtomicBool::new(false)
        )
        .unwrap_err(),
        CompositeError::WorkLimit
    );
    assert_eq!(budget.retained_bytes(), 16);
}

#[test]
fn late_strokes_keep_ancestor_clip_and_pass_through_child_backdrop_at_fractional_dpr() {
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
    insert(
        &mut s,
        rectangle("yellow", 70., 70., Color::rgb(255, 255, 0)),
        "g",
    );
    insert(
        &mut s,
        DisplayObject::new("outer", display::Frame::new(27., 45.).rounded(2.))
            .with_transform(Affine::translation(9., 5.))
            .with_style(Style {
                fill: None,
                ..Default::default()
            }),
        "g",
    );
    insert(
        &mut s,
        DisplayObject::new(
            "inner",
            display::Frame::new(30., 30.)
                .rounded(3.)
                .with_paint_order(Order::StrokeAfterChildren),
        )
        .with_transform(Affine::translation(1., 5.))
        .with_style(Style {
            fill: None,
            stroke: Some(Stroke::new(Color::rgb(0, 255, 0), 8.)),
            ..Default::default()
        }),
        "outer",
    );
    let mut child = rectangle("multiply", 80., 80., Color::rgb(0, 0, 255))
        .with_transform(Affine::translation(-10., -10.));
    child.style.blend_mode = BlendMode::Multiply;
    insert(&mut s, child, "inner");
    let mut v = viewport();
    v.device_scale = 1.5;
    let f = rasterize(
        &source(&s),
        v,
        &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap();
    assert_eq!(pixel(&f, 7., 20., 1.5), [255, 255, 0, 255]);
    assert_eq!(pixel(&f, 9.5, 20., 1.5), [0, 255, 0, 255]);
    assert_eq!(pixel(&f, 12., 20., 1.5), [0, 255, 0, 255]);
    assert_eq!(pixel(&f, 25., 20., 1.5), [0, 0, 0, 255]);
    assert_eq!(pixel(&f, 37., 20., 1.5), [255, 255, 0, 255]);
}

#[test]
fn phase_order_does_not_add_geometry_or_surface_work() {
    let mut s = fixture(display::Frame::new(30., 30.).into());
    let mut before = PixelBudget::new(CompositeLimits::default(), 0).unwrap();
    let a = rasterize(
        &source(&s),
        viewport(),
        &mut before,
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap();
    s.update_object(&"container".into(), |o| {
        set_order(o, Order::StrokeAfterChildren)
    })
    .unwrap();
    let mut after = PixelBudget::new(CompositeLimits::default(), 0).unwrap();
    let b = rasterize(&source(&s), viewport(), &mut after, &AtomicBool::new(false))
        .unwrap()
        .unwrap();
    assert_eq!(a.screen_bounds, b.screen_bounds);
    assert_eq!(before.surface_writes(), after.surface_writes());
    assert_eq!(before.peak_bytes(), after.peak_bytes());
    assert_ne!(a.premul_rgba, b.premul_rgba);
}

#[test]
fn late_parent_stroke_finishes_before_the_next_root_on_owned_canvas() {
    let mut s = fixture(
        display::Frame::new(30., 30.)
            .with_paint_order(Order::StrokeAfterChildren)
            .into(),
    );
    s.update_object(&"container".into(), |o| o.style.opacity = 1.)
        .unwrap();
    s.insert(
        rectangle("later", 15., 30., Color::WHITE).with_transform(Affine::translation(5., 10.)),
        None,
        None,
    )
    .unwrap();
    let units = scene_units(&s, CompositeLimits::default()).unwrap();
    assert!(
        matches!(&units[units.len() - 2], SceneUnit::NativePhase { id, phase: PaintPhase::Stroke } if id.as_str() == "container")
    );
    assert_eq!(units.last(), Some(&SceneUnit::Native("later".into())));
    let source = owned_canvas(
        &s,
        CanvasBackdrop {
            color: Color::BLACK,
            grid: None,
        },
        CompositeLimits::default(),
    )
    .unwrap();
    let f = rasterize(
        &source,
        viewport(),
        &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap();
    assert_eq!(pixel(&f, 12., 20., 1.), [255; 4]);
    assert_eq!(pixel(&f, 38., 20., 1.), [0, 255, 0, 255]);
    assert_eq!(pixel(&f, 25., 20., 1.), [0, 0, 255, 255]);
}
