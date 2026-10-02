use super::*;
use gpui_flow_scene::{
    Affine, BlendMode, Bounds, Color, DisplayObject, Point, Scene, Size, Style, display::Ellipse,
};
use std::sync::atomic::AtomicBool;
fn render(ellipse: Ellipse, transform: Affine) -> Option<CpuFrame> {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("ellipse", ellipse)
                .with_transform(transform)
                .with_style(Style {
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
fn default_oval_keeps_the_original_tiny_skia_oval_path() {
    let actual = crate::vector::path(&Ellipse::new(40., 20.).into())
        .unwrap()
        .unwrap();
    let expected =
        tiny_skia::PathBuilder::from_oval(tiny_skia::Rect::from_xywh(0., 0., 40., 20.).unwrap())
            .unwrap();
    assert_eq!(actual, expected);
}
#[test]
fn donut_hole_survives_real_cpu_pixels_and_isolated_opacity() {
    let frame = render(Ellipse::circle(20.).ring(0.5), Affine::IDENTITY).unwrap();
    assert_eq!(at(&frame, 20, 20), [0; 4]);
    assert_eq!(at(&frame, 25, 20), [0; 4]);
    assert_eq!(at(&frame, 35, 20), [128, 0, 0, 128]);
}
#[test]
fn pie_and_open_chord_fill_differ_at_the_real_center_region() {
    let pie = render(Ellipse::circle(20.).sector(0., 90.), Affine::IDENTITY).unwrap();
    let arc = render(Ellipse::circle(20.).arc(0., 90.), Affine::IDENTITY).unwrap();
    assert_eq!(at(&pie, 23, 23), [128, 0, 0, 128]);
    assert_eq!(at(&arc, 23, 23), [0; 4]);
    assert_eq!(at(&arc, 33, 33), [128, 0, 0, 128]);
    assert_eq!(at(&pie, 10, 10), [0; 4]);
}
#[test]
fn annular_sector_pixels_do_not_fill_the_center_or_other_quadrants() {
    let frame = render(
        Ellipse::circle(20.).sector(0., 90.).ring(0.5),
        Affine::IDENTITY,
    )
    .unwrap();
    assert_eq!(at(&frame, 23, 23), [0; 4]);
    assert_eq!(at(&frame, 30, 30), [128, 0, 0, 128]);
    assert_eq!(at(&frame, 10, 30), [0; 4]);
}
#[test]
fn affine_ring_keeps_the_hole_in_its_transformed_location() {
    let frame = render(
        Ellipse::circle(20.).ring(0.5),
        Affine::translation(5., 5.) * Affine::scale(1.5, 0.75),
    )
    .unwrap();
    assert_eq!(at(&frame, 35, 20), [0; 4]);
    assert_eq!(at(&frame, 57, 20), [128, 0, 0, 128]);
}
#[test]
fn zero_span_is_empty_in_compositor_instead_of_falling_back_to_a_full_oval() {
    assert!(
        crate::vector::path(&Ellipse::circle(20.).sector(45., 45.).into())
            .unwrap()
            .is_none()
    );
    assert!(render(Ellipse::circle(20.).sector(45., 45.), Affine::IDENTITY).is_none());
}
