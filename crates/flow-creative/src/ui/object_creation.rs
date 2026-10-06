use super::*;

impl CreativeCanvas {
    /// Toolbar creation and asset reuse search for space; explicit draw/drop
    /// gestures retain their own exact positions and existing membership rules.
    pub(super) fn add_object_in_free_space(
        &mut self,
        mut node: Node,
        cx: &mut Context<Self>,
    ) -> bool {
        let canvas = self.canvas.read(cx);
        let placement = crate::object_placement::place_object(
            canvas.editor.graph(),
            node.position,
            node.size,
            canvas.editor.viewport,
            self.canvas_size,
        );
        let placement = match placement {
            Ok(placement) => placement,
            Err(error) => {
                self.notice = Some(format!("无法放置新对象：{error}"));
                cx.notify();
                return false;
            }
        };
        node.position = placement.position;
        let id = node.id.clone();
        if !self.command(CanvasCommand::AddNode(node), cx) {
            return false;
        }
        self.command(
            CanvasCommand::SelectNode {
                node: id,
                mode: SelectionMode::Replace,
            },
            cx,
        );
        if placement.pan_delta != WorldPoint::ZERO {
            self.command(
                CanvasCommand::Pan {
                    delta: placement.pan_delta,
                },
                cx,
            );
        }
        true
    }
}
