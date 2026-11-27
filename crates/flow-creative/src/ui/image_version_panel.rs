use super::*;
use crate::image_versions::{ImageVersion, ImageVersionOperation};

const PAGE_SIZE: usize = 4;

#[derive(Clone)]
pub(super) enum VersionAction {
    Toggle,
    Page(usize),
    Restore {
        source: Box<Node>,
        version: ImageVersion,
    },
}

pub(super) struct VersionPanel {
    node: NodeId,
    page: usize,
}

impl CreativeCanvas {
    pub(super) fn is_image_versions_open(&self, node: &NodeId) -> bool {
        self.image_versions
            .as_ref()
            .is_some_and(|panel| &panel.node == node)
    }

    pub(super) fn reconcile_image_versions(&mut self, cx: &App) {
        if self.image_versions.as_ref().is_some_and(|panel| {
            self.selected(cx)
                .is_none_or(|node| node.id != panel.node || node.kind != "image")
        }) {
            self.image_versions = None;
        }
    }

    /// Record only an accepted image result, through the same graph/history path.
    /// A rejected result remains in the project library without changing the node.
    pub(super) fn apply_image_version_result(
        &mut self,
        target: &NodeId,
        asset_key: String,
        position: WorldPoint,
        size: WorldSize,
        operation: ImageVersionOperation,
        cx: &mut Context<Self>,
    ) -> bool {
        let mut graph = self.canvas.read(cx).editor.graph().clone();
        let Some(before) = graph.node(target).cloned() else {
            return false;
        };
        let mut after = before.clone();
        after.data["asset"] = Value::String(asset_key);
        after.position = position;
        after.size = size;
        match crate::image_versions::record_edit(&before, &after, operation) {
            Ok(after) => {
                graph.nodes.insert(target.clone(), after);
                self.command(CanvasCommand::ImportGraph(graph), cx)
            }
            Err(error) => {
                self.notice = Some(tr_format!(
                    "对象未接受编辑，结果保留在素材中：{error}",
                    "The object rejected the edit. The result was saved to Project Assets: {error}"
                ));
                false
            }
        }
    }

    pub(super) fn handle_image_versions(
        &mut self,
        action: VersionAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(action, VersionAction::Toggle) && self.image_versions.is_some() {
            self.image_versions = None;
            self.canvas.read(cx).focus_handle(cx).focus(window, cx);
            cx.notify();
            return;
        }
        if self.importing || self.saving || self.project.is_none() {
            return;
        }
        let Some(node) = self.selected(cx).filter(|node| node.kind == "image") else {
            return;
        };
        match action {
            VersionAction::Toggle => {
                self.clear_image_edit();
                self.clear_text_edit();
                self.asset_picker = None;
                self.show_colors = false;
                self.image_versions = Some(VersionPanel {
                    node: node.id,
                    page: 0,
                });
            }
            VersionAction::Page(page) => {
                if let Some(panel) = &mut self.image_versions {
                    panel.page = page.min(3);
                }
            }
            VersionAction::Restore { source, version } => {
                if node != *source {
                    self.notice = Some(
                        tr(
                            "图片已变更，请重新选择版本",
                            "The image changed. Select a version again.",
                        )
                        .into(),
                    );
                } else if self.canvas.read(cx).is_editable()
                    && !crate::layer_guard::node_is_locked(
                        self.canvas.read(cx).editor.graph(),
                        &node.id,
                    )
                {
                    let result = (|| {
                        let project = self
                            .project
                            .as_ref()
                            .ok_or(tr("请先打开本地项目", "Open a local project first"))?;
                        let asset = project
                            .assets()
                            .iter()
                            .find(|asset| asset.key == version.asset_key)
                            .ok_or(tr(
                                "此版本素材已缺失",
                                "The asset for this version is missing",
                            ))?;
                        if !matches!(asset.media_type.as_str(), "image/png" | "image/jpeg") {
                            return Err(tr(
                                "此版本不是静态图片",
                                "This version is not a still image",
                            )
                            .to_string());
                        }
                        project
                            .resolve_asset_path(&asset.key)
                            .map_err(|error| error.to_string())?;
                        let restored = crate::image_versions::restore_version(&node, &version)?;
                        let mut document = self.canvas.read(cx).editor.document();
                        document.graph.nodes.insert(node.id.clone(), restored);
                        project
                            .validate_document(&document, &self.registry)
                            .map_err(|e| e.to_string())?;
                        Ok(document.graph)
                    })();
                    match result {
                        Ok(graph) => {
                            if self.command(CanvasCommand::ImportGraph(graph), cx) {
                                self.notice = Some(tr("已恢复图片版本；原素材保留，可撤销", "Image version restored. The original asset is retained and this change can be undone.").into());
                            }
                        }
                        Err(error) => self.notice = Some(error),
                    }
                }
            }
        }
        self.canvas.read(cx).focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    pub(super) fn image_version_composer(
        &self,
        node: &Node,
        bounds: gpui_flow_core::Rect,
        editable: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let size = self.composer_size(node, cx);
        let at = anchored_panel(bounds, self.canvas_size, size);
        let versions = crate::image_versions::entries(node).unwrap_or_default();
        let page = self
            .image_versions
            .as_ref()
            .map_or(0, |p| p.page)
            .min(versions.len().saturating_sub(1) / PAGE_SIZE);
        let visible = versions
            .iter()
            .rev()
            .skip(page * PAGE_SIZE)
            .take(PAGE_SIZE)
            .cloned()
            .collect::<Vec<_>>();
        let requests = visible
            .iter()
            .filter_map(|version| {
                let project = self.project.as_ref()?;
                let asset = project
                    .assets()
                    .iter()
                    .find(|asset| asset.key == version.asset_key)?;
                Some(asset_thumbnails::ThumbnailRequest {
                    key: asset.key.clone(),
                    path: project.resolve_asset_path(&asset.key).ok()?,
                    media_type: asset.media_type.clone(),
                    byte_len: asset.byte_len,
                })
            })
            .collect();
        self.asset_thumbnails
            .update(cx, |thumbs, cx| thumbs.request(requests, cx));
        let mut rows = div().flex().flex_col().gap_1();
        for (index, version) in visible.into_iter().enumerate() {
            let current = node.data["asset"].as_str() == Some(&version.asset_key)
                && node.size == version.size;
            let available = self
                .project
                .as_ref()
                .is_some_and(|project| project.resolve_asset_path(&version.asset_key).is_ok());
            let thumbnail = self.asset_thumbnails.read(cx).image(&version.asset_key);
            let operation = tr(
                version.operation.label(),
                match version.operation {
                    ImageVersionOperation::Original => "Original",
                    ImageVersionOperation::CropFlip => "Crop and Flip",
                    ImageVersionOperation::Rotate => "Rotate",
                    ImageVersionOperation::Adjust => "Adjust",
                    ImageVersionOperation::BeforeEdit => "Before Edit",
                },
            );
            let title = if current {
                tr("当前版本", "Current version")
            } else {
                operation
            };
            rows = rows.child(
                self.button(
                    SharedString::from(format!("image-version-{index}")),
                    "",
                    Action::ImageVersions(VersionAction::Restore {
                        source: Box::new(node.clone()),
                        version: version.clone(),
                    }),
                    !editable || current || !available || self.saving,
                    cx,
                )
                .w_full()
                .h(px(62.))
                .flex_shrink_0()
                .px_1()
                .flex()
                .items_center()
                .gap_2()
                .accessibility_label(tr_format!(
                    "恢复图片版本 {}",
                    "Restore image version {}",
                    operation
                ))
                .when(current, |v| v.bg(rgb(0xeff4ff)))
                .child(
                    div()
                        .w(px(64.))
                        .h(px(48.))
                        .flex_shrink_0()
                        .overflow_hidden()
                        .bg(rgb(0xf0f1f4))
                        .when_some(thumbnail, |v, image| {
                            v.child(img(image).size_full().object_fit(ObjectFit::Contain))
                        }),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .items_start()
                        .gap_1()
                        .child(div().text_size(px(12.)).child(title))
                        .child(label(tr_format!(
                            "显示 {:.0} × {:.0}",
                            "Display {:.0} × {:.0}",
                            version.size.width,
                            version.size.height
                        )))
                        .when(!available, |v| {
                            v.child(label(tr(
                                "素材缺失，无法恢复",
                                "Asset missing. Restore unavailable.",
                            )))
                        }),
                ),
            );
        }
        if versions.is_empty() {
            rows = rows.child(
                div()
                    .p_3()
                    .text_size(px(12.))
                    .child(tr("后续裁剪、翻转、旋转和调整会在此保留版本。旧编辑不会推测补记。", "New crops, flips, rotations, and adjustments will appear here. Earlier edits are not included.")),
            );
        }
        let content = div()
            .p_2()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(label(tr_format!(
                        "图片编辑版本 · {} / 16",
                        "Image Version History · {} / 16",
                        versions.len()
                    )))
                    .child(self.button(
                        "close-image-versions",
                        tr("关闭", "Close"),
                        Action::ImageVersions(VersionAction::Toggle),
                        false,
                        cx,
                    )),
            )
            .child(rows)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(self.button(
                        "image-version-prev",
                        tr("上一页", "Previous page"),
                        Action::ImageVersions(VersionAction::Page(page.saturating_sub(1))),
                        page == 0,
                        cx,
                    ))
                    .child(label(format!(
                        "{} / {}",
                        page + 1,
                        versions.len().max(1).div_ceil(PAGE_SIZE)
                    )))
                    .child(self.button(
                        "image-version-next",
                        tr("下一页", "Next page"),
                        Action::ImageVersions(VersionAction::Page(page + 1)),
                        (page + 1) * PAGE_SIZE >= versions.len(),
                        cx,
                    )),
            )
            .child(label(tr(
                "恢复保留当前中心 · 原素材不删除 · 可撤销",
                "Restore keeps the center and the original asset · Undo available",
            )));
        div()
            .occlude()
            .absolute()
            .left(px(at.x))
            .top(px(at.y))
            .w(px(size.width))
            .rounded_xl()
            .bg(rgb(0xffffff))
            .shadow_sm()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .id("image-versions-scroll")
                    .max_h(px(size.height))
                    .overflow_y_scroll()
                    .child(content),
            )
    }
}
