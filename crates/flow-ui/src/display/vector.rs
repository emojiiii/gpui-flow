use super::{RenderObject, clip::clip_polygon};
use gpui::{
    App, FillOptions, FillRule, Path, PathBuilder, PathStyle, Pixels, Point, Rgba, StrokeOptions,
    Window, point, px,
};
use gpui_flow_scene::{self as scene, Paint};
fn color(paint: &Paint, opacity: f32) -> Option<Rgba> {
    match paint {
        Paint::Solid { color } => Some(Rgba {
            r: color.r as f32 / 255.,
            g: color.g as f32 / 255.,
            b: color.b as f32 / 255.,
            a: color.a as f32 / 255. * opacity,
        }),
        _ => None,
    }
}
fn transformed_path(
    path: Path<Pixels>,
    transform: scene::Affine,
    clips: &[Vec<scene::Point>],
) -> Path<Pixels> {
    let mut output = Path::new(point(px(0.), px(0.)));
    for triangle in path.vertices.as_chunks::<3>().0 {
        let mut polygon: Vec<_> = triangle
            .iter()
            .map(|v| {
                transform.transform_point(scene::Point::new(
                    v.xy_position.x.into(),
                    v.xy_position.y.into(),
                ))
            })
            .collect();
        for clip in clips {
            polygon = clip_polygon(polygon, clip);
        }
        if polygon.len() >= 3 {
            for index in 1..polygon.len() - 1 {
                let to_gpui = |p: scene::Point| point(px(p.x), px(p.y));
                output.push_triangle(
                    (
                        to_gpui(polygon[0]),
                        to_gpui(polygon[index]),
                        to_gpui(polygon[index + 1]),
                    ),
                    (point(0., 1.), point(0., 1.), point(0., 1.)),
                );
            }
        }
    }
    output
}
fn mesh_path(mesh: &scene::StrokeMesh) -> Path<Pixels> {
    let mut path = Path::new(point(px(0.0), px(0.0)));
    for triangle in mesh.triangles() {
        let [a, b, c] = triangle.map(|p| point(px(p.x), px(p.y)));
        path.push_triangle((a, b, c), (point(0., 1.), point(0., 1.), point(0., 1.)));
    }
    path
}
/// Identifies a paint operation, including width-varying strokes using the same
/// gradient. A fill/stroke boolean would alias distinct layers in the cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PaintLayer {
    Fill(usize),
    Stroke(usize),
}
struct VectorPaint<'a> {
    path: Path<Pixels>,
    paint: &'a Paint,
    layer: PaintLayer,
}
/// Tessellate in local space, then transform triangles. Non-uniform scales
/// preserve each layer's stroke geometry. This is the renderer's actual ordered
/// preparation path, also exercised without a native window by the tests.
#[cfg(test)]
fn prepare_vector_paints(
    object: &RenderObject,
    origin: Point<Pixels>,
    scale_factor: f32,
) -> Vec<VectorPaint<'_>> {
    prepare_vector_paints_for_phase(object, origin, scale_factor, scene::PaintPhase::All)
}
fn prepare_vector_paints_for_phase(
    object: &RenderObject,
    origin: Point<Pixels>,
    scale_factor: f32,
    phase: scene::PaintPhase,
) -> Vec<VectorPaint<'_>> {
    let screen =
        scene::Affine::translation(origin.x.into(), origin.y.into()) * object.screen_transform;
    let needs_contours = (phase.includes_fill()
        && object.object.style.fills().any(|paint| paint.is_visible()))
        || (phase.includes_stroke()
            && object.object.style.strokes().any(|stroke| {
                stroke.width > 0.0 && stroke.paint.is_visible() && !stroke.uses_advanced_geometry()
            }));
    let contours = if needs_contours {
        object
            .object
            .shape
            .flatten(0.25 / (object.screen_transform.max_scale() * scale_factor).max(0.01))
    } else {
        Vec::new()
    };
    let clips: Vec<Vec<scene::Point>> = object
        .clips
        .iter()
        .map(|clip| {
            let transform = scene::Affine::translation(origin.x.into(), origin.y.into())
                * object.viewport_transform
                * clip.world_transform;
            scene::display::Rect::new(clip.local_bounds.size.width, clip.local_bounds.size.height)
                .rounded(clip.corner_radius)
                .flatten(0.25 / (transform.max_scale() * scale_factor).max(0.01))
                .points
                .into_iter()
                .map(|p| transform.transform_point(p + clip.local_bounds.origin))
                .collect()
        })
        .collect();
    let build = |mut builder: PathBuilder| {
        for contour in &contours {
            if let Some(first) = contour.points.first() {
                builder.move_to(point(px(first.x), px(first.y)));
                for p in contour.points.iter().skip(1) {
                    builder.line_to(point(px(p.x), px(p.y)));
                }
                if contour.closed {
                    builder.close();
                }
            }
        }
        builder
            .build()
            .ok()
            .map(|path| transformed_path(path, screen, &clips))
    };
    let mut paints = Vec::new();
    let rule = match object.object.style.fill_rule {
        scene::FillRule::NonZero => FillRule::NonZero,
        scene::FillRule::EvenOdd => FillRule::EvenOdd,
    };
    for (index, fill) in object.object.style.fills().enumerate() {
        if !phase.includes_fill() || !fill.is_visible() {
            continue;
        }
        if let Some(path) = build(
            PathBuilder::fill()
                .with_style(PathStyle::Fill(FillOptions::default().with_fill_rule(rule))),
        ) {
            paints.push(VectorPaint {
                path,
                paint: fill,
                layer: PaintLayer::Fill(index),
            });
        }
    }
    for (index, stroke) in object.object.style.strokes().enumerate() {
        if !phase.includes_stroke() || stroke.width <= 0.0 || !stroke.paint.is_visible() {
            continue;
        }
        let path = if stroke.uses_advanced_geometry() {
            object
                .object
                .shape
                .stroke_mesh_with_rule(stroke, object.object.style.fill_rule)
                .ok()
                .map(|mesh| transformed_path(mesh_path(&mesh), screen, &clips))
        } else {
            // Preserve the pre-existing adaptive solid round renderer exactly.
            build(
                PathBuilder::stroke(px(stroke.width)).with_style(PathStyle::Stroke(
                    StrokeOptions::default()
                        .with_line_width(stroke.width)
                        .with_line_cap(lyon::path::LineCap::Round)
                        .with_line_join(lyon::path::LineJoin::Round),
                )),
            )
        };
        if let Some(path) = path {
            paints.push(VectorPaint {
                path,
                paint: &stroke.paint,
                layer: PaintLayer::Stroke(index),
            });
        }
    }
    paints
}

pub(super) fn paint_vector(
    object: &RenderObject,
    origin: Point<Pixels>,
    phase: scene::PaintPhase,
    window: &mut Window,
    cx: &mut App,
) {
    super::gradient::begin_paint_phase(
        window.window_handle().window_id().as_u64(),
        object.object.id.as_str(),
        phase,
    );
    for VectorPaint { path, paint, layer } in
        prepare_vector_paints_for_phase(object, origin, window.scale_factor(), phase)
    {
        if let Some(color) = color(paint, object.opacity) {
            window.paint_path(path, color);
        } else {
            super::gradient::paint_gradient(object, origin, path, paint, layer, window, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scene::{
        Affine, Color, Contour, DisplayObject, GradientStop, Point as ScenePoint, Stroke, Style,
    };

    fn object(style: Style) -> RenderObject {
        RenderObject {
            object: DisplayObject::new("stack", scene::display::Rect::new(20.0, 20.0))
                .with_style(style),
            screen_transform: Affine::IDENTITY,
            viewport_transform: Affine::IDENTITY,
            opacity: 1.0,
            clips: vec![],
        }
    }
    fn linear(color: Color) -> Paint {
        Paint::LinearGradient {
            start: ScenePoint::ZERO,
            end: ScenePoint::new(20.0, 0.0),
            stops: vec![
                GradientStop { offset: 0.0, color },
                GradientStop { offset: 1.0, color },
            ],
        }
    }
    fn triangles(path: &Path<Pixels>) -> Vec<[ScenePoint; 3]> {
        path.vertices
            .as_chunks::<3>()
            .0
            .iter()
            .map(|triangle| {
                std::array::from_fn(|i| {
                    ScenePoint::new(
                        triangle[i].xy_position.x.into(),
                        triangle[i].xy_position.y.into(),
                    )
                })
            })
            .collect()
    }
    fn sample(paint: &VectorPaint<'_>, transform: Affine, at: ScenePoint, opacity: f32) -> Color {
        let Some(raster) = scene::rasterize_paint(
            paint.paint,
            &triangles(&paint.path),
            transform,
            scene::RasterOptions {
                opacity,
                ..Default::default()
            },
        )
        .unwrap() else {
            return Color::TRANSPARENT;
        };
        let x = (at.x - raster.bounds.min_x()).floor();
        let y = (at.y - raster.bounds.min_y()).floor();
        if x < 0.0 || y < 0.0 {
            return Color::TRANSPARENT;
        }
        raster
            .pixel(x as u32, y as u32)
            .unwrap_or(Color::TRANSPARENT)
    }
    #[test]
    fn renderer_prepares_every_fill_before_every_stroke_in_exact_order() {
        let red = Color::rgb(255, 0, 0);
        let blue = Color::rgb(0, 0, 255);
        let obj = object(Style {
            fill: Some(red.into()),
            fill_layers: vec![linear(Color::rgba(0, 255, 0, 128)), blue.into()],
            stroke: Some(Stroke::new(Color::BLACK, 10.0)),
            stroke_layers: vec![Stroke {
                paint: linear(Color::WHITE),
                width: 2.0,
                ..Stroke::default()
            }],
            ..Style::default()
        });
        let paints = prepare_vector_paints(&obj, point(px(0.0), px(0.0)), 1.0);
        assert_eq!(
            paints.iter().map(|p| p.layer).collect::<Vec<_>>(),
            vec![
                PaintLayer::Fill(0),
                PaintLayer::Fill(1),
                PaintLayer::Fill(2),
                PaintLayer::Stroke(0),
                PaintLayer::Stroke(1)
            ]
        );
        assert_eq!(
            sample(
                &paints[0],
                Affine::IDENTITY,
                ScenePoint::new(10.5, 10.5),
                1.0
            ),
            red
        );
        assert_eq!(
            sample(
                &paints[1],
                Affine::IDENTITY,
                ScenePoint::new(10.5, 10.5),
                1.0
            ),
            Color::rgba(0, 255, 0, 128)
        );
        assert_eq!(
            sample(
                &paints[2],
                Affine::IDENTITY,
                ScenePoint::new(10.5, 10.5),
                1.0
            ),
            blue
        );
        assert_eq!(
            sample(
                &paints[3],
                Affine::IDENTITY,
                ScenePoint::new(-3.5, 10.5),
                1.0
            ),
            Color::BLACK
        );
        assert_eq!(
            sample(
                &paints[4],
                Affine::IDENTITY,
                ScenePoint::new(-3.5, 10.5),
                1.0
            )
            .a,
            0
        );
        assert_eq!(
            sample(
                &paints[4],
                Affine::IDENTITY,
                ScenePoint::new(0.5, 10.5),
                1.0
            ),
            Color::WHITE
        );
        // Reordering serialized layers changes the actual order submitted to GPUI.
        let mut reversed = obj.clone();
        reversed.object.style.fill_layers.reverse();
        let reversed_paints = prepare_vector_paints(&reversed, point(px(0.0), px(0.0)), 1.0);
        assert_eq!(*reversed_paints[1].paint, Paint::solid(blue));
    }
    #[test]
    fn renderer_skips_transparent_and_zero_width_layers_without_renumbering_slots() {
        let obj = object(Style {
            fill: Some(Color::TRANSPARENT.into()),
            fill_layers: vec![linear(Color::TRANSPARENT), Color::WHITE.into()],
            stroke: Some(Stroke::new(Color::BLACK, 0.0)),
            stroke_layers: vec![
                Stroke::new(Color::TRANSPARENT, 100.0),
                Stroke::new(Color::WHITE, 2.0),
            ],
            ..Style::default()
        });
        let paints = prepare_vector_paints(&obj, point(px(0.0), px(0.0)), 1.0);
        assert_eq!(
            paints.iter().map(|p| p.layer).collect::<Vec<_>>(),
            vec![PaintLayer::Fill(2), PaintLayer::Stroke(2)]
        );
    }
    #[test]
    fn every_layer_uses_affine_stroke_geometry_and_the_same_nested_clips() {
        let mut obj = object(Style {
            fill_layers: vec![linear(Color::WHITE)],
            stroke_layers: vec![
                Stroke::new(Color::BLACK, 8.0),
                Stroke {
                    paint: linear(Color::WHITE),
                    width: 2.0,
                    ..Stroke::default()
                },
            ],
            ..Style::default()
        });
        obj.screen_transform = Affine::translation(10.0, 5.0) * Affine::scale(2.0, 3.0);
        obj.clips = vec![
            scene::ClipRegion {
                local_bounds: scene::Bounds::from_points(
                    ScenePoint::new(12.0, 8.0),
                    ScenePoint::new(35.0, 45.0),
                ),
                corner_radius: 3.0,
                world_transform: Affine::IDENTITY,
            },
            scene::ClipRegion {
                local_bounds: scene::Bounds::from_points(
                    ScenePoint::new(14.0, 10.0),
                    ScenePoint::new(32.0, 40.0),
                ),
                corner_radius: 0.0,
                world_transform: Affine::IDENTITY,
            },
        ];
        let paints = prepare_vector_paints(&obj, point(px(0.0), px(0.0)), 2.0);
        assert_eq!(paints.len(), 4);
        for paint in paints {
            for p in triangles(&paint.path).into_iter().flatten() {
                assert!(p.x >= 14.0 - 0.001 && p.x <= 32.0 + 0.001);
                assert!(p.y >= 10.0 - 0.001 && p.y <= 40.0 + 0.001);
            }
        }
        obj.clips.clear();
        let paints = prepare_vector_paints(&obj, point(px(0.0), px(0.0)), 2.0);
        let points: Vec<_> = triangles(&paints[2].path).into_iter().flatten().collect();
        assert!((points.iter().map(|p| p.x).fold(f32::INFINITY, f32::min) - 2.0).abs() < 0.01);
        assert!((points.iter().map(|p| p.y).fold(f32::INFINITY, f32::min) + 7.0).abs() < 0.01);
    }
    #[test]
    fn extra_fills_reuse_even_odd_holes_instead_of_filling_the_bounding_box() {
        let outer = Contour {
            points: vec![
                ScenePoint::new(0.0, 0.0),
                ScenePoint::new(20.0, 0.0),
                ScenePoint::new(20.0, 20.0),
                ScenePoint::new(0.0, 20.0),
            ],
            closed: true,
        };
        let inner = Contour {
            points: vec![
                ScenePoint::new(5.0, 5.0),
                ScenePoint::new(15.0, 5.0),
                ScenePoint::new(15.0, 15.0),
                ScenePoint::new(5.0, 15.0),
            ],
            closed: true,
        };
        let mut obj = object(Style {
            fill: None,
            fill_layers: vec![linear(Color::WHITE), Color::BLACK.into()],
            fill_rule: scene::FillRule::EvenOdd,
            ..Style::default()
        });
        obj.object.shape = scene::display::Custom::new(
            "donut",
            scene::Bounds::from_points(ScenePoint::ZERO, ScenePoint::new(20.0, 20.0)),
            vec![outer, inner],
        )
        .into();
        let paints = prepare_vector_paints(&obj, point(px(0.0), px(0.0)), 1.0);
        for paint in &paints {
            assert_eq!(
                sample(paint, Affine::IDENTITY, ScenePoint::new(10.5, 10.5), 1.0).a,
                0
            );
            assert_eq!(
                sample(paint, Affine::IDENTITY, ScenePoint::new(2.5, 2.5), 1.0).a,
                255
            );
        }
    }
    #[test]
    fn element_opacity_is_explicitly_per_paint_in_this_backend() {
        let obj = object(Style {
            fill_layers: vec![linear(Color::WHITE)],
            ..Style::default()
        });
        let paints = prepare_vector_paints(&obj, point(px(0.0), px(0.0)), 1.0);
        assert!((color(paints[0].paint, 0.5).unwrap().a - 0.5).abs() < 0.001);
        assert_eq!(
            sample(
                &paints[1],
                Affine::IDENTITY,
                ScenePoint::new(10.5, 10.5),
                0.5
            )
            .a,
            128
        );
    }
    #[test]
    fn advanced_renderer_uses_exact_headless_mesh_at_all_device_scales() {
        let stroke = Stroke::new(Color::BLACK, 4.0)
            .with_cap(scene::StrokeCap::Square)
            .with_join(scene::StrokeJoin::Miter)
            .with_dashes(vec![6.0, 4.0, 2.0], -3.0);
        let mut obj = object(Style {
            fill: None,
            stroke: Some(stroke.clone()),
            ..Style::default()
        });
        obj.object.shape = scene::display::Pen::new()
            .move_to(ScenePoint::ZERO)
            .cubic_to(
                ScenePoint::new(10.0, 30.0),
                ScenePoint::new(25.0, -10.0),
                ScenePoint::new(40.0, 10.0),
            )
            .into();
        obj.screen_transform =
            Affine::translation(10.0, 20.0) * Affine::rotation(20.0) * Affine::scale(2.0, 0.7);
        let mesh = obj.object.shape.stroke_mesh(&stroke).unwrap();
        let origin = point(px(5.0), px(7.0));
        let screen = Affine::translation(5.0, 7.0) * obj.screen_transform;
        let expected: Vec<_> = mesh
            .triangles()
            .iter()
            .map(|triangle| triangle.map(|p| screen.transform_point(p)))
            .collect();
        for device_scale in [1.0, 2.0, 4.0] {
            let paints = prepare_vector_paints(&obj, origin, device_scale);
            assert_eq!(paints.len(), 1);
            assert_eq!(paints[0].layer, PaintLayer::Stroke(0));
            assert_eq!(triangles(&paints[0].path), expected);
        }
    }
    #[test]
    fn advanced_gradient_dash_gaps_and_nested_clips_match_scene_picking() {
        let mut stroke = Stroke::new(Color::WHITE, 4.0)
            .with_cap(scene::StrokeCap::Butt)
            .with_dashes(vec![6.0, 4.0], 0.0);
        stroke.paint = linear(Color::WHITE);
        let mut obj = object(Style {
            fill: None,
            stroke: Some(stroke),
            ..Style::default()
        });
        obj.object.shape =
            scene::display::Line::new(ScenePoint::ZERO, ScenePoint::new(40.0, 0.0)).into();
        obj.screen_transform = Affine::translation(10.0, 10.0) * Affine::scale(2.0, 3.0);
        obj.clips = vec![
            scene::ClipRegion {
                local_bounds: scene::Bounds::from_points(
                    ScenePoint::new(12.0, 4.0),
                    ScenePoint::new(60.0, 16.0),
                ),
                corner_radius: 1.0,
                world_transform: Affine::IDENTITY,
            },
            scene::ClipRegion {
                local_bounds: scene::Bounds::from_points(
                    ScenePoint::new(14.0, 5.0),
                    ScenePoint::new(55.0, 15.0),
                ),
                corner_radius: 0.0,
                world_transform: Affine::IDENTITY,
            },
        ];
        let paints = prepare_vector_paints(&obj, point(px(0.0), px(0.0)), 2.0);
        assert_eq!(paints.len(), 1);
        for p in triangles(&paints[0].path).iter().flatten() {
            assert!(p.x >= 14.0 - 0.001 && p.x <= 55.0 + 0.001);
            assert!(p.y >= 5.0 - 0.001 && p.y <= 15.0 + 0.001);
        }
        let inverse = obj.screen_transform.inverse().unwrap();
        for (point, visible) in [
            (ScenePoint::new(17.5, 10.5), true),
            (ScenePoint::new(26.5, 10.5), false),
        ] {
            let local = inverse.transform_point(point);
            assert_eq!(
                obj.object.shape.hit_test(local, &obj.object.style, 0.0),
                visible
            );
            assert_eq!(
                sample(&paints[0], obj.screen_transform, point, 1.0).a > 0,
                visible
            );
        }
        assert_eq!(
            sample(
                &paints[0],
                obj.screen_transform,
                ScenePoint::new(12.5, 10.5),
                1.0
            )
            .a,
            0
        );
    }
    #[test]
    fn advanced_stroke_layer_order_and_failures_are_explicit() {
        let dashed = Stroke::new(Color::BLACK, 4.0)
            .with_cap(scene::StrokeCap::Butt)
            .with_dashes(vec![6.0, 4.0], 0.0);
        let mut obj = object(Style {
            fill_layers: vec![linear(Color::WHITE)],
            stroke: Some(dashed.clone()),
            stroke_layers: vec![Stroke::new(Color::WHITE, 1.0)],
            ..Style::default()
        });
        let paints = prepare_vector_paints(&obj, point(px(0.0), px(0.0)), 1.0);
        assert_eq!(
            paints.iter().map(|p| p.layer).collect::<Vec<_>>(),
            vec![
                PaintLayer::Fill(0),
                PaintLayer::Fill(1),
                PaintLayer::Stroke(0),
                PaintLayer::Stroke(1)
            ]
        );
        obj.object.style.stroke.as_mut().unwrap().dash_pattern = vec![f32::MIN_POSITIVE, 1.0];
        assert!(
            obj.diagnostics()
                .iter()
                .any(|issue| issue.feature == super::super::UnsupportedFeature::StrokeGeometry)
        );
        obj.object.shape = scene::display::Text::new("text", scene::Size::new(20.0, 10.0)).into();
        obj.object.style = Style {
            stroke: Some(dashed),
            ..Style::default()
        };
        assert!(
            obj.diagnostics()
                .iter()
                .any(|issue| issue.feature == super::super::UnsupportedFeature::StrokeGeometry)
        );
    }

    #[test]
    fn aligned_native_triangles_are_the_final_mesh_under_every_affine_and_fill_rule() {
        for alignment in [
            scene::StrokeAlignment::Inside,
            scene::StrokeAlignment::Outside,
        ] {
            for rule in [scene::FillRule::NonZero, scene::FillRule::EvenOdd] {
                let stroke = Stroke::new(Color::WHITE, 4.).with_alignment(alignment);
                let mut obj = object(Style {
                    fill: None,
                    fill_rule: rule,
                    stroke: Some(stroke.clone()),
                    ..Default::default()
                });
                obj.object.shape = scene::display::Path::from_svg("M0 0H40V40H0Z M10 10H30V30H10Z")
                    .unwrap()
                    .into();
                obj.screen_transform = Affine::translation(52., 18.)
                    * Affine::rotation(14.)
                    * Affine::scale(-1.7, 0.8);
                let mesh = obj
                    .object
                    .shape
                    .stroke_mesh_with_rule(&stroke, rule)
                    .unwrap();
                let expected: Vec<_> = mesh
                    .triangles()
                    .iter()
                    .map(|t| t.map(|p| obj.screen_transform.transform_point(p)))
                    .collect();
                for dpr in [1., 1.5, 2.] {
                    let paints = prepare_vector_paints(&obj, point(px(0.), px(0.)), dpr);
                    assert_eq!(paints.len(), 1);
                    assert_eq!(triangles(&paints[0].path), expected);
                }
            }
        }
    }
    #[test]
    fn independently_aligned_gradient_layers_keep_order_and_nested_clip_geometry() {
        let mut obj = object(Style {
            fill: None,
            stroke: Some(Stroke {
                paint: linear(Color::rgba(255, 0, 0, 128)),
                ..Stroke::new(Color::WHITE, 4.).with_alignment(scene::StrokeAlignment::Inside)
            }),
            stroke_layers: vec![Stroke {
                paint: linear(Color::rgba(0, 0, 255, 128)),
                ..Stroke::new(Color::WHITE, 4.).with_alignment(scene::StrokeAlignment::Outside)
            }],
            ..Default::default()
        });
        obj.screen_transform = Affine::translation(10., 10.);
        obj.clips = vec![
            scene::ClipRegion {
                local_bounds: scene::Bounds::new(
                    ScenePoint::new(8., 8.),
                    scene::Size::new(30., 30.),
                ),
                corner_radius: 4.,
                world_transform: Affine::IDENTITY,
            },
            scene::ClipRegion {
                local_bounds: scene::Bounds::new(
                    ScenePoint::new(9., 9.),
                    scene::Size::new(24., 24.),
                ),
                corner_radius: 2.,
                world_transform: Affine::IDENTITY,
            },
        ];
        let paints = prepare_vector_paints(&obj, point(px(0.), px(0.)), 1.5);
        assert_eq!(
            paints.iter().map(|p| p.layer).collect::<Vec<_>>(),
            vec![PaintLayer::Stroke(0), PaintLayer::Stroke(1)]
        );
        assert_eq!(
            sample(
                &paints[0],
                obj.screen_transform,
                ScenePoint::new(11.5, 20.5),
                1.
            ),
            Color::rgba(255, 0, 0, 128)
        );
        assert_eq!(
            sample(
                &paints[1],
                obj.screen_transform,
                ScenePoint::new(31.5, 20.5),
                1.
            ),
            Color::rgba(0, 0, 255, 128)
        );
        for paint in &paints {
            for p in triangles(&paint.path).iter().flatten() {
                assert!(
                    p.x >= 9. - 0.001
                        && p.x <= 33. + 0.001
                        && p.y >= 9. - 0.001
                        && p.y <= 33. + 0.001
                );
            }
        }
    }
    #[test]
    fn container_phases_partition_original_layers_and_preserve_ancestor_clips() {
        let mut obj = object(Style {
            fill: Some(Color::TRANSPARENT.into()),
            fill_layers: vec![linear(Color::rgba(255, 0, 0, 128)), Color::WHITE.into()],
            stroke: Some(Stroke::new(Color::TRANSPARENT, 8.)),
            stroke_layers: vec![
                Stroke::new(Color::BLACK, 8.).with_alignment(scene::StrokeAlignment::Outside),
                Stroke {
                    paint: linear(Color::rgba(0, 0, 255, 128)),
                    ..Stroke::new(Color::WHITE, 4.)
                },
            ],
            ..Default::default()
        });
        obj.object.shape = scene::display::Frame::new(20., 20.)
            .rounded(3.)
            .with_paint_order(scene::ContainerPaintOrder::StrokeAfterChildren)
            .into();
        obj.screen_transform = Affine::translation(10., 10.);
        obj.clips = vec![scene::ClipRegion {
            local_bounds: scene::Bounds::new(ScenePoint::new(5., 5.), scene::Size::new(32., 32.)),
            corner_radius: 0.,
            world_transform: Affine::IDENTITY,
        }];
        let all = prepare_vector_paints(&obj, point(px(0.), px(0.)), 1.5);
        let fills = prepare_vector_paints_for_phase(
            &obj,
            point(px(0.), px(0.)),
            1.5,
            scene::PaintPhase::Fill,
        );
        let strokes = prepare_vector_paints_for_phase(
            &obj,
            point(px(0.), px(0.)),
            1.5,
            scene::PaintPhase::Stroke,
        );
        assert_eq!(
            fills.iter().map(|p| p.layer).collect::<Vec<_>>(),
            vec![PaintLayer::Fill(1), PaintLayer::Fill(2)]
        );
        assert_eq!(
            strokes.iter().map(|p| p.layer).collect::<Vec<_>>(),
            vec![PaintLayer::Stroke(1), PaintLayer::Stroke(2)]
        );
        for (all, split) in all.iter().zip(fills.iter().chain(&strokes)) {
            assert_eq!(all.layer, split.layer);
            assert_eq!(all.paint, split.paint);
            assert_eq!(triangles(&all.path), triangles(&split.path));
        }
        // Own stroke is allowed outside the Frame's child clip (x < 10),
        // and is still constrained by the real ancestor clip (x >= 5).
        let points: Vec<_> = triangles(&strokes[0].path).into_iter().flatten().collect();
        assert!(points.iter().any(|p| p.x < 10.));
        assert!(
            points
                .iter()
                .all(|p| p.x >= 5. - 0.001 && p.x <= 37. + 0.001)
        );
    }
}
