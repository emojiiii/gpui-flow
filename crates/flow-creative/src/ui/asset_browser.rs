use super::asset_thumbnails::ThumbnailRequest;
use super::*;
use crate::asset_filter::AssetFilter;
use gpui_kit::component::input::{Input, InputState};

const PAGE_SIZE: usize = 8;
pub(super) struct AssetPicker {
    pub state: AssetBrowseState,
    query: Entity<InputState>,
    _subscription: Subscription,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct AssetBrowseState {
    pub page: usize,
    pub kind: AssetFilter,
    pub query: String,
}
impl AssetBrowseState {
    fn update_query(&mut self, value: &str) {
        if self.query != value {
            self.query = value.to_string();
            self.page = 0;
        }
    }
}
impl AssetPicker {
    pub(super) fn browse_state(&self, cx: &App) -> AssetBrowseState {
        let mut state = self.state.clone();
        state.update_query(self.query.read(cx).value().as_ref());
        state
    }
}

fn asset_kind(asset: &ProjectAsset) -> &'static str {
    if asset.media_type == MASK_MEDIA_TYPE {
        "mask"
    } else if asset.media_type.starts_with("image/") {
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
        "mask" => "Mask · 灰度遮罩".into(),
        _ => "GLB 模型".into(),
    }
}
impl CreativeCanvas {
    pub(super) fn open_asset_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_asset_picker_at(AssetBrowseState::default(), window, cx);
    }
    pub(super) fn open_asset_picker_at(
        &mut self,
        state: AssetBrowseState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let query = cx.new(|cx| {
            let mut input = InputState::new(window, cx)
                .placeholder("搜索素材名称…")
                .context_menu(false);
            input.set_value(state.query.clone(), window, cx);
            input
        });
        let subscription = cx.subscribe(&query, |this, input, event, cx| {
            if matches!(event, InputEvent::Change)
                && let Some(picker) = &mut this.asset_picker
                && picker.query.entity_id() == input.entity_id()
            {
                // Initializing a restored input emits Change too. Only a new
                // value should reset the remembered page.
                picker.state.update_query(input.read(cx).value().as_ref());
                cx.notify();
            }
        });
        query.read(cx).focus_handle(cx).focus(window, cx);
        self.asset_picker = Some(AssetPicker {
            state,
            query,
            _subscription: subscription,
        });
        self.show_layers = false;
        cx.notify();
    }

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
        let kind = asset_kind(&asset);
        if kind == "mask" {
            let existing = self
                .canvas
                .read(cx)
                .editor
                .graph()
                .nodes
                .values()
                .find(|node| node.kind == "mask" && node.data["asset"].as_str() == Some(key))
                .cloned();
            if let Some(node) = existing {
                self.asset_picker = None;
                self.command(
                    CanvasCommand::SelectNode {
                        node: node.id.clone(),
                        mode: SelectionMode::Replace,
                    },
                    cx,
                );
                self.focus_mask_object(&node, cx);
            } else {
                self.notice = Some("该Mask输出已无编辑节点，请从原图建立新遮罩".into());
                cx.notify();
            }
            return;
        }
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
            id,
            at,
            Some(serde_json::json!({"asset":key,"caption":""})),
        ) {
            Ok(mut node) => {
                node.title = asset.name;
                node.size = size;
                if self.add_object_in_free_space(node, cx) {
                    self.asset_picker = None;
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
        let query = picker.query.clone();
        let query_text = query.read(cx).value().to_string();
        let kind = picker.state.kind;
        let total = self
            .project
            .as_ref()
            .map_or(0, |project| project.assets().len());
        let assets = self
            .project
            .as_ref()
            .map(|project| {
                project
                    .assets()
                    .iter()
                    .rev()
                    .filter(|asset| kind.matches(asset, &query_text))
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let page = picker
            .state
            .page
            .min(assets.len().saturating_sub(1) / PAGE_SIZE);
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
                "mask" => "paintbrush",
                _ => "image",
            };
            let card = self
                .button(
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
                );
            grid = grid.child(
                div()
                    .w(px(140.))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(card)
                    .child(
                        self.button(
                            SharedString::from(format!("export-stored-{}", asset.key)),
                            "导出…",
                            Action::ExportAsset(asset_export_ui::ExportAction::OpenStored(
                                asset.key.clone(),
                            )),
                            self.importing || self.saving,
                            cx,
                        )
                        .w_full()
                        .h(px(26.))
                        .text_size(px(11.))
                        .accessibility_label(SharedString::from(format!(
                            "导出项目素材 {}",
                            asset.name
                        ))),
                    ),
            );
        }
        if assets.is_empty() {
            grid = grid.child(
                div()
                    .p_4()
                    .text_size(px(12.))
                    .text_color(rgb(0x929aa8))
                    .child(if total == 0 {
                        "导入的素材会保留在这里，随时放回画布。"
                    } else {
                        "没有匹配的素材，试试其他名称或类型。"
                    }),
            );
        }
        let mut filters = div().flex().gap_1();
        for (index, filter) in [
            AssetFilter::All,
            AssetFilter::Image,
            AssetFilter::Video,
            AssetFilter::Audio,
            AssetFilter::Model,
            AssetFilter::Mask,
        ]
        .into_iter()
        .enumerate()
        {
            filters = filters.child(
                self.button(
                    SharedString::from(format!("asset-filter-{index}")),
                    filter.label(),
                    Action::FilterAssets(filter),
                    false,
                    cx,
                )
                .flex_1()
                .min_w(px(0.))
                .px_1()
                .text_size(px(11.))
                .bg(rgb(if filter == kind { 0xe7edfb } else { 0xf4f5f7 }))
                .text_color(rgb(if filter == kind { 0x355fc7 } else { 0x616775 })),
            );
        }
        let mut panel = div()
            .id("project-asset-picker")
            .track_focus(&self.asset_picker_focus)
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
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
                    .child(div().text_size(px(13.)).child("本项目素材"))
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
                Input::new(&query)
                    .w_full()
                    .h(px(32.))
                    .aria_label("搜索本项目素材名称"),
            )
            .child(filters)
            .child(
                div()
                    .id("project-asset-grid")
                    .max_h(px((self.canvas_size.height - 318.).max(96.)))
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
                                "{} / {} 个素材 · {} / {}",
                                assets.len(),
                                total,
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
        panel = panel.child(
            self.button(
                "asset-import",
                "＋ 导入本地素材",
                Action::ImportImage,
                !editable || self.saving,
                cx,
            )
            .w_full()
            .bg(rgb(0xf0f3f8)),
        );
        panel.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;

    #[test]
    fn restored_search_initial_change_keeps_page_and_real_edit_resets_it() {
        let mut state = AssetBrowseState {
            page: 3,
            kind: AssetFilter::Image,
            query: " original ".into(),
        };
        let snapshot = state.clone();
        state.update_query(" original ");
        assert_eq!(state, snapshot);
        state.update_query("mask");
        assert_eq!(state.page, 0);
        assert_eq!(state.kind, AssetFilter::Image);
        assert_eq!(snapshot.query, " original ");
        assert_eq!(snapshot.page, 3);
    }
}
