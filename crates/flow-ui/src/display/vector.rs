use super::{RenderObject, clip::clip_polygon};
use gpui::*;
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
/// Tessellate in local space, then transform triangles. Non-uniform scales preserve stroke geometry.
pub(super) fn paint_vector(
    object: &RenderObject,
    origin: Point<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    super::gradient::clear_object_issues(
        window.window_handle().window_id().as_u64(),
        object.object.id.as_str(),
    );
    let screen =
        scene::Affine::translation(origin.x.into(), origin.y.into()) * object.screen_transform;
    let contours = object
        .object
        .shape
        .flatten(0.25 / (object.screen_transform.max_scale() * window.scale_factor()).max(0.01));
    let clips: Vec<Vec<scene::Point>> = object
        .clips
        .iter()
        .map(|clip| {
            let transform = scene::Affine::translation(origin.x.into(), origin.y.into())
                * object.viewport_transform
                * clip.world_transform;
            scene::display::Rect::new(clip.local_bounds.size.width, clip.local_bounds.size.height)
                .rounded(clip.corner_radius)
                .flatten(0.25 / (transform.max_scale() * window.scale_factor()).max(0.01))
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
    if let Some(fill) = &object.object.style.fill {
        let rule = match object.object.style.fill_rule {
            scene::FillRule::NonZero => FillRule::NonZero,
            scene::FillRule::EvenOdd => FillRule::EvenOdd,
        };
        if let Some(path) = build(
            PathBuilder::fill()
                .with_style(PathStyle::Fill(FillOptions::default().with_fill_rule(rule))),
        ) {
            if let Some(color) = color(fill, object.opacity) {
                window.paint_path(path, color);
            } else {
                super::gradient::paint_gradient(object, origin, path, fill, false, window, cx);
            }
        }
    }
    if let Some(stroke) = &object.object.style.stroke
        && stroke.width > 0.0
        && let Some(path) = build(
            PathBuilder::stroke(px(stroke.width)).with_style(PathStyle::Stroke(
                StrokeOptions::default()
                    .with_line_width(stroke.width)
                    .with_line_cap(lyon::path::LineCap::Round)
                    .with_line_join(lyon::path::LineJoin::Round),
            )),
        )
    {
        if let Some(color) = color(&stroke.paint, object.opacity) {
            window.paint_path(path, color);
        } else {
            super::gradient::paint_gradient(object, origin, path, &stroke.paint, true, window, cx);
        }
    }
}
