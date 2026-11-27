use super::image_adjust_preview::AdjustmentPreview;
use super::*;
use crate::image_adjust::ImageAdjustSpec;
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};

#[derive(Clone, Copy)]
pub(super) enum AdjustAction {
    Begin,
    Cancel,
    CancelApply,
    Reset,
    Apply,
}
pub(super) struct AdjustmentPresentation {
    pub node: NodeId,
    pub data: Value,
    pub path: PathBuf,
    pub preview: Entity<AdjustmentPreview>,
}
pub(super) struct AdjustmentEditing {
    pub project_id: String,
    pub node: NodeId,
    pub asset: ProjectAsset,
    pub path: PathBuf,
    pub original_data: Value,
    pub spec: ImageAdjustSpec,
    pub preview: Entity<AdjustmentPreview>,
    sliders: [Entity<SliderState>; 5],
    _subscriptions: Vec<Subscription>,
}
impl CreativeCanvas {
    pub(super) fn clear_image_adjustment(&mut self, cx: &mut Context<Self>) {
        if self.image_adjustment.is_some()
            && let Some(cancel) = &self.image_edit_cancel
        {
            cancel.store(true, std::sync::atomic::Ordering::Release);
        }
        *self.adjustment_presentation.lock().unwrap() = None;
        self.image_adjustment = None;
        self.canvas.update(cx, |_, cx| cx.notify());
    }
    pub(super) fn is_image_adjusting(&self, id: &NodeId) -> bool {
        self.image_adjustment
            .as_ref()
            .is_some_and(|e| &e.node == id)
    }
    pub(super) fn reconcile_image_adjustment(&mut self, cx: &mut Context<Self>) {
        if self.image_adjustment.as_ref().is_some_and(|edit| {
            let view = self.canvas.read(cx);
            self.project
                .as_ref()
                .is_none_or(|p| p.metadata().id != edit.project_id)
                || view.editor.selection.nodes.len() != 1
                || !view.editor.selection.nodes.contains(&edit.node)
                || view.editor.graph().node(&edit.node).is_none_or(|n| {
                    n.data != edit.original_data
                        || crate::layer_guard::node_is_locked(view.editor.graph(), &n.id)
                })
        }) {
            self.clear_image_adjustment(cx);
        }
    }
    pub(super) fn update_adjustment_preview(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = &mut self.image_adjustment else {
            return;
        };
        let values = edit.sliders.each_ref().map(|s| s.read(cx).value().end());
        let spec = ImageAdjustSpec {
            light: values[0],
            exposure: values[1],
            saturation: values[2],
            vibrance: values[3],
            vignette: values[4],
        };
        match spec.validate() {
            Ok(()) => {
                edit.spec = spec;
                edit.preview.update(cx, |v, cx| v.request(spec, cx));
            }
            Err(error) => self.notice = Some(error.to_string()),
        }
        cx.notify();
    }
    pub(super) fn handle_image_adjustment(
        &mut self,
        action: AdjustAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(action, AdjustAction::CancelApply) {
            if let Some(cancel) = &self.image_edit_cancel {
                cancel.store(true, std::sync::atomic::Ordering::Release);
            }
            return;
        }
        if self.importing || self.saving {
            return;
        }
        if matches!(action, AdjustAction::Begin) && self.image_adjustment.is_some() {
            self.clear_image_adjustment(cx);
            self.canvas.read(cx).focus_handle(cx).focus(window, cx);
            cx.notify();
            return;
        }
        match action {
            AdjustAction::Cancel => {
                self.clear_image_adjustment(cx);
                self.canvas.read(cx).focus_handle(cx).focus(window, cx);
            }
            AdjustAction::Begin => {
                if !self.canvas.read(cx).is_editable() {
                    return;
                }
                let Some(node) = self.selected(cx).filter(|n| {
                    n.kind == "image"
                        && !crate::layer_guard::node_is_locked(
                            self.canvas.read(cx).editor.graph(),
                            &n.id,
                        )
                }) else {
                    return;
                };
                let result = (|| {
                    let project = self
                        .project
                        .as_ref()
                        .ok_or(tr("请先打开本地项目", "Open a local project first"))?;
                    let asset = project
                        .assets()
                        .iter()
                        .find(|a| Some(a.key.as_str()) == node.data["asset"].as_str())
                        .filter(|a| matches!(a.media_type.as_str(), "image/png" | "image/jpeg"))
                        .cloned()
                        .ok_or(tr(
                            "调整支持本地静态PNG/JPEG",
                            "Adjustment supports local still PNG and JPEG images",
                        ))?;
                    ImageAdjustSpec::default()
                        .output_dimensions(asset.width, asset.height)
                        .map_err(|e| e.to_string())?;
                    let path = project
                        .resolve_asset_path(&asset.key)
                        .map_err(|e| e.to_string())?;
                    Ok::<_, String>((project.metadata().id.clone(), asset, path))
                })();
                match result {
                    Ok((project_id, asset, path)) => {
                        self.clear_image_edit();
                        self.clear_image_rotation();
                        self.clear_mask_edit();
                        self.clear_text_edit();
                        self.clear_asset_export();
                        let preview =
                            cx.new(|cx| AdjustmentPreview::new(path.clone(), asset.clone(), cx));
                        let mut subscriptions = vec![cx.observe(&preview, |_, _, cx| cx.notify())];
                        let sliders = [
                            (-1., 1., 0.01, 0.),
                            (-4., 4., 0.1, 0.),
                            (0., 2., 0.01, 1.),
                            (-1., 1., 0.01, 0.),
                            (0., 1., 0.01, 0.),
                        ]
                        .map(|(min, max, step, value)| {
                            cx.new(|_| {
                                SliderState::new()
                                    .min(min)
                                    .max(max)
                                    .step(step)
                                    .default_value(value)
                            })
                        });
                        for slider in &sliders {
                            subscriptions.push(cx.subscribe(slider, |this, _, event, cx| {
                                if matches!(event, SliderEvent::Change(_)) {
                                    this.update_adjustment_preview(cx);
                                }
                            }));
                        }
                        *self.adjustment_presentation.lock().unwrap() =
                            Some(AdjustmentPresentation {
                                node: node.id.clone(),
                                data: node.data.clone(),
                                path: path.clone(),
                                preview: preview.clone(),
                            });
                        self.image_adjustment = Some(AdjustmentEditing {
                            project_id,
                            node: node.id,
                            asset,
                            path,
                            original_data: node.data,
                            spec: Default::default(),
                            preview,
                            sliders,
                            _subscriptions: subscriptions,
                        });
                        self.canvas.update(cx, |_, cx| cx.notify());
                        self.canvas.read(cx).focus_handle(cx).focus(window, cx);
                    }
                    Err(error) => self.notice = Some(error),
                }
            }
            AdjustAction::Reset => {
                if let Some(edit) = &self.image_adjustment {
                    for (slider, value) in edit.sliders.iter().zip([0., 0., 1., 0., 0.]) {
                        slider.update(cx, |v, cx| v.set_value(value, window, cx));
                    }
                }
                self.update_adjustment_preview(cx);
            }
            AdjustAction::Apply => {
                self.update_adjustment_preview(cx);
                let Some(edit) = &self.image_adjustment else {
                    return;
                };
                if !edit.preview.read(cx).ready_for(edit.spec) {
                    return;
                }
                self.canvas.read(cx).focus_handle(cx).focus(window, cx);
                if edit.spec.is_identity() {
                    self.clear_image_adjustment(cx);
                } else {
                    self.apply_image_adjustment(cx);
                }
            }
            AdjustAction::CancelApply => {}
        }
        cx.notify();
    }
    pub(super) fn image_adjustment_panel(&self, cx: &mut Context<Self>) -> Div {
        let Some(edit) = &self.image_adjustment else {
            return div();
        };
        let busy = self.image_edit_cancel.is_some();
        let mut panel = div().flex().flex_col().gap_1().child(label(tr(
            "图片调整 · 画布内预览",
            "Image Adjustments · Canvas Preview",
        )));
        for (index, (name, slider)) in [
            tr("明暗", "Brightness"),
            tr("曝光", "Exposure"),
            tr("饱和度", "Saturation"),
            tr("自然饱和度", "Vibrance"),
            tr("暗角", "Vignette"),
        ]
        .iter()
        .zip(&edit.sliders)
        .enumerate()
        {
            let value = slider.read(cx).value().end();
            let text = if index == 1 {
                format!("{value:+.1} EV")
            } else if index == 0 || index == 3 {
                format!("{:+.0}%", value * 100.)
            } else {
                format!("{:.0}%", value * 100.)
            };
            panel = panel.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(28.))
                    .child(
                        div()
                            .w(px(62.))
                            .text_size(px(11.))
                            .text_color(rgb(0x68707a))
                            .child(*name),
                    )
                    .child(
                        div()
                            .w(px(144.))
                            .h(px(24.))
                            .child(Slider::new(slider).disabled(busy).bg(rgb(0x536b8d))),
                    )
                    .child(
                        div()
                            .w(px(54.))
                            .text_size(px(10.))
                            .text_color(rgb(0x68707a))
                            .text_right()
                            .child(text),
                    ),
            );
        }
        panel
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(self.button(
                        "adjust-reset",
                        tr("重置", "Reset"),
                        Action::ImageAdjust(AdjustAction::Reset),
                        busy,
                        cx,
                    ))
                    .child(self.button(
                        "adjust-cancel",
                        if busy {
                            tr("取消处理", "Cancel processing")
                        } else {
                            tr("取消", "Cancel")
                        },
                        Action::ImageAdjust(if busy {
                            AdjustAction::CancelApply
                        } else {
                            AdjustAction::Cancel
                        }),
                        false,
                        cx,
                    ))
                    .child(self.button(
                        "adjust-apply",
                        tr("应用调整", "Apply Adjustments"),
                        Action::ImageAdjust(AdjustAction::Apply),
                        busy || !edit.preview.read(cx).ready_for(edit.spec),
                        cx,
                    )),
            )
            .child(label(tr(
                "原图保留 · Esc取消 · 确认后可撤销",
                "Original retained · Esc cancels · Apply can be undone",
            )))
    }
}
