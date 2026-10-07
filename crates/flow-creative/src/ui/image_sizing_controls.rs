//! Geometry-only image sizing, distinct from editing or resampling source pixels.
use super::*;
use crate::image_sizing;
use gpui_kit::component::input::{Input, InputState};

#[derive(Clone)]
pub(super) enum ImageSizingAction {
    Toggle,
    Apply,
    ToggleLock,
    Close,
}

pub(super) struct ImageSizingFields {
    node: NodeId,
    asset: Option<String>,
    size: WorldSize,
    initial_width: String,
    initial_height: String,
    width: Entity<InputState>,
    height: Entity<InputState>,
    error: Option<String>,
    _observers: Vec<Subscription>,
}

fn number(value: f32) -> String {
    format!("{value:.3}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
}

fn requested_size(
    size: WorldSize,
    initial: [&str; 2],
    values: [&str; 2],
    locked: bool,
) -> Result<WorldSize, String> {
    let changed = [
        initial[0] != values[0].trim(),
        initial[1] != values[1].trim(),
    ];
    let read = |index: usize| -> Result<f32, String> {
        let value = values[index]
            .trim()
            .parse::<f32>()
            .map_err(|_| "请输入有效的显示宽高".to_owned())?;
        if !(image_sizing::MIN_IMAGE_DISPLAY_SIZE..=image_sizing::MAX_IMAGE_DISPLAY_SIZE)
            .contains(&value)
        {
            return Err("显示尺寸须在 0.01 到 16384 之间".into());
        }
        Ok(value)
    };
    match (changed, locked) {
        ([false, false], _) => Ok(size),
        ([true, false], _) => image_sizing::size_from_width(size, read(0)?, locked),
        ([false, true], _) => image_sizing::size_from_height(size, read(1)?, locked),
        ([true, true], false) => image_sizing::size_from_height(
            image_sizing::size_from_width(size, read(0)?, false)?,
            read(1)?,
            false,
        ),
        ([true, true], true) => {
            let result = image_sizing::size_from_width(size, read(0)?, true)?;
            if (result.height - read(1)?).abs() > (result.height * 0.0001).max(0.001) {
                Err("比例已锁定：请只改 W 或 H，或先关闭比例锁".into())
            } else {
                Ok(result)
            }
        }
    }
}

impl CreativeCanvas {
    pub(super) fn is_image_sizing(&self, id: &NodeId) -> bool {
        self.image_sizing
            .as_ref()
            .is_some_and(|fields| fields.node == *id)
    }
    pub(super) fn close_image_sizing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(fields) = self.image_sizing.take()
            && (fields.width.read(cx).focus_handle(cx).is_focused(window)
                || fields.height.read(cx).focus_handle(cx).is_focused(window))
        {
            self.canvas.read(cx).focus_handle(cx).focus(window, cx);
        }
    }
    pub(super) fn reconcile_image_sizing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(fields) = &self.image_sizing else {
            return;
        };
        let current = self.selected(cx).is_some_and(|node| {
            node.kind == "image"
                && node.id == fields.node
                && node.size == fields.size
                && node.data["asset"].as_str() == fields.asset.as_deref()
        });
        if !current {
            self.close_image_sizing(window, cx);
        }
    }
    pub(super) fn handle_image_sizing(
        &mut self,
        action: ImageSizingAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(action, ImageSizingAction::Close)
            || (matches!(action, ImageSizingAction::Toggle) && self.image_sizing.is_some())
        {
            self.close_image_sizing(window, cx);
            self.canvas.read(cx).focus_handle(cx).focus(window, cx);
            cx.notify();
            return;
        }
        let Some(node) = self.selected(cx).filter(|node| node.kind == "image") else {
            return;
        };
        if self.importing
            || self.saving
            || !self.canvas.read(cx).is_editable()
            || crate::layer_guard::node_is_locked(self.canvas.read(cx).editor.graph(), &node.id)
        {
            return;
        }
        let locked = match image_sizing::aspect_locked(&node) {
            Ok(value) => value,
            Err(error) => {
                self.notice = Some(error);
                cx.notify();
                return;
            }
        };
        if matches!(action, ImageSizingAction::Toggle) {
            self.clear_image_edit();
            self.clear_image_rotation();
            self.clear_image_adjustment(cx);
            self.clear_mask_edit();
            self.clear_asset_export();
            self.quick_edit = false;
            self.asset_picker = None;
            self.show_layers = false;
            let make = |value: String, window: &mut Window, cx: &mut Context<Self>| {
                cx.new(|cx| {
                    let mut input = InputState::new(window, cx).context_menu(false);
                    input.set_value(value, window, cx);
                    input
                })
            };
            let initial_width = number(node.size.width);
            let initial_height = number(node.size.height);
            let width = make(initial_width.clone(), window, cx);
            let height = make(initial_height.clone(), window, cx);
            let observers = vec![
                cx.observe(&width, |_, _, cx| cx.notify()),
                cx.observe(&height, |_, _, cx| cx.notify()),
            ];
            self.image_sizing = Some(ImageSizingFields {
                node: node.id,
                asset: node.data["asset"].as_str().map(str::to_owned),
                size: node.size,
                initial_width,
                initial_height,
                width,
                height,
                error: None,
                _observers: observers,
            });
            cx.notify();
            return;
        }
        if matches!(action, ImageSizingAction::ToggleLock) {
            match image_sizing::with_aspect_lock(&node, !locked) {
                Ok(data) => {
                    self.command(
                        CanvasCommand::SetNodeData {
                            node: node.id,
                            data,
                        },
                        cx,
                    );
                }
                Err(error) => self.notice = Some(error),
            }
            if let Some(fields) = &mut self.image_sizing {
                fields.error = None;
            }
            cx.notify();
            return;
        }
        self.reconcile_image_sizing(window, cx);
        let Some(fields) = &self.image_sizing else {
            return;
        };
        let width = fields.width.read(cx).value();
        let height = fields.height.read(cx).value();
        match requested_size(
            fields.size,
            [&fields.initial_width, &fields.initial_height],
            [&width, &height],
            locked,
        ) {
            Ok(size) => {
                self.close_image_sizing(window, cx);
                if size != node.size {
                    self.command(
                        CanvasCommand::ResizeNode {
                            node: node.id,
                            size,
                        },
                        cx,
                    );
                }
                self.canvas.read(cx).focus_handle(cx).focus(window, cx);
            }
            Err(error) => {
                if let Some(fields) = &mut self.image_sizing {
                    fields.error = Some(error);
                }
            }
        }
        cx.notify();
    }

    pub(super) fn image_sizing_panel(
        &self,
        node: &Node,
        editable: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let fields = self.image_sizing.as_ref().expect("active image sizing");
        let locked = image_sizing::aspect_locked(node).unwrap_or(true);
        let width = fields.width.read(cx).value();
        let height = fields.height.read(cx).value();
        let preview = requested_size(
            fields.size,
            [&fields.initial_width, &fields.initial_height],
            [&width, &height],
            locked,
        )
        .map(|size| format!("应用后 {} × {}", number(size.width), number(size.height)))
        .unwrap_or_else(|error| error);
        let original = self
            .project
            .as_ref()
            .and_then(|project| {
                project
                    .assets()
                    .iter()
                    .find(|asset| Some(asset.key.as_str()) == node.data["asset"].as_str())
            })
            .filter(|asset| asset.width > 0 && asset.height > 0)
            .map(|asset| format!("原图 {} × {} px · 文件不变", asset.width, asset.height));
        let can_restore = original.is_some();
        let disabled = !editable || self.importing || self.saving;
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().text_sm().child("画布显示尺寸"))
                    .child(self.icon_button(
                        "close-image-size",
                        "x",
                        "取消尺寸草稿",
                        Action::ImageSizing(ImageSizingAction::Close),
                        false,
                        cx,
                    )),
            )
            .child(label(
                original.unwrap_or_else(|| "原图尺寸不可用 · 文件不变".into()),
            ))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(label("W"))
                    .child(
                        Input::new(&fields.width)
                            .w(px(108.))
                            .h(px(30.))
                            .disabled(disabled)
                            .aria_label("图片显示宽度"),
                    )
                    .child(label("H"))
                    .child(
                        Input::new(&fields.height)
                            .w(px(108.))
                            .h(px(30.))
                            .disabled(disabled)
                            .aria_label("图片显示高度"),
                    ),
            )
            .child(
                self.button(
                    "image-size-lock",
                    if locked {
                        "已锁定当前比例"
                    } else {
                        "自由调整宽高"
                    },
                    Action::ImageSizing(ImageSizingAction::ToggleLock),
                    disabled,
                    cx,
                )
                .when(locked, |button| button.bg(rgb(0xeff4ff))),
            )
            .child(label(if locked {
                "改 W 或 H 联动；Shift 拖角点临时自由缩放"
            } else {
                "自由宽高可能裁切显示；Shift 拖角点等比"
            }))
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(if fields.error.is_some() {
                        0xb34432
                    } else {
                        0x667085
                    }))
                    .child(fields.error.clone().unwrap_or(preview)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(self.button(
                        "image-size-original",
                        "恢复原图比例",
                        Action::OriginalRatio,
                        disabled || !can_restore,
                        cx,
                    ))
                    .child(self.button(
                        "image-size-apply",
                        "应用",
                        Action::ImageSizing(ImageSizingAction::Apply),
                        disabled,
                        cx,
                    )),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;
    #[test]
    fn unchanged_rounded_fields_do_not_drift_and_either_axis_can_drive_the_ratio() {
        let size = WorldSize::new(420., 269.77646);
        let a = number(size.width);
        let b = number(size.height);
        assert_eq!(
            requested_size(size, [&a, &b], [&a, &b], true).unwrap(),
            size
        );
        let wide = requested_size(size, [&a, &b], ["630", &b], true).unwrap();
        assert_eq!(wide.width, 630.);
        assert!((wide.height - size.height * 1.5).abs() < 0.0001);
        let tall = requested_size(size, [&a, &b], [&a, "100"], true).unwrap();
        assert_eq!(tall.height, 100.);
        assert!((tall.width - size.width * 100. / size.height).abs() < 0.0001);
    }
    #[test]
    fn conflicting_locked_axes_and_invalid_geometry_never_create_a_size_command() {
        let size = WorldSize::new(400., 200.);
        assert!(requested_size(size, ["400", "200"], ["600", "100"], true).is_err());
        assert_eq!(
            requested_size(size, ["400", "200"], ["600", "100"], false).unwrap(),
            WorldSize::new(600., 100.)
        );
        assert!(requested_size(size, ["400", "200"], ["600", "NaN"], true).is_err());
        for value in ["", "NaN", "inf", "0", "-1", "999999"] {
            assert!(requested_size(size, ["400", "200"], [value, "200"], true).is_err());
        }
    }
}
