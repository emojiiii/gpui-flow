use super::*;

/// The object/selection rectangle is already in screen coordinates. Keep the
/// controls at a readable size and within reach while the canvas moves/zooms.
pub(super) fn toolbar_position(
    bounds: gpui_flow_core::Rect,
    canvas: WorldSize,
    width: f32,
) -> WorldPoint {
    let right = (canvas.width - width - 12.).max(12.);
    let bottom = (canvas.height - 38. - 12.).max(12.);
    WorldPoint::new(
        (bounds.center().x - width / 2.).clamp(78_f32.min(right), right),
        (bounds.origin.y - 68.).clamp(62_f32.min(bottom), bottom),
    )
}

pub(super) fn toolbar_avoiding_composer(
    bounds: gpui_flow_core::Rect,
    canvas: WorldSize,
    width: f32,
    composer: gpui_flow_core::Rect,
) -> WorldPoint {
    let at = toolbar_position(bounds, canvas, width);
    let overlaps = |point: WorldPoint| {
        point.x < composer.origin.x + composer.size.width + 12.
            && point.x + width + 12. > composer.origin.x
            && point.y < composer.origin.y + composer.size.height + 12.
            && point.y + 38. + 12. > composer.origin.y
    };
    if !overlaps(at) {
        return at;
    }
    let bottom = (canvas.height - 50.).max(12.);
    for y in [
        composer.origin.y - 50.,
        composer.origin.y + composer.size.height + 12.,
    ] {
        let candidate = WorldPoint::new(at.x, y);
        if y >= 62_f32.min(bottom) && y <= bottom && !overlaps(candidate) {
            return candidate;
        }
    }
    at
}

impl CreativeCanvas {
    pub(super) fn composer_size(&self, node: &Node, cx: &App) -> WorldSize {
        if node.kind == "mask" {
            return WorldSize::new(360., 164.);
        }
        if matches!(node.kind.as_str(), "video" | "audio") {
            return WorldSize::new(272., 44.);
        }
        if node.kind == "text" {
            return WorldSize::new(112., 32.);
        }
        if node.kind == "image" && !self.quick_edit {
            return WorldSize::new(104., 32.);
        }
        let generation = GenerationKind::from_node_kind(&node.kind).is_some();
        let model = matches!(node.kind.as_str(), "model" | "generate_model")
            && !node.data["asset"].as_str().unwrap_or_default().is_empty();
        let height = if generation {
            if self.media_views.snapshot(&node.id, cx).is_some() {
                210.
            } else {
                170.
            }
        } else if model {
            112.
        } else {
            104.
        };
        WorldSize::new(
            if generation { 360. } else { 320. },
            height
                + if node.kind == "edit_image" { 44. } else { 0. }
                + if generation && crate::references::accepts_image_reference(node) {
                    56.
                } else {
                    0.
                },
        )
    }

    pub(super) fn selection_toolbar(
        &self,
        count: usize,
        editable: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        div()
            .occlude()
            .w(px(230.))
            .h(px(38.))
            .px_1()
            .rounded(px(10.))
            .bg(rgb(0xffffff))
            .shadow_sm()
            .border_1()
            .border_color(rgb(0xe9eaed))
            .flex()
            .items_center()
            .gap_1()
            .child(
                div()
                    .flex_1()
                    .text_size(px(12.))
                    .text_color(rgb(0x5b606a))
                    .child(format!("{count} 个对象")),
            )
            .child(chrome::separator())
            .child(self.icon_button(
                "selection-copy",
                "copy",
                "复制所选对象",
                Action::Duplicate,
                !editable,
                cx,
            ))
            .child(self.icon_button(
                "selection-frame",
                "grid-2x2",
                "编组所选对象",
                Action::Frame,
                !editable,
                cx,
            ))
            .child(self.icon_button(
                "selection-delete",
                "trash",
                "删除所选对象",
                Action::Delete,
                !editable,
                cx,
            ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;
    use gpui_flow_core::{Point, Rect, Size};

    #[test]
    fn toolbar_stays_reachable_when_selection_moves_beyond_any_edge() {
        let canvas = Size::new(1180., 812.);
        for origin in [
            Point::new(-800., -600.),
            Point::new(1200., -100.),
            Point::new(-100., 900.),
            Point::new(1200., 900.),
        ] {
            for width in [184., 230., 334., 456.] {
                let p = toolbar_position(Rect::new(origin, Size::new(400., 300.)), canvas, width);
                assert!(p.x >= 78. && p.x + width <= canvas.width - 12.);
                assert!(p.y >= 62. && p.y + 38. <= canvas.height - 12.);
            }
        }
    }

    #[test]
    fn toolbar_tracks_screen_bounds_without_scaling_controls() {
        let canvas = Size::new(1600., 1000.);
        for zoom in [0.5, 1., 2.] {
            let bounds = Rect::new(
                Point::new(400. * zoom, 300. * zoom),
                Size::new(300. * zoom, 200. * zoom),
            );
            let p = toolbar_position(bounds, canvas, 230.);
            assert_eq!(p.x + 115., bounds.center().x);
            assert_eq!(p.y + 68., bounds.origin.y);
        }
    }

    #[test]
    fn offscreen_object_controls_do_not_cover_each_other() {
        let canvas = Size::new(1180., 812.);
        for origin in [Point::new(1250., 980.), Point::new(-500., -600.)] {
            for size in [Size::new(112., 32.), Size::new(360., 226.)] {
                let bounds = Rect::new(origin, Size::new(400., 300.));
                let composer = Rect::new(anchored_panel(bounds, canvas, size), size);
                let at = toolbar_avoiding_composer(bounds, canvas, 456., composer);
                assert!(
                    at.y + 38. + 12. <= composer.origin.y
                        || at.y >= composer.origin.y + size.height + 12.
                        || at.x + 456. + 12. <= composer.origin.x
                        || at.x >= composer.origin.x + size.width + 12.
                );
                assert!(at.y >= 62. && at.y + 38. <= canvas.height - 12.);
            }
        }
    }
}
