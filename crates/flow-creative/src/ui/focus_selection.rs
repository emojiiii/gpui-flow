use super::*;
use gpui_flow_core::{Editor, Viewport};

fn fitted_selection(editor: &Editor, canvas: WorldSize, panel_height: f32) -> Option<Viewport> {
    if !panel_height.is_finite() || panel_height < 0. {
        return None;
    }
    let bounds = editor.selection_bounds()?;
    let mut viewport = editor.viewport;
    // Leave normal floating-tool space; small windows keep a usable interior.
    let padding = (panel_height + 28.)
        .max(140.)
        .min(canvas.width.min(canvas.height) * 0.4);
    viewport.fit(bounds, canvas, padding).then_some(viewport)
}

impl CreativeCanvas {
    pub(super) fn focus_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let panel_height = self
            .selected(cx)
            .map_or(0., |node| self.composer_size(&node, cx).height);
        let Some(target) =
            fitted_selection(&self.canvas.read(cx).editor, self.canvas_size, panel_height)
        else {
            self.notice = Some(
                tr(
                    "请先选择可见对象或画框",
                    "Select a visible object or frame first",
                )
                .into(),
            );
            cx.notify();
            return;
        };
        if !self.command(
            CanvasCommand::Zoom {
                position: WorldPoint::new(
                    self.canvas_size.width / 2.,
                    self.canvas_size.height / 2.,
                ),
                zoom: target.zoom,
            },
            cx,
        ) {
            return;
        }
        let actual_pan = self.canvas.read(cx).editor.viewport.pan;
        if self.command(
            CanvasCommand::Pan {
                delta: target.pan - actual_pan,
            },
            cx,
        ) {
            self.show_layers = false;
            self.canvas.read(cx).focus_handle(cx).focus(window, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;
    use gpui_flow_core::{FlowDocument, Graph, Group, Rect, Selection};
    use gpui_flow_interaction::{AccessPolicy, PluginHost};

    #[test]
    fn locating_locked_offscreen_selection_uses_navigation_without_editing_graph() {
        let mut node = standard_registry()
            .make_node("image", "far", WorldPoint::new(5000., -2000.), None)
            .unwrap();
        node.locked = true;
        let mut graph = Graph::new();
        graph.add_node(node.clone()).unwrap();
        let mut editor = Editor::new(graph.clone()).unwrap();
        editor.selection.nodes.insert(node.id);
        editor.viewport.pan = WorldPoint::new(300., 800.);
        editor.viewport.zoom = 0.5;
        let canvas = WorldSize::new(1180., 812.);
        let target = fitted_selection(&editor, canvas, 205.).unwrap();
        let mut host = PluginHost::with_defaults();
        host.set_policy(AccessPolicy::read_only(), &mut editor);
        host.execute(
            &mut editor,
            CanvasCommand::Zoom {
                position: WorldPoint::new(590., 406.),
                zoom: target.zoom,
            },
        )
        .unwrap();
        let delta = target.pan - editor.viewport.pan;
        host.execute(&mut editor, CanvasCommand::Pan { delta })
            .unwrap();
        assert_eq!(editor.graph(), &graph);
        assert_eq!(editor.viewport, target);
        let bounds = editor.selection_bounds().unwrap();
        let bottom = target
            .world_to_screen(WorldPoint::new(
                bounds.origin.x,
                bounds.origin.y + bounds.size.height,
            ))
            .y;
        assert!(bottom + 205. + 24. <= canvas.height);
        let center = editor
            .viewport
            .world_to_screen(editor.selection_bounds().unwrap().center());
        assert!((center.x - 590.).abs() < 0.01 && (center.y - 406.).abs() < 0.01);
        let roundtrip = Editor::from_document(
            FlowDocument::from_json(&editor.document().to_json().unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(roundtrip.viewport, target);
        assert_eq!(roundtrip.graph(), &graph);
        host.set_policy(AccessPolicy::locked(), &mut editor);
        let previous = editor.viewport;
        assert!(
            host.execute(
                &mut editor,
                CanvasCommand::Zoom {
                    position: WorldPoint::ZERO,
                    zoom: 0.5
                }
            )
            .is_err()
        );
        assert_eq!(editor.viewport, previous);
    }

    #[test]
    fn selection_fit_ignores_hidden_nodes_supports_frames_and_rejects_empty_or_invalid_canvas() {
        let registry = standard_registry();
        let near = registry
            .make_node("text", "near", WorldPoint::new(100., 100.), None)
            .unwrap();
        let far = registry
            .make_node("text", "hidden", WorldPoint::new(10000., 10000.), None)
            .unwrap();
        let mut graph = Graph::new();
        graph.add_node(near).unwrap();
        graph.add_node(far).unwrap();
        gpui_flow_core::set_node_visible(&mut graph, &"hidden".into(), false).unwrap();
        let mut editor = Editor::new(graph).unwrap();
        let canvas = WorldSize::new(320., 240.);
        assert!(fitted_selection(&editor, canvas, 205.).is_none());
        editor.selection.nodes.insert("hidden".into());
        assert!(fitted_selection(&editor, canvas, 205.).is_none());
        editor.selection.nodes.insert("near".into());
        let visible = fitted_selection(&editor, canvas, 205.).unwrap();
        editor.selection.nodes.remove(&"hidden".into());
        assert_eq!(fitted_selection(&editor, canvas, 205.), Some(visible));
        assert!(fitted_selection(&editor, WorldSize::new(0., 100.), 205.).is_none());
        assert!(fitted_selection(&editor, WorldSize::new(f32::NAN, 100.), 205.).is_none());
        let mut framed = editor.graph().clone();
        let frame = Group::new(
            "frame",
            "Frame",
            Rect::new(WorldPoint::new(20., 30.), WorldSize::new(800., 600.)),
        )
        .with_nodes(["near".into(), "hidden".into()]);
        framed.add_group(frame.clone()).unwrap();
        let mut editor = Editor::new(framed).unwrap();
        editor.selection = Selection {
            groups: [frame.id].into_iter().collect(),
            ..Default::default()
        };
        let target = fitted_selection(&editor, canvas, 205.).unwrap();
        let center = target.world_to_screen(editor.selection_bounds().unwrap().center());
        assert!((center.x - 160.).abs() < 0.01 && (center.y - 120.).abs() < 0.01);
        assert!(target.zoom >= target.min_zoom && target.zoom <= target.max_zoom);
    }
}
