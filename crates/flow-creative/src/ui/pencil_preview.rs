//! One retained Scene path per stroke; no image or mask asset is manufactured.
use super::*;
use crate::pencil::{PencilData, PencilStyle};
use gpui_flow::display::{DisplayElement, RenderObject};
use gpui_flow_scene::{self as scene, Affine, Color, DisplayObject, Shape, Stroke, Style};

pub(super) fn object(id: &str, points: &[WorldPoint], style: PencilStyle) -> DisplayObject {
    let first = points[0];
    if points.iter().all(|p| *p == first) {
        return DisplayObject::new(id, scene::display::Ellipse::circle(style.stroke_width / 2.))
            .with_transform(Affine::translation(
                first.x - style.stroke_width / 2.,
                first.y - style.stroke_width / 2.,
            ))
            .with_style(Style {
                fill: Some(Color::hex(style.color).into()),
                opacity: style.opacity,
                ..Default::default()
            });
    }
    let commands = std::iter::once(scene::display::PathCommand::MoveTo { to: first })
        .chain(
            points
                .iter()
                .skip(1)
                .map(|p| scene::display::PathCommand::LineTo { to: *p }),
        )
        .collect();
    DisplayObject::new(id, Shape::Path(scene::display::Path::new(commands))).with_style(Style {
        fill: None,
        stroke: Some(Stroke::new(Color::hex(style.color), style.stroke_width)),
        opacity: style.opacity,
        ..Default::default()
    })
}

fn rendered(object: DisplayObject, view: Affine) -> AnyElement {
    let screen_transform = view * object.transform;
    div()
        .relative()
        .size_full()
        .child(DisplayElement::new(RenderObject {
            opacity: object.style.opacity,
            object,
            screen_transform,
            viewport_transform: view,
            clips: vec![],
        }))
        .into_any_element()
}

pub(super) fn object_for_node(node: &Node) -> Result<DisplayObject, String> {
    let data = PencilData::from_node(node)?;
    let points = data
        .points
        .iter()
        .map(|p| WorldPoint::new(p.x * node.size.width, p.y * node.size.height))
        .collect::<Vec<_>>();
    Ok(object(node.id.as_str(), &points, data.style))
}

pub(super) fn render(node: &Node, zoom: f32) -> AnyElement {
    match object_for_node(node) {
        Ok(object) => rendered(object, Affine::scale(zoom, zoom)),
        Err(_) => div()
            .child(tr("笔迹数据无效", "Invalid stroke data"))
            .into_any_element(),
    }
}

pub(super) fn preview(
    points: &[WorldPoint],
    style: PencilStyle,
    viewport: gpui_flow_core::Viewport,
) -> AnyElement {
    rendered(
        object("pencil-draft", points, style),
        Affine::translation(viewport.pan.x, viewport.pan.y)
            * Affine::scale(viewport.zoom, viewport.zoom),
    )
}

pub(super) fn hit(node: &Node, local: WorldPoint, tolerance: f32) -> bool {
    if !local.is_finite() || !node.size.is_positive() || !tolerance.is_finite() || tolerance < 0. {
        return false;
    }
    // Reject distant objects before decoding their bounded point arrays.
    if !scene::Bounds::new(WorldPoint::ZERO, node.size)
        .expanded(crate::pencil::MAX_PENCIL_STROKE_WIDTH / 2. + tolerance)
        .contains(local)
    {
        return false;
    }
    let Ok(data) = PencilData::from_node(node) else {
        return false;
    };
    if data.style.opacity == 0. {
        return false;
    }
    let points = data
        .points
        .iter()
        .map(|p| WorldPoint::new(p.x * node.size.width, p.y * node.size.height))
        .collect::<Vec<_>>();
    let object = object(node.id.as_str(), &points, data.style);
    let local = WorldPoint::new(local.x - object.transform.tx, local.y - object.transform.ty);
    object.shape.hit_test(local, &object.style, tolerance)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[::core::prelude::v1::test]
    fn path_and_dot_pick_painted_geometry_instead_of_the_rectangle() {
        let style = PencilStyle {
            stroke_width: 4.,
            opacity: 0.5,
            ..Default::default()
        };
        let node = crate::pencil::create_pencil_node_from_points(
            "p",
            "stroke",
            &[
                WorldPoint::new(10., 10.),
                WorldPoint::new(110., 10.),
                WorldPoint::new(110., 110.),
            ],
            style,
        )
        .unwrap();
        assert!(hit(&node, WorldPoint::new(50., 1.), 1.));
        assert!(!hit(&node, WorldPoint::new(50., 50.), 1.));
        let dot = crate::pencil::create_pencil_node_from_points(
            "d",
            "dot",
            &[WorldPoint::new(10., 10.)],
            style,
        )
        .unwrap();
        assert!(hit(&dot, WorldPoint::new(0.5, 0.5), 0.));
        assert!(!hit(&dot, WorldPoint::new(4., 4.), 0.));
        let object = object("p", &[WorldPoint::ZERO, WorldPoint::new(10., 10.)], style);
        let render = RenderObject {
            object,
            screen_transform: Affine::IDENTITY,
            viewport_transform: Affine::IDENTITY,
            opacity: 0.5,
            clips: vec![],
        };
        assert!(render.diagnostics().is_empty());
    }
}
