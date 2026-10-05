//! Retained shared-budget regression from the retired container-inner-shadow gallery.
//! The GUI controls and labels are deliberately excluded; these three domains must
//! remain renderable together while retaining their output frames under one budget.
use gpui_flow_composite::{
    CompositeLimits, PixelBudget, RasterViewport, SceneUnit, rasterize, scene_units,
};
use gpui_flow_scene::{
    Affine, Bounds as SceneBounds, Color, ContainerPaintOrder, DisplayObject, Point as ScenePoint,
    Scene, Shadow, Shape, Size, Stroke, StrokeAlignment, Style, display,
};
use std::sync::atomic::AtomicBool;

fn effect() -> Option<Shadow> {
    Some(Shadow::new(20., 24., 0., Color::rgba(0, 0, 0, 180)))
}
fn layers() -> Vec<Shadow> {
    vec![
        Shadow::new(10., 7., 2., Color::rgba(255, 65, 120, 230)),
        Shadow::new(-7., -4., 0., Color::rgba(255, 215, 30, 170)),
    ]
}
fn document() -> Scene {
    let mut s = Scene::new();
    for (id, x, shape) in [
        ("group", 65., Shape::Group(display::Group::new())),
        (
            "box",
            340.,
            Shape::Box(
                display::Box::new(130., 100.)
                    .rounded(20.)
                    .with_paint_order(ContainerPaintOrder::StrokeAfterChildren),
            ),
        ),
        (
            "frame",
            640.,
            Shape::Frame(
                display::Frame::new(130., 100.)
                    .rounded(20.)
                    .with_paint_order(ContainerPaintOrder::StrokeAfterChildren),
            ),
        ),
    ] {
        s.insert(
            DisplayObject::new(id, shape)
                .with_transform(Affine::translation(x, 100.))
                .with_style(Style {
                    fill: None,
                    stroke: (id != "group").then(|| {
                        Stroke::new(Color::hex(0x40c8ff), 8.)
                            .with_alignment(StrokeAlignment::Outside)
                    }),
                    shadow: effect(),
                    inner_shadow_layers: layers(),
                    ..Default::default()
                }),
            None,
            None,
        )
        .unwrap();
        for (suffix, x, y) in [("a", 15., 15.), ("b", 85., 65.)] {
            s.insert(
                DisplayObject::new(format!("{id}-{suffix}"), display::Ellipse::circle(30.))
                    .with_transform(Affine::translation(x, y))
                    .with_style(Style {
                        fill: Some(Color::rgba(90, 230, 175, 220).into()),
                        shadow: (id == "group")
                            .then(|| Shadow::new(-14., 10., 0., Color::rgba(45, 150, 240, 160))),
                        ..Default::default()
                    }),
                Some(id.into()),
                None,
            )
            .unwrap();
        }
    }
    s
}

#[test]
fn group_box_and_frame_inner_shadows_share_one_budget() {
    let scene = document();
    let bounds = SceneBounds::new(ScenePoint::ZERO, Size::new(1250., 360.));
    let viewport = RasterViewport {
        world_to_screen: Affine::IDENTITY,
        canvas_bounds: bounds,
        content_mask: bounds,
        device_scale: 1.,
    };
    let mut budget = PixelBudget::new(CompositeLimits::default(), 0).unwrap();
    let mut frames = Vec::new();
    for unit in scene_units(&scene, CompositeLimits::default()).unwrap() {
        match unit {
            SceneUnit::Domain(source) => {
                frames.push(
                    rasterize(&source, viewport, &mut budget, &AtomicBool::new(false))
                        .unwrap()
                        .unwrap(),
                );
            }
            SceneUnit::Rejected(issue) => panic!("{issue:?}"),
            _ => {}
        }
    }
    assert_eq!(frames.len(), 3);
}
