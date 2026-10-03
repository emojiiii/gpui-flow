use super::*;
use gpui_flow_scene::{
    Affine, BlendMode, Bounds, Color, DisplayObject, EraserMode, FillRule, MaskMode, Point, Scene,
    Size, Style, display,
};
use std::sync::atomic::AtomicBool;

fn rect(id: &str, x: f32, width: f32, color: Color) -> DisplayObject {
    DisplayObject::new(id, display::Rect::new(width, 30.))
        .with_transform(Affine::translation(x, 0.))
        .with_style(Style::fill(color))
}
fn scene() -> Scene {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("g", display::Group::new()).with_style(Style {
                blend_mode: BlendMode::Normal,
                ..Default::default()
            }),
            None,
            None,
        )
        .unwrap();
    scene
}
fn add(scene: &mut Scene, object: DisplayObject, parent: &str) {
    scene.insert(object, Some(parent.into()), None).unwrap();
}
fn source(scene: &Scene) -> DomainSource {
    capture_subtree(scene, &"g".into(), CompositeLimits::default()).unwrap()
}
fn raster(
    source: &DomainSource,
    limits: CompositeLimits,
) -> Result<Option<CpuFrame>, CompositeError> {
    let bounds = Bounds::new(Point::ZERO, Size::new(60., 40.));
    rasterize(
        source,
        RasterViewport {
            world_to_screen: Affine::IDENTITY,
            canvas_bounds: bounds,
            content_mask: bounds,
            device_scale: 1.,
        },
        &mut PixelBudget::new(limits, 0)?,
        &AtomicBool::new(false),
    )
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
fn render(scene: &Scene) -> CpuFrame {
    raster(&source(scene), CompositeLimits::default())
        .unwrap()
        .unwrap()
}

#[test]
fn grayscale_uses_rendered_luminance_times_alpha() {
    for (color, expected) in [
        (Color::rgb(255, 0, 0), 76),
        (Color::rgb(0, 255, 0), 150),
        (Color::rgb(0, 0, 255), 29),
        (Color::rgba(255, 0, 0, 128), 38),
    ] {
        let mut s = scene();
        add(
            &mut s,
            rect("m", 0., 30., color).with_mask(MaskMode::Grayscale),
            "g",
        );
        add(&mut s, rect("ink", 0., 40., Color::WHITE), "g");
        let frame = render(&s);
        assert_eq!(at(&frame, 10, 10), [expected; 4]);
        assert_eq!(at(&frame, 35, 10), [0; 4]);
    }
}
#[test]
fn clipping_modes_paint_their_own_content_before_the_run() {
    for mode in [
        MaskMode::Alpha,
        MaskMode::Path,
        MaskMode::Clipping,
        MaskMode::ClippingPath,
    ] {
        let mut s = scene();
        add(
            &mut s,
            rect("m", 0., 30., Color::rgb(255, 0, 0)).with_mask(mode),
            "g",
        );
        add(&mut s, rect("ink", 20., 30., Color::rgb(0, 0, 255)), "g");
        let frame = render(&s);
        assert_eq!(
            at(&frame, 10, 10),
            if mode.paints_self() {
                [255, 0, 0, 255]
            } else {
                [0; 4]
            }
        );
        assert_eq!(at(&frame, 25, 10), [0, 0, 255, 255]);
        assert_eq!(at(&frame, 40, 10), [0; 4]);
    }
}
#[test]
fn path_mask_ignores_paint_alpha_and_uses_evenodd_contours() {
    let mut s = scene();
    let mut mask = DisplayObject::new(
        "m",
        display::Path::from_svg("M0 0H40V30H0Z M10 5H30V25H10Z").unwrap(),
    )
    .with_mask(MaskMode::Path);
    mask.style.fill = None;
    mask.style.fill_rule = FillRule::EvenOdd;
    add(&mut s, mask, "g");
    add(&mut s, rect("ink", 0., 50., Color::WHITE), "g");
    let frame = render(&s);
    assert_eq!(at(&frame, 5, 10), [255; 4]);
    assert_eq!(at(&frame, 20, 10), [0; 4]);
    assert_eq!(at(&frame, 45, 10), [0; 4]);
}
#[test]
fn opaque_path_clips_actual_backdrop_and_translucent_path_isolates_once() {
    // Float opacity 0.5 gives round(255 * .5)=128 for both channels.
    // An alpha-mask surface quantizes first and has a distinct 128/255 factor.
    for (opacity, expected) in [(1., [0, 0, 0, 255]), (0.5, [128, 0, 128, 255])] {
        let mut s = scene();
        add(&mut s, rect("red", 0., 50., Color::rgb(255, 0, 0)), "g");
        let mut mask = rect("m", 0., 30., Color::WHITE).with_mask(MaskMode::Path);
        mask.style.opacity = opacity;
        add(&mut s, mask, "g");
        let mut ink = rect("blue", 0., 50., Color::rgb(0, 0, 255));
        ink.style.blend_mode = BlendMode::Multiply;
        add(&mut s, ink, "g");
        let frame = render(&s);
        assert_eq!(at(&frame, 10, 10), expected);
        assert_eq!(at(&frame, 40, 10), [255, 0, 0, 255]);
    }
}
#[test]
fn opaque_path_allows_eraser_to_reach_prior_parent_pixels() {
    let mut s = scene();
    add(&mut s, rect("red", 0., 50., Color::rgb(255, 0, 0)), "g");
    add(
        &mut s,
        rect("m", 0., 30., Color::WHITE).with_mask(MaskMode::Path),
        "g",
    );
    add(
        &mut s,
        rect("erase", 10., 40., Color::WHITE).with_eraser(EraserMode::Path),
        "g",
    );
    let frame = render(&s);
    assert_eq!(at(&frame, 5, 10), [255, 0, 0, 255]);
    assert_eq!(at(&frame, 20, 10), [0; 4]);
    assert_eq!(at(&frame, 40, 10), [255, 0, 0, 255]);
}
#[test]
fn branch_path_intersects_children_and_image_geometry_needs_no_pixels() {
    let mut s = scene();
    add(
        &mut s,
        DisplayObject::new("m", display::Group::new()).with_mask(MaskMode::Path),
        "g",
    );
    add(&mut s, rect("left", 0., 30., Color::WHITE), "m");
    add(
        &mut s,
        DisplayObject::new(
            "image",
            display::Image::new("unloaded", Size::new(30., 30.)),
        )
        .with_transform(Affine::translation(20., 0.)),
        "m",
    );
    add(&mut s, rect("ink", 0., 50., Color::WHITE), "g");
    let source = source(&s);
    assert!(source.images().is_empty());
    let frame = raster(&source, CompositeLimits::default())
        .unwrap()
        .unwrap();
    assert_eq!(at(&frame, 10, 10), [0; 4]);
    assert_eq!(at(&frame, 25, 10), [255; 4]);
    assert_eq!(at(&frame, 40, 10), [0; 4]);
}
#[test]
fn hidden_only_marker_does_not_activate_mask_schedule() {
    for mode in [
        MaskMode::Alpha,
        MaskMode::Grayscale,
        MaskMode::Path,
        MaskMode::Clipping,
        MaskMode::ClippingPath,
    ] {
        let mut s = scene();
        let mut mask = rect("m", 0., 30., Color::WHITE).with_mask(mode);
        mask.visible = false;
        add(&mut s, mask, "g");
        add(&mut s, rect("ink", 0., 50., Color::WHITE), "g");
        assert_eq!(at(&render(&s), 40, 10), [255; 4]);
    }
}
#[test]
fn subsequent_hidden_marker_splits_active_runs_and_path_clip_is_identity() {
    for (mode, expected) in [(MaskMode::Alpha, [0; 4]), (MaskMode::Path, [255; 4])] {
        let mut s = scene();
        add(
            &mut s,
            rect("first", 0., 10., Color::WHITE).with_mask(MaskMode::Alpha),
            "g",
        );
        let mut hidden = rect("hidden", 0., 10., Color::WHITE).with_mask(mode);
        hidden.visible = false;
        add(&mut s, hidden, "g");
        add(&mut s, rect("ink", 0., 50., Color::WHITE), "g");
        assert_eq!(at(&render(&s), 40, 10), expected);
    }
}
#[test]
fn empty_path_clears_coverage_but_empty_branch_adds_no_clip() {
    for (shape, expected) in [
        (
            gpui_flow_scene::Shape::Path(display::Path::default()),
            [0; 4],
        ),
        (
            gpui_flow_scene::Shape::Group(display::Group::new()),
            [255; 4],
        ),
    ] {
        let mut s = scene();
        add(
            &mut s,
            DisplayObject::new("m", shape).with_mask(MaskMode::Path),
            "g",
        );
        add(&mut s, rect("ink", 0., 50., Color::WHITE), "g");
        assert_eq!(at(&render(&s), 10, 10), expected);
    }
}
#[test]
fn path_mask_admission_rejects_low_budget_before_raster_writes() {
    let mut s = scene();
    add(
        &mut s,
        rect("m", 0., 30., Color::WHITE).with_mask(MaskMode::Path),
        "g",
    );
    add(&mut s, rect("ink", 0., 50., Color::WHITE), "g");
    let source = source(&s);
    let limits = CompositeLimits {
        max_surface_writes: 1,
        ..CompositeLimits::default()
    };
    assert_eq!(
        raster(&source, limits).unwrap_err(),
        CompositeError::WorkLimit
    );
    let limits = CompositeLimits {
        max_surface_bytes: 1,
        ..CompositeLimits::default()
    };
    assert_eq!(
        raster(&source, limits).unwrap_err(),
        CompositeError::WorkingSetLimit
    );
}

#[test]
fn path_mask_intersects_inherited_frame_clip_under_affine_placement() {
    let mut s = scene();
    add(
        &mut s,
        DisplayObject::new("frame", display::Frame::new(25., 30.))
            .with_style(Style {
                fill: None,
                ..Default::default()
            })
            .with_transform(Affine::translation(10., 0.)),
        "g",
    );
    add(
        &mut s,
        DisplayObject::new("inner", display::Group::new()),
        "frame",
    );
    add(
        &mut s,
        rect("m", 10., 30., Color::WHITE).with_mask(MaskMode::Path),
        "inner",
    );
    add(&mut s, rect("ink", 0., 50., Color::WHITE), "inner");
    let frame = render(&s);
    assert_eq!(at(&frame, 15, 10), [0; 4]);
    assert_eq!(at(&frame, 25, 10), [255; 4]);
    assert_eq!(at(&frame, 38, 10), [0; 4]);
}

#[test]
fn path_text_rejects_explicitly_and_mask_modes_roundtrip_native_json() {
    let mut s = scene();
    add(
        &mut s,
        DisplayObject::new("m", display::Text::new("x", Size::new(30., 30.)))
            .with_mask(MaskMode::Path),
        "g",
    );
    add(&mut s, rect("ink", 0., 50., Color::WHITE), "g");
    let source = source(&s);
    assert!(source.texts().is_empty());
    assert_eq!(
        raster(&source, CompositeLimits::default()).unwrap_err(),
        CompositeError::UnsupportedPathMask("m".into())
    );
    for mode in [
        MaskMode::Alpha,
        MaskMode::Grayscale,
        MaskMode::Path,
        MaskMode::Clipping,
        MaskMode::ClippingPath,
    ] {
        let json = serde_json::to_string(&mode).unwrap();
        assert_eq!(serde_json::from_str::<MaskMode>(&json).unwrap(), mode);
    }
}

#[test]
fn mask_schedule_preflight_matches_exact_memory_and_work_limits() {
    for mode in [
        MaskMode::Alpha,
        MaskMode::Grayscale,
        MaskMode::Path,
        MaskMode::Clipping,
        MaskMode::ClippingPath,
    ] {
        for opacity in [1., 0.5] {
            let mut s = scene();
            let mut mask = rect("m", 0., 30., Color::WHITE).with_mask(mode);
            mask.style.opacity = opacity;
            add(&mut s, mask, "g");
            add(&mut s, rect("ink", 0., 50., Color::WHITE), "g");
            let source = source(&s);
            let bounds = Bounds::new(Point::ZERO, Size::new(60., 40.));
            let viewport = RasterViewport {
                world_to_screen: Affine::IDENTITY,
                canvas_bounds: bounds,
                content_mask: bounds,
                device_scale: 1.,
            };
            let draw = |budget: &mut PixelBudget| {
                rasterize(&source, viewport, budget, &AtomicBool::new(false))
            };
            let mut measured = PixelBudget::new(CompositeLimits::default(), 0).unwrap();
            let frame = draw(&mut measured).unwrap().unwrap();
            let exact = CompositeLimits {
                max_surface_bytes: measured.peak_bytes(),
                max_surface_writes: measured.surface_writes(),
                ..Default::default()
            };
            assert_eq!(
                draw(&mut PixelBudget::new(exact, 0).unwrap())
                    .unwrap()
                    .unwrap()
                    .premul_rgba,
                frame.premul_rgba,
                "{mode:?} {opacity}"
            );
            for limits in [
                CompositeLimits {
                    max_surface_bytes: exact.max_surface_bytes - 1,
                    ..exact
                },
                CompositeLimits {
                    max_surface_writes: exact.max_surface_writes - 1,
                    ..exact
                },
            ] {
                let mut budget = PixelBudget::new(limits, 0).unwrap();
                assert!(
                    matches!(
                        draw(&mut budget),
                        Err(CompositeError::WorkingSetLimit | CompositeError::WorkLimit)
                    ),
                    "{mode:?} {opacity}"
                );
                assert_eq!(
                    budget.surface_writes(),
                    0,
                    "admission precedes output writes"
                );
                assert_eq!(budget.retained_bytes(), 0);
            }
        }
    }
}
