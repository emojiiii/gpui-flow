use super::{RenderObject, clip::clip_polygon};
use gpui::{App, PathBuilder, Pixels, Point, Rgba, Window, point, px};
use gpui_flow_scene::{self as scene, Paint};
use lyon::tessellation::{
    BuffersBuilder, FillOptions, FillRule, FillTessellator, FillVertex, StrokeOptions,
    StrokeTessellator, StrokeVertex, VertexBuffers,
};

type Triangle = [scene::Point; 3];

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
fn transform_triangles(
    triangles: impl IntoIterator<Item = Triangle>,
    transform: scene::Affine,
    clips: &[Vec<scene::Point>],
) -> Vec<Triangle> {
    let mut output = Vec::new();
    for triangle in triangles {
        let mut polygon = Vec::from(triangle.map(|p| transform.transform_point(p)));
        for clip in clips {
            polygon = clip_polygon(polygon, clip);
        }
        for index in 1..polygon.len().saturating_sub(1) {
            output.push([polygon[0], polygon[index], polygon[index + 1]]);
        }
    }
    output
}
/// Identifies a paint operation, including width-varying strokes using the same
/// gradient. A fill/stroke boolean would alias distinct layers in the cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PaintLayer {
    Fill(usize),
    Stroke(usize),
}
struct VectorPaint<'a> {
    triangles: Vec<Triangle>,
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
            clip.local_rect()
                .flatten(0.25 / (transform.max_scale() * scale_factor).max(0.01))
                .points
                .into_iter()
                .map(|p| transform.transform_point(p + clip.local_bounds.origin))
                .collect()
        })
        .collect();
    let raw_path = {
        let mut builder = lyon::path::Path::builder();
        for contour in contours {
            if let Some((first, rest)) = contour.points.split_first() {
                builder.begin(lyon::math::point(first.x, first.y));
                for p in rest {
                    builder.line_to(lyon::math::point(p.x, p.y));
                }
                builder.end(contour.closed);
            }
        }
        builder.build()
    };
    let transform_buffer = |buffer: VertexBuffers<lyon::math::Point, u16>| {
        transform_triangles(
            buffer.indices.as_chunks::<3>().0.iter().map(|triangle| {
                triangle.map(|index| {
                    let p = buffer.vertices[index as usize];
                    scene::Point::new(p.x, p.y)
                })
            }),
            screen,
            &clips,
        )
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
        let mut buffer = VertexBuffers::new();
        if FillTessellator::new()
            .tessellate_path(
                &raw_path,
                &FillOptions::default().with_fill_rule(rule),
                &mut BuffersBuilder::new(&mut buffer, |vertex: FillVertex| vertex.position()),
            )
            .is_ok()
        {
            paints.push(VectorPaint {
                triangles: transform_buffer(buffer),
                paint: fill,
                layer: PaintLayer::Fill(index),
            });
        }
    }
    for (index, stroke) in object.object.style.strokes().enumerate() {
        if !phase.includes_stroke() || stroke.width <= 0.0 || !stroke.paint.is_visible() {
            continue;
        }
        let triangles = if stroke.uses_advanced_geometry() {
            object
                .object
                .shape
                .stroke_mesh_with_rule(stroke, object.object.style.fill_rule)
                .ok()
                .map(|mesh| transform_triangles(mesh.triangles().iter().copied(), screen, &clips))
        } else {
            let mut buffer = VertexBuffers::new();
            StrokeTessellator::new()
                .tessellate_path(
                    &raw_path,
                    &StrokeOptions::default()
                        .with_line_width(stroke.width)
                        .with_line_cap(lyon::path::LineCap::Round)
                        .with_line_join(lyon::path::LineJoin::Round),
                    &mut BuffersBuilder::new(&mut buffer, |vertex: StrokeVertex| vertex.position()),
                )
                .ok()
                .map(|()| transform_buffer(buffer))
        };
        if let Some(triangles) = triangles {
            paints.push(VectorPaint {
                triangles,
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
    for VectorPaint {
        triangles,
        paint,
        layer,
    } in prepare_vector_paints_for_phase(object, origin, window.scale_factor(), phase)
    {
        if let Some(color) = color(paint, object.opacity) {
            // Consistent winding lets the native path shader fill the union
            // of the mesh, including overlapping stroke triangles.
            let mut builder =
                PathBuilder::fill().with_style(gpui::PathStyle::Fill(gpui::FillOptions {
                    fill_rule: gpui::FillRule::NonZero,
                }));
            for [a, mut b, mut c] in triangles {
                if (b.x - a.x) * (c.y - a.y) < (b.y - a.y) * (c.x - a.x) {
                    std::mem::swap(&mut b, &mut c);
                }
                builder.move_to(point(px(a.x), px(a.y)));
                builder.line_to(point(px(b.x), px(b.y)));
                builder.line_to(point(px(c.x), px(c.y)));
                builder.close();
            }
            if let Ok(path) = builder.build() {
                window.paint_path(path, color);
            }
        } else {
            super::gradient::paint_gradient(object, origin, &triangles, paint, layer, window, cx);
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
    fn sample(paint: &VectorPaint<'_>, transform: Affine, at: ScenePoint, opacity: f32) -> Color {
        let Some(raster) = scene::rasterize_paint(
            paint.paint,
            &paint.triangles,
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
                corner_radii: None,
                local_bounds: scene::Bounds::from_points(
                    ScenePoint::new(12.0, 8.0),
                    ScenePoint::new(35.0, 45.0),
                ),
                corner_radius: 3.0,
                world_transform: Affine::IDENTITY,
            },
            scene::ClipRegion {
                corner_radii: None,
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
            for p in paint.triangles.into_iter().flatten() {
                assert!(p.x >= 14.0 - 0.001 && p.x <= 32.0 + 0.001);
                assert!(p.y >= 10.0 - 0.001 && p.y <= 40.0 + 0.001);
            }
        }
        obj.clips.clear();
        let paints = prepare_vector_paints(&obj, point(px(0.0), px(0.0)), 2.0);
        let points = || paints[2].triangles.iter().flatten();
        assert!((points().map(|p| p.x).fold(f32::INFINITY, f32::min) - 2.0).abs() < 0.01);
        assert!((points().map(|p| p.y).fold(f32::INFINITY, f32::min) + 7.0).abs() < 0.01);
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
            assert_eq!(paints[0].triangles, expected);
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
                corner_radii: None,
                local_bounds: scene::Bounds::from_points(
                    ScenePoint::new(12.0, 4.0),
                    ScenePoint::new(60.0, 16.0),
                ),
                corner_radius: 1.0,
                world_transform: Affine::IDENTITY,
            },
            scene::ClipRegion {
                corner_radii: None,
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
        for p in paints[0].triangles.iter().flatten() {
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
                for data in [
                    "M0 0H40V40H0Z M10 10H30V30H10Z",
                    "M0 0H40V40H0",
                    "M0 0H40V40H0 M10 10H30V30H10Z",
                    "M0 0C12 30 26 -10 40 10",
                    "M0 0H40H0H40",
                ] {
                    for dashed in [false, true] {
                        let mut stroke = Stroke::new(Color::WHITE, 4.)
                            .with_alignment(alignment)
                            .with_cap(scene::StrokeCap::Square)
                            .with_join(scene::StrokeJoin::Miter);
                        if dashed {
                            stroke = stroke.with_dashes(vec![6., 4., 2.], -3.);
                        }
                        let mut obj = object(Style {
                            fill: None,
                            fill_rule: rule,
                            stroke: Some(stroke.clone()),
                            ..Default::default()
                        });
                        obj.object.shape = scene::display::Path::from_svg(data).unwrap().into();
                        obj.screen_transform = Affine::translation(52., 18.)
                            * Affine::rotation(14.)
                            * Affine::scale(-1.7, 0.8);
                        let mesh = obj
                            .object
                            .shape
                            .stroke_mesh_with_rule(&stroke, rule)
                            .unwrap();
                        let origin = point(px(5.), px(7.));
                        let screen = Affine::translation(5., 7.) * obj.screen_transform;
                        let expected: Vec<_> = mesh
                            .triangles()
                            .iter()
                            .map(|t| t.map(|p| screen.transform_point(p)))
                            .collect();
                        for dpr in [1., 1.5, 2.] {
                            let paints = prepare_vector_paints(&obj, origin, dpr);
                            assert_eq!(paints.len(), 1);
                            assert_eq!(paints[0].layer, PaintLayer::Stroke(0));
                            assert_eq!(
                                paints[0].triangles, expected,
                                "{alignment:?} {rule:?} {data} dashed={dashed} dpr={dpr}"
                            );
                        }
                    }
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
                corner_radii: None,
                local_bounds: scene::Bounds::new(
                    ScenePoint::new(8., 8.),
                    scene::Size::new(30., 30.),
                ),
                corner_radius: 4.,
                world_transform: Affine::IDENTITY,
            },
            scene::ClipRegion {
                corner_radii: None,
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
            for p in paint.triangles.iter().flatten() {
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
            corner_radii: None,
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
            assert_eq!(all.triangles, split.triangles);
        }
        // Own stroke is allowed outside the Frame's child clip (x < 10),
        // and is still constrained by the real ancestor clip (x >= 5).
        let points: Vec<_> = strokes[0].triangles.iter().flatten().collect();
        assert!(points.iter().any(|p| p.x < 10.));
        assert!(
            points
                .iter()
                .all(|p| p.x >= 5. - 0.001 && p.x <= 37. + 0.001)
        );
    }
    #[test]
    fn independent_corner_clips_affect_every_native_vector_paint_layer() {
        let mut obj = object(Style {
            fill: Some(Color::WHITE.into()),
            fill_layers: vec![linear(Color::WHITE)],
            ..Default::default()
        });
        obj.object.shape = scene::display::Rect::new(60., 40.).into();
        obj.clips = vec![scene::ClipRegion {
            local_bounds: scene::Bounds::new(ScenePoint::ZERO, scene::Size::new(60., 40.)),
            corner_radius: 0.,
            corner_radii: Some([15., 0., 10., 0.].into()),
            world_transform: Affine::IDENTITY,
        }];
        let paints = prepare_vector_paints(&obj, point(px(0.), px(0.)), 1.);
        assert_eq!(paints.len(), 2);
        for paint in &paints {
            assert_eq!(
                sample(paint, Affine::IDENTITY, ScenePoint::new(1.5, 1.5), 1.),
                Color::TRANSPARENT
            );
            assert_eq!(
                sample(paint, Affine::IDENTITY, ScenePoint::new(58.5, 1.5), 1.),
                Color::WHITE
            );
            assert_eq!(
                sample(paint, Affine::IDENTITY, ScenePoint::new(58.5, 38.5), 1.),
                Color::TRANSPARENT
            );
        }
        drop(paints);
        obj.clips[0].corner_radii = Some([0., 15., 0., 10.].into());
        let paints = prepare_vector_paints(&obj, point(px(0.), px(0.)), 1.);
        for paint in &paints {
            assert_eq!(
                sample(paint, Affine::IDENTITY, ScenePoint::new(1.5, 1.5), 1.),
                Color::WHITE
            );
            assert_eq!(
                sample(paint, Affine::IDENTITY, ScenePoint::new(58.5, 1.5), 1.),
                Color::TRANSPARENT
            );
        }
    }
    #[test]
    fn nested_independent_corner_clips_intersect_in_native_geometry() {
        let mut obj = object(Style::fill(Color::WHITE));
        obj.object.shape = scene::display::Rect::new(60., 40.).into();
        let clip = scene::ClipRegion {
            local_bounds: scene::Bounds::new(ScenePoint::ZERO, scene::Size::new(60., 40.)),
            corner_radius: 0.,
            corner_radii: Some([15., 0., 0., 0.].into()),
            world_transform: Affine::IDENTITY,
        };
        obj.clips = vec![
            clip,
            scene::ClipRegion {
                corner_radii: Some([0., 15., 0., 0.].into()),
                ..clip
            },
        ];
        let paints = prepare_vector_paints(&obj, point(px(0.), px(0.)), 1.);
        assert_eq!(paints.len(), 1);
        assert_eq!(
            sample(&paints[0], Affine::IDENTITY, ScenePoint::new(1.5, 1.5), 1.),
            Color::TRANSPARENT
        );
        assert_eq!(
            sample(&paints[0], Affine::IDENTITY, ScenePoint::new(58.5, 1.5), 1.),
            Color::TRANSPARENT
        );
        assert_eq!(
            sample(
                &paints[0],
                Affine::IDENTITY,
                ScenePoint::new(30.5, 20.5),
                1.
            ),
            Color::WHITE
        );
    }
}

#[cfg(test)]
mod scale_fixed_tests {
    use super::*;
    use gpui_flow_scene::{display, *};
    #[::core::prelude::v1::test]
    fn scale_fixed_native_uses_shared_resolved_triangles_and_rejects_raw_literals() {
        let mut scene = Scene::new();
        scene
            .insert(
                DisplayObject::new("g", display::Group::new())
                    .with_transform(Affine::scale(2., 1.)),
                None,
                None,
            )
            .unwrap();
        scene
            .insert(
                DisplayObject::new("r", display::Rect::new(40., 30.))
                    .with_transform(Affine::rotation(90.))
                    .with_style(Style {
                        fill: None,
                        stroke: Some(
                            Stroke::new(Color::BLACK, 8.)
                                .with_scale_fixed(StrokeScaleFixed::Fixed)
                                .with_alignment(StrokeAlignment::Outside)
                                .with_join(StrokeJoin::Miter),
                        ),
                        ..Default::default()
                    }),
                Some("g".into()),
                None,
            )
            .unwrap();
        for zoom in [0.5, 1., 2.] {
            let viewport = gpui_flow_core::Viewport {
                zoom,
                ..Default::default()
            };
            let list = scene.paint_list().unwrap();
            let item = list.iter().find(|i| i.object.id.as_str() == "r").unwrap();
            let rendered = RenderObject::try_from_paint_item(item, viewport).unwrap();
            assert!(rendered.native_renderable());
            assert_eq!(
                rendered.object.style.stroke.as_ref().unwrap().width,
                4. / zoom
            );
            let resolved = scene.resolved_object(&"r".into(), zoom).unwrap();
            let mesh = resolved
                .shape
                .stroke_mesh_with_rule(
                    resolved.style.stroke.as_ref().unwrap(),
                    resolved.style.fill_rule,
                )
                .unwrap();
            for dpr in [1., 1.5, 2.] {
                let paints = prepare_vector_paints(&rendered, point(px(0.), px(0.)), dpr);
                assert_eq!(paints.len(), 1);
                let expected = transform_triangles(
                    mesh.triangles().iter().copied(),
                    rendered.screen_transform,
                    &[],
                );
                assert_eq!(paints[0].triangles, expected);
            }
            let mut raw = rendered.clone();
            raw.object = item.object.clone();
            assert!(!raw.native_renderable());
            assert!(
                raw.diagnostics().iter().any(|issue| issue.feature
                    == crate::display::UnsupportedFeature::UnresolvedStrokeScale)
            );
        }
    }
    #[::core::prelude::v1::test]
    fn scale_fixed_native_rejects_ambiguous_numeric_view_without_losing_scale_provenance() {
        let mut scene = Scene::new();
        scene
            .insert(
                DisplayObject::new("r", display::Rect::new(40., 30.))
                    .with_transform(Affine::rotation(60.) * Affine::scale(0.5, 2.))
                    .with_style(Style {
                        fill: None,
                        stroke: Some(
                            Stroke::new(Color::BLACK, 8.)
                                .with_scale_fixed(StrokeScaleFixed::Threshold(1.)),
                        ),
                        ..Default::default()
                    }),
                None,
                None,
            )
            .unwrap();
        let items = scene.paint_list().unwrap();
        let view = gpui_flow_core::Viewport {
            zoom: 0.5,
            ..Default::default()
        };
        assert!(RenderObject::try_from_paint_item(&items[0], view).is_err());
        let resolved = RenderObject::try_from_paint_item(
            &items[0],
            gpui_flow_core::Viewport { zoom: 0.49, ..view },
        )
        .unwrap();
        assert_eq!(resolved.object.style.stroke.as_ref().unwrap().width, 8.);
    }
}
