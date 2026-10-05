use super::*;
use crate::references::{
    accepts_image_reference, linked_reference, reference_asset, replace_reference,
};

impl CreativeCanvas {
    pub(super) fn set_reference(
        &mut self,
        target: &NodeId,
        asset: Option<&str>,
        cx: &mut Context<Self>,
    ) -> bool {
        let result = replace_reference(
            &self.registry,
            self.canvas.read(cx).editor.graph(),
            target,
            asset,
        )
        .and_then(|graph| {
            if let Some(project) = &self.project {
                let mut document = self.canvas.read(cx).editor.document();
                document.graph = graph.clone();
                project
                    .validate_document(&document, &self.registry)
                    .map_err(|e| CreativeError(e.to_string()))?;
            }
            Ok(graph)
        });
        match result {
            Ok(graph) => self.command(CanvasCommand::ImportGraph(graph), cx),
            Err(error) => {
                self.notice = Some(error.to_string());
                cx.notify();
                false
            }
        }
    }
    pub(super) fn import_reference_paths(
        &mut self,
        target: NodeId,
        paths: Vec<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        self.reference_target = Some(target);
        self.import_model = false;
        self.import_image_request(false, None, std::future::ready(Ok(Some(paths))), cx);
    }
    pub(super) fn reference_row(&self, node: &Node, editable: bool, cx: &mut Context<Self>) -> Div {
        let linked = linked_reference(self.canvas.read(cx).editor.graph(), &node.id).cloned();
        let attached = reference_asset(node);
        let key = attached.or_else(|| {
            linked
                .as_ref()
                .and_then(|source| source.data["asset"].as_str())
        });
        let path = key.and_then(|key| {
            self.project
                .as_ref()
                .and_then(|project| project.resolve_asset_path(key).ok())
                .or_else(|| CreativeAssets::demo().paths.get(key).cloned())
        });
        let name = attached
            .and_then(|key| {
                self.project
                    .as_ref()?
                    .assets()
                    .iter()
                    .find(|asset| asset.key == key)
                    .map(|a| a.name.clone())
            })
            .or_else(|| linked.as_ref().map(|source| source.title.clone()));
        let has_reference = attached.is_some() || linked.is_some();
        let mut row = div().flex().items_center().gap_2().py_1();
        if has_reference {
            let thumbnail = div()
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
                });
            row = row
                .child(thumbnail)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .text_size(px(11.))
                        .text_color(rgb(0x8b919c))
                        .child(if path.is_some() {
                            name.unwrap_or_else(|| "图片参考".into())
                                .chars()
                                .take(26)
                                .collect::<String>()
                        } else {
                            "参考节点尚无图片".into()
                        }),
                )
                .child(self.icon_button(
                    "remove-reference",
                    "x",
                    "移除参考，不删除素材",
                    Action::ClearReference(node.id.clone()),
                    !editable,
                    cx,
                ));
        } else {
            row = row.child(
                div()
                    .flex_1()
                    .text_size(px(11.))
                    .text_color(rgb(0x9a9fa9))
                    .child("将一张图片拖入提示框作为参考"),
            );
        }
        row.child(self.icon_button(
            "upload-reference",
            "image-plus",
            if has_reference {
                "更换参考图"
            } else {
                "添加参考图"
            },
            Action::ImportReference(node.id.clone()),
            !editable || self.project.is_none() || self.saving || self.importing,
            cx,
        ))
    }
    pub(super) fn reference_prompt(
        &self,
        node: &Node,
        editable: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let target = node.id.clone();
        let supports_image = accepts_image_reference(node);
        div()
            .id(SharedString::from(format!("prompt-drop-{}", node.id)))
            .on_drop(cx.listener(move |this, paths: &ExternalPaths, _, cx| {
                cx.stop_propagation();
                if supports_image {
                    this.import_reference_paths(target.clone(), paths.paths().to_vec(), cx);
                } else {
                    this.notice = Some("这个生成节点只接收文字提示，图片未添加为参考".into());
                    cx.notify();
                }
            }))
            .child(
                Textarea::new(&self.form)
                    .h(px(104.))
                    .bordered(false)
                    .appearance(false)
                    .text_size(px(13.))
                    .disabled(!editable)
                    .aria_label("生成提示词，可拖入一张图片参考"),
            )
    }
}
