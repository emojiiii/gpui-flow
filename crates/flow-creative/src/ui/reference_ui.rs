use super::*;
use crate::references::linked_reference;

impl CreativeCanvas {
    pub(super) fn reference_row(&self, node: &Node, cx: &mut Context<Self>) -> Div {
        let linked = linked_reference(self.canvas.read(cx).editor.graph(), &node.id).cloned();
        let resolved = self
            .canvas
            .read(cx)
            .editor
            .graph()
            .edges
            .values()
            .find(|e| e.target.node == node.id && e.target.port.as_str() == "reference")
            .and_then(|e| {
                crate::custom_nodes::resolve_graph_output(
                    self.canvas.read(cx).editor.graph(),
                    &e.source,
                )
                .ok()
            });
        let path = resolved
            .as_ref()
            .and_then(|v| v.asset_key())
            .or_else(|| {
                linked
                    .as_ref()
                    .filter(|source| {
                        !source.kind.starts_with("custom.")
                            && crate::processing::ProcessingKind::from_node_kind(&source.kind)
                                .is_none()
                    })
                    .and_then(|source| source.data["asset"].as_str())
            })
            .and_then(|key| {
                self.project
                    .as_ref()
                    .and_then(|project| project.resolve_asset_path(key).ok())
                    .or_else(|| CreativeAssets::demo().paths.get(key).cloned())
            });
        let image_generation = node.kind == "generate_image";
        let mut row = div().flex().items_center().gap_2().py_1();
        if let Some(source) = linked {
            row = row
                .child(
                    div()
                        .w(px(48.))
                        .h(px(44.))
                        .flex_shrink_0()
                        .rounded(px(6.))
                        .overflow_hidden()
                        .bg(rgb(0xeef1f5))
                        .flex()
                        .items_center()
                        .justify_center()
                        .when_some(path.clone(), |thumb, path| {
                            thumb.child(img(path).size_full().object_fit(ObjectFit::Cover))
                        })
                        .when(path.is_none(), |thumb| {
                            thumb.child(chrome::icon("image", 18.).text_color(rgb(0xaab4c5)))
                        }),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .text_size(px(11.))
                        .text_color(rgb(0x8b919c))
                        .child(if path.is_some() {
                            format!(
                                "{} · {}",
                                if image_generation {
                                    "图生图"
                                } else {
                                    "图片输入"
                                },
                                source.title.chars().take(24).collect::<String>(),
                            )
                        } else {
                            "已连接图片输入，等待源节点提供图片".into()
                        }),
                );
        } else {
            row = row.child(
                div()
                    .flex_1()
                    .text_size(px(11.))
                    .text_color(rgb(0x9a9fa9))
                    .child(if image_generation {
                        "文生图 · 连接图片节点可使用图生图"
                    } else {
                        "可连接图片节点作为输入"
                    }),
            );
        }
        row
    }
    pub(super) fn reference_prompt(
        &self,
        node: &Node,
        editable: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        div()
            .id(SharedString::from(format!("generation-prompt-{}", node.id)))
            .on_drop(cx.listener(|this, _: &ExternalPaths, _, cx| {
                cx.stop_propagation();
                this.notice = Some("请将图片导入画布，再把图片节点连接到生成器的图片输入".into());
                cx.notify();
            }))
            .child(
                Textarea::new(&self.form)
                    .h(px(104.))
                    .bordered(false)
                    .appearance(false)
                    .text_size(px(13.))
                    .disabled(!editable)
                    .aria_label("生成提示词"),
            )
    }
}
