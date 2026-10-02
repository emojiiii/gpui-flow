use super::*;
use gpui_flow_scene::{
    Affine, BlendMode, Bounds, Color, DisplayObject, Point, Scene, Size, Style, display::Rect,
};
use std::sync::atomic::AtomicBool;
fn render(rect: Rect) -> CpuFrame {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("rect", rect).with_style(Style {
                fill: Some(Color::hex(0xff0000).into()),
                opacity: 0.5,
                blend_mode: BlendMode::Normal,
                ..Default::default()
            }),
            None,
            None,
        )
        .unwrap();
    let source = match scene_units(&scene, CompositeLimits::default())
        .unwrap()
        .remove(0)
    {
        SceneUnit::Domain(d) => *d,
        u => panic!("{u:?}"),
    };
    rasterize(
        &source,
        RasterViewport {
            world_to_screen: Affine::IDENTITY,
            canvas_bounds: Bounds::new(Point::ZERO, Size::new(100., 100.)),
            content_mask: Bounds::new(Point::ZERO, Size::new(100., 100.)),
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
fn independent_corner_pixels_match_the_selected_clockwise_radii() {
    let frame = render(Rect::new(60., 40.).with_corners([15., 0., 10., 0.]));
    assert_eq!(at(&frame, 1, 1), [0; 4]);
    assert_eq!(at(&frame, 58, 1), [128, 0, 0, 128]);
    assert_eq!(at(&frame, 58, 38), [0; 4]);
    assert_eq!(at(&frame, 1, 38), [128, 0, 0, 128]);
    assert_eq!(at(&frame, 30, 20), [128, 0, 0, 128]);
}
#[test]
fn zero_optional_radii_and_legacy_sharp_rect_have_identical_pixels() {
    let legacy = render(Rect::new(60., 40.));
    let optional = render(Rect::new(60., 40.).with_corners([0.; 4]));
    assert_eq!(legacy.screen_bounds, optional.screen_bounds);
    assert_eq!(legacy.premul_rgba, optional.premul_rgba);
}
#[test]
fn huge_input_corner_radii_are_clamped_before_pixel_conversion() {
    let large = render(Rect::new(60., 40.).with_corners([500., 0., 200., 0.]));
    let clamped = render(Rect::new(60., 40.).with_corners([20., 0., 20., 0.]));
    assert_eq!(large.screen_bounds, clamped.screen_bounds);
    assert_eq!(large.premul_rgba, clamped.premul_rgba);
}
