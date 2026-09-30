use gpui_flow_core::{Editor, Graph, Node, Point};
use gpui_flow_interaction::*;
struct SnapToTen;
impl CanvasPlugin for SnapToTen {
    fn id(&self) -> &str {
        "snapping"
    }
    fn priority(&self) -> i32 {
        50
    }
    fn filter_command(
        &mut self,
        command: &mut CanvasCommand,
        _: &PluginContext<'_>,
        _: &mut Vec<CanvasEffect>,
    ) -> Result<(), String> {
        if let CanvasCommand::SetPositions { nodes, groups } = command {
            for position in nodes.values_mut().chain(groups.values_mut()) {
                *position = position.snapped(10.);
            }
        }
        Ok(())
    }
}
fn main() {
    let mut graph = Graph::new();
    graph
        .add_node(Node::new("box", "Box", Point::ZERO))
        .unwrap();
    let mut editor = Editor::new(graph).unwrap();
    let mut host = PluginHost::with_defaults();
    host.replace(SnapToTen, &mut editor).unwrap();
    for event in [
        CanvasEvent::PointerDown(PointerInput::left(
            Point::new(100., 70.),
            HitTarget::Node("box".into()),
        )),
        CanvasEvent::PointerMove(PointerInput::left(
            Point::new(142., 110.),
            HitTarget::Node("box".into()),
        )),
        CanvasEvent::PointerUp(PointerInput::left(
            Point::new(142., 110.),
            HitTarget::Node("box".into()),
        )),
    ] {
        assert!(host.dispatch(&mut editor, event).errors.is_empty());
    }
    assert_eq!(
        editor.graph().nodes[&"box".into()].position,
        Point::new(40., 40.)
    );
    host.execute(&mut editor, CanvasCommand::Undo).unwrap();
    assert_eq!(editor.graph().nodes[&"box".into()].position, Point::ZERO);
    host.set_editable(false, &mut editor);
    assert!(
        host.execute(&mut editor, CanvasCommand::DeleteSelection)
            .is_err()
    );
    host.set_enabled("connection", false, &mut editor).unwrap();
    assert!(!host.is_enabled("connection"));
    println!("Replaceable snapping, gesture undo and read-only command enforcement passed");
}
