use super::*;
use gpui_flow_scene::{
    Affine, BlendMode, Bounds, Color, DisplayObject, Point, Scene, Shape, Size, Style, display,
};
use std::sync::atomic::AtomicBool;
fn render(frame: bool, external: bool, nested: bool, transform: Affine) -> CpuFrame {
    let mut scene = Scene::new();
    let shape = if frame {
        Shape::from(display::Frame::new(60., 40.).with_corners([15., 0., 10., 0.]))
    } else {
        Shape::from(
            display::Box::new(60., 40.)
                .with_corners([15., 0., 10., 0.])
                .clipped(true),
        )
    };
    scene
        .insert(
            DisplayObject::new("parent", shape)
                .with_transform(transform)
                .with_style(Style {
                    fill: None,
                    opacity: if external { 1. } else { 0.5 },
                    blend_mode: if external {
                        BlendMode::PassThrough
                    } else {
                        BlendMode::Normal
                    },
                    ..Default::default()
                }),
            None,
            None,
        )
        .unwrap();
    let parent = if nested {
        scene
            .insert(
                DisplayObject::new(
                    "inner",
                    display::Frame::new(60., 40.).with_corners([0., 15., 0., 0.]),
                )
                .with_style(Style {
                    fill: None,
                    ..Default::default()
                }),
                Some("parent".into()),
                None,
            )
            .unwrap();
        "inner"
    } else {
        "parent"
    };
    scene
        .insert(
            DisplayObject::new("child", display::Rect::new(60., 40.)).with_style(Style {
                fill: Some(Color::hex(0xff0000).into()),
                opacity: if external { 0.5 } else { 1. },
                blend_mode: if external {
                    BlendMode::Normal
                } else {
                    BlendMode::PassThrough
                },
                ..Default::default()
            }),
            Some(parent.into()),
            None,
        )
        .unwrap();
    let source = scene_units(&scene, CompositeLimits::default())
        .unwrap()
        .into_iter()
        .find_map(|unit| {
            if let SceneUnit::Domain(d) = unit {
                Some(*d)
            } else {
                None
            }
        })
        .unwrap();
    rasterize(
        &source,
        RasterViewport {
            world_to_screen: Affine::IDENTITY,
            canvas_bounds: Bounds::new(Point::ZERO, Size::new(120., 100.)),
            content_mask: Bounds::new(Point::ZERO, Size::new(120., 100.)),
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
fn child_clip_masks_use_independent_corners_for_box_and_frame() {
    for frame in [false, true] {
        let f = render(frame, false, false, Affine::IDENTITY);
        assert_eq!(at(&f, 1, 1), [0; 4]);
        assert_eq!(at(&f, 58, 1), [128, 0, 0, 128]);
        assert_eq!(at(&f, 58, 38), [0; 4]);
        assert_eq!(at(&f, 1, 38), [128, 0, 0, 128]);
    }
}
#[test]
fn ancestor_clips_at_domain_boundaries_match_internal_child_masks() {
    let internal = render(true, false, false, Affine::IDENTITY);
    let external = render(true, true, false, Affine::IDENTITY);
    // Domains may conservatively allocate a different transparent fringe.
    // Compare actual world-space pixels, not allocation rectangles/strides.
    for y in 0..44 {
        for x in 0..64 {
            assert_eq!(at(&internal, x, y), at(&external, x, y), "pixel {x},{y}");
        }
    }
}
#[test]
fn nested_different_corners_intersect_in_real_pixels() {
    let f = render(true, false, true, Affine::IDENTITY);
    assert_eq!(at(&f, 1, 1), [0; 4]);
    assert_eq!(at(&f, 58, 1), [0; 4]);
    assert_eq!(at(&f, 1, 38), [128, 0, 0, 128]);
    assert_eq!(at(&f, 30, 20), [128, 0, 0, 128]);
}
#[test]
fn transformed_corner_mask_is_applied_in_container_local_space() {
    let f = render(
        true,
        false,
        false,
        Affine::translation(5., 5.) * Affine::scale(1.5, 0.75),
    );
    assert_eq!(at(&f, 7, 6), [0; 4]);
    assert_eq!(at(&f, 92, 6), [128, 0, 0, 128]);
    assert_eq!(at(&f, 50, 20), [128, 0, 0, 128]);
}
