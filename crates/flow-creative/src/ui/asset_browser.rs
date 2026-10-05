use super::asset_thumbnails::ThumbnailRequest;
use super::*;

const PAGE_SIZE: usize = 8;
pub(super) struct AssetPicker {
    pub target: Option<NodeId>,
    pub page: usize,
}

fn asset_kind(asset: &ProjectAsset) -> &'static str {
    if asset.media_type.starts_with("image/") {
        "image"
    } else if asset.media_type.starts_with("video/") {
        "video"
    } else if asset.media_type.starts_with("audio/") {
        "audio"
    } else {
        "model"
    }
}
fn asset_description(asset: &ProjectAsset) -> String {
    match asset_kind(asset) {
        "image" | "video" => format!("{} × {}", asset.width, asset.height),
        "audio" => "音频".into(),
        _ => "GLB 模型".into(),
    }
}
impl CreativeCanvas {
    pub(super) fn place_project_asset(&mut self, key: &str, cx: &mut Context<Self>) {
        if !self.canvas.read(cx).is_editable() {
            return;
        }
        let Some(project) = &self.project else {
            return;
        };
        let Some(asset) = project
            .assets()
            .iter()
            .find(|asset| asset.key == key)
            .cloned()
        else {
            self.notice = Some("这个素材已不在当前项目中".into());
            cx.notify();
            return;
        };
        if let Err(error) = project.resolve_asset_path(key) {
            self.notice = Some(format!("素材无法读取：{error}"));
            cx.notify();
            return;
        }
        if let Some(target) = self
            .asset_picker
            .as_ref()
            .and_then(|picker| picker.target.clone())
        {
            if self.set_reference(&target, Some(key), cx) {
                self.asset_picker = None;
            }
            return;
        }
        let kind = asset_kind(&asset);
        let size = match kind {
            "audio" => WorldSize::new(400., 144.),
            "model" => WorldSize::new(420., 315.),
            _ => project_ui::imported_image_size(asset.width, asset.height),
        };
        let view = self.canvas.read(cx);
        let id: NodeId = view.editor.graph().fresh_id(kind).into();
        let center = view.editor.viewport.screen_to_world(WorldPoint::new(
            self.canvas_size.width / 2.,
            self.canvas_size.height / 2.,
        ));
        let at = WorldPoint::new(center.x - size.width / 2., center.y - size.height / 2.);
        match self.registry.make_node(
            kind,
            id.clone(),
            at,
            Some(serde_json::json!({"asset":key,"caption":""})),
        ) {
            Ok(mut node) => {
                node.title = asset.name;
                node.size = size;
                if self.command(CanvasCommand::AddNode(node), cx) {
                    self.asset_picker = None;
                    self.command(
                        CanvasCommand::SelectNode {
                            node: id,
                            mode: SelectionMode::Replace,
                        },
                        cx,
                    );
                }
            }
            Err(error) => {
                self.notice = Some(error.to_string());
                cx.notify();
            }
        }
    }
    pub(super) fn asset_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let picker = self.asset_picker.as_ref().expect("open picker");
        let target = picker.target.clone();
        let assets = self
            .project
            .as_ref()
            .map(|project| {
                project
                    .assets()
                    .iter()
                    .rev()
                    .filter(|asset| target.is_none() || asset.media_type.starts_with("image/"))
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let page = picker.page.min(assets.len().saturating_sub(1) / PAGE_SIZE);
        let visible = assets.iter().skip(page * PAGE_SIZE).take(PAGE_SIZE);
        let requests = visible
            .clone()
            .filter(|asset| asset.media_type.starts_with("image/"))
            .filter_map(|asset| {
                Some(ThumbnailRequest {
                    key: asset.key.clone(),
                    path: self.project.as_ref()?.resolve_asset_path(&asset.key).ok()?,
                    media_type: asset.media_type.clone(),
                    byte_len: asset.byte_len,
                })
            })
            .collect();
        self.asset_thumbnails
            .update(cx, |thumbs, cx| thumbs.request(requests, cx));
        let editable = self.canvas.read(cx).is_editable();
        let mut grid = div().flex().flex_wrap().gap_2();
        for asset in visible {
            let thumbnail = self.asset_thumbnails.read(cx).image(&asset.key);
            let preview_status = (asset_kind(asset) == "image" && thumbnail.is_none()).then(|| {
                if self.asset_thumbnails.read(cx).is_loading(&asset.key) {
                    "正在加载预览…"
                } else {
                    "预览不可用"
                }
            });
            let name = asset.name.clone();
            let icon = match asset_kind(asset) {
                "video" => "video",
                "audio" => "music",
                "model" => "box",
                _ => "image",
            };
            grid = grid.child(
                self.button(
                    SharedString::from(format!("asset-{}", asset.key)),
                    "",
                    Action::PlaceAsset(asset.key.clone()),
                    !editable,
                    cx,
                )
                .w(px(140.))
                .h(px(144.))
                .p_1()
                .rounded_lg()
                .flex()
                .flex_col()
                .gap_1()
                .accessibility_label(SharedString::from(format!("选择素材 {}", asset.name)))
                .child(
                    div()
                        .w_full()
                        .h(px(88.))
                        .flex_shrink_0()
                        .rounded(px(7.))
                        .overflow_hidden()
                        .bg(rgb(0xeff1f5))
                        .flex()
                        .items_center()
                        .justify_center()
                        .when_some(thumbnail.clone(), |thumb, image| {
                            thumb.child(img(image).size_full().object_fit(ObjectFit::Contain))
                        })
                        .when(thumbnail.is_none(), |thumb| {
                            thumb
                                .flex_col()
                                .gap_1()
                                .child(chrome::icon(icon, 24.).text_color(rgb(0xaeb8c8)))
                                .when_some(preview_status, |thumb, status| {
                                    thumb.child(
                                        div()
                                            .text_size(px(10.))
                                            .text_color(rgb(0x929aa8))
                                            .child(status),
                                    )
                                })
                        }),
                )
                .child(
                    div()
                        .w_full()
                        .overflow_hidden()
                        .text_size(px(11.))
                        .child(name.chars().take(19).collect::<String>()),
                )
                .child(
                    div()
                        .w_full()
                        .text_size(px(10.))
                        .text_color(rgb(0x929aa8))
                        .child(asset_description(asset)),
                ),
            );
        }
        if assets.is_empty() {
            grid = grid.child(
                div()
                    .p_4()
                    .text_size(px(12.))
                    .text_color(rgb(0x929aa8))
                    .child(if target.is_some() {
                        "本项目还没有图片。导入一张图片作为参考。"
                    } else {
                        "导入的素材会保留在这里，随时放回画布。"
                    }),
            );
        }
        let mut panel = div()
            .id("project-asset-picker")
            .track_focus(&self.asset_picker_focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    this.action(Action::CloseAssets, window, cx);
                    cx.stop_propagation();
                }
            }))
            .occlude()
            .absolute()
            .left(px(78.))
            .top(px(74.))
            .w(px(316.))
            .p_3()
            .rounded(px(14.))
            .bg(rgb(0xffffff))
            .shadow_md()
            .border_1()
            .border_color(rgb(0xe9eaed))
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().text_size(px(13.)).child(if target.is_some() {
                        "选择参考图"
                    } else {
                        "本项目素材"
                    }))
                    .child(self.icon_button(
                        "close-assets",
                        "x",
                        "关闭素材",
                        Action::CloseAssets,
                        false,
                        cx,
                    )),
            )
            .child(
                div()
                    .id("project-asset-grid")
                    .max_h(px((self.canvas_size.height - 244.).max(120.)))
                    .overflow_y_scroll()
                    .child(grid),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(rgb(0x959ca8))
                            .child(format!(
                                "{} 个素材 · {} / {}",
                                assets.len(),
                                page + 1,
                                assets.len().div_ceil(PAGE_SIZE).max(1)
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .child(self.icon_button(
                                "asset-prev",
                                "chevron-left",
                                "上一页",
                                Action::AssetsPage(false),
                                page == 0,
                                cx,
                            ))
                            .child(self.icon_button(
                                "asset-next",
                                "chevron-right",
                                "下一页",
                                Action::AssetsPage(true),
                                (page + 1) * PAGE_SIZE >= assets.len(),
                                cx,
                            )),
                    ),
            );
        let import_action = target.map_or(Action::ImportImage, Action::ImportReference);
        panel = panel.child(
            self.button(
                "asset-import",
                "＋ 导入本地素材",
                import_action,
                !editable || self.saving,
                cx,
            )
            .w_full()
            .bg(rgb(0xf0f3f8)),
        );
        panel.into_any_element()
    }
}
