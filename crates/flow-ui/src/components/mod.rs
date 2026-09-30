//! Standalone GPUI elements. None requires a `FlowCanvas` or owns an editor.
//! Compose them in any GPUI view and supply host-owned event callbacks.
pub mod button;
pub mod inspector;
pub mod minimap;
pub mod node_frame;
pub mod palette;
pub mod port;
pub mod primitives;
pub mod toolbar;
pub mod viewport_controls;

pub use button::*;
pub use inspector::*;
pub use minimap::*;
pub use node_frame::*;
pub use palette::*;
pub use port::*;
pub use primitives::*;
pub use toolbar::*;
pub use viewport_controls::*;

/// GPUI pointer callbacks accept ordinary closures or `Context::listener` results.
pub type PointerCallback = Box<dyn Fn(&gpui::MouseDownEvent, &mut gpui::Window, &mut gpui::App)>;
pub type ClickCallback = Box<dyn Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App)>;

pub(crate) fn valid_zoom(zoom: f32) -> f32 {
    if zoom.is_finite() && zoom > 0. {
        zoom
    } else {
        1.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_flow_core::{DataType, Point, PortDirection, Rect, Size};

    #[test]
    fn standalone_builder_keeps_logical_geometry_and_typed_ports() {
        let frame = NodeFrame::new("node", "Reusable")
            .position(Point::new(12., 18.))
            .size(Size::new(200., 120.))
            .zoom(2.)
            .selected(true);
        assert_eq!(frame.position, Some(Point::new(12., 18.)));
        assert_eq!(frame.size, Size::new(200., 120.));
        assert_eq!(frame.zoom, 2.);
        assert!(frame.selected);
        assert_eq!(NodeFrame::new("bad-zoom", "").zoom(f32::NAN).zoom, 1.);
        let handle = Handle::new("port", PortDirection::Output, DataType::String)
            .anchor(Point::new(200., 60.))
            .label("Text")
            .diameter(-2.);
        assert_eq!(handle.direction, PortDirection::Output);
        assert_eq!(handle.data_type, DataType::String);
        assert_eq!(handle.diameter, 12.);
        assert_eq!(handle.anchor, Some(Point::new(200., 60.)));
    }

    #[test]
    fn minimap_navigation_round_trip_includes_viewport_and_nodes() {
        let visible = Rect::new(Point::new(-200., -100.), Size::new(600., 400.));
        let far = Rect::new(Point::new(800., 600.), Size::new(100., 100.));
        let map = MiniMap::new("map", visible)
            .node(MiniMapNode::new(far, 0xffffff))
            .transform()
            .unwrap();
        assert_eq!(map.world_bounds, visible.union(far));
        let original = Point::new(430., 290.);
        let round_trip = map.map_to_world(map.world_to_map(original));
        assert!((round_trip.x - original.x).abs() < 0.001);
        assert!((round_trip.y - original.y).abs() < 0.001);
        assert!(
            MiniMap::new("invalid-map", visible)
                .size(Size::new(1., 1.))
                .transform()
                .is_none()
        );
    }
}
