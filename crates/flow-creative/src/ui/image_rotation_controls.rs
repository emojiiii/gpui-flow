use super::image_rotation_preview::RotationPreview;
use super::*;
use crate::image_rotation::ImageRotationSpec;
use gpui_kit::component::input::{Input, InputState};

#[derive(Clone, Copy)]
pub(super) enum RotationAction {
    Begin,
    Cancel,
    CancelApply,
    Preset(f64),
    Apply,
}
pub(super) struct RotationEditing {
    pub node: NodeId,
    pub asset: ProjectAsset,
    pub path: PathBuf,
    pub original_data: Value,
    pub spec: ImageRotationSpec,
    pub error: Option<String>,
    pub preview: Entity<RotationPreview>,
    angle: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}
impl CreativeCanvas {
    pub(super) fn clear_image_rotation(&mut self) {
        if self.image_rotation.is_some()
            && let Some(cancel) = &self.image_edit_cancel
        {
            cancel.store(true, std::sync::atomic::Ordering::Release);
        }
        self.image_rotation = None;
    }
    pub(super) fn is_image_rotating(&self, id: &NodeId) -> bool {
        self.image_rotation.as_ref().is_some_and(|e| &e.node == id)
    }
    pub(super) fn reconcile_image_rotation(&mut self, cx: &App) {
        if self.image_rotation.as_ref().is_some_and(|edit| {
            let view = self.canvas.read(cx);
            view.editor.selection.nodes.len() != 1
                || !view.editor.selection.nodes.contains(&edit.node)
                || view.editor.graph().node(&edit.node).is_none_or(|n| {
                    n.data != edit.original_data
                        || crate::layer_guard::node_is_locked(view.editor.graph(), &n.id)
                })
        }) {
            self.clear_image_rotation();
        }
    }
    pub(super) fn update_rotation_preview(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = &mut self.image_rotation else {
            return;
        };
        let result = (|| {
            let degrees = edit
                .angle
                .read(cx)
                .value()
                .trim()
                .parse::<f64>()
                .map_err(|_| "请输入-360到360之间的角度".to_string())?;
            let spec = ImageRotationSpec { degrees };
            spec.output_dimensions(edit.asset.width, edit.asset.height)
                .map_err(|e| e.to_string())?;
            Ok::<_, String>(spec)
        })();
        match result {
            Ok(spec) => {
                edit.spec = spec;
                edit.error = None;
                edit.preview.update(cx, |v, cx| v.request(spec, cx));
            }
            Err(error) => edit.error = Some(error),
        }
        cx.notify();
    }
    pub(super) fn handle_image_rotation(
        &mut self,
        action: RotationAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(action, RotationAction::CancelApply) {
            if let Some(cancel) = &self.image_edit_cancel {
                cancel.store(true, std::sync::atomic::Ordering::Release);
            }
            return;
        }
        if self.importing || self.saving {
            return;
        }
        match action {
            RotationAction::Cancel => {
                self.clear_image_rotation();
                self.canvas.read(cx).focus_handle(cx).focus(window, cx);
            }
            RotationAction::Begin => {
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
                    let project = self.project.as_ref().ok_or("请先打开本地项目")?;
                    let asset = project
                        .assets()
                        .iter()
                        .find(|a| Some(a.key.as_str()) == node.data["asset"].as_str())
                        .filter(|a| matches!(a.media_type.as_str(), "image/png" | "image/jpeg"))
                        .cloned()
                        .ok_or("旋转支持本地静态PNG/JPEG")?;
                    ImageRotationSpec::default()
                        .output_dimensions(asset.width, asset.height)
                        .map_err(|e| e.to_string())?;
                    let path = project
                        .resolve_asset_path(&asset.key)
                        .map_err(|e| e.to_string())?;
                    Ok::<_, String>((asset, path))
                })();
                match result {
                    Ok((asset, path)) => {
                        self.clear_image_edit();
                        self.clear_mask_edit();
                        self.clear_text_edit();
                        self.clear_asset_export();
                        let preview =
                            cx.new(|cx| RotationPreview::new(path.clone(), asset.clone(), cx));
                        let observer = cx.observe(&preview, |_, _, cx| cx.notify());
                        let angle = cx.new(|cx| {
                            let mut field = InputState::new(window, cx).context_menu(false);
                            field.set_value("0", window, cx);
                            field
                        });
                        let subscription = cx.subscribe(&angle, |this, _, event, cx| {
                            if matches!(event, InputEvent::Change) {
                                this.update_rotation_preview(cx);
                            }
                        });
                        self.image_rotation = Some(RotationEditing {
                            node: node.id,
                            asset,
                            path,
                            original_data: node.data,
                            spec: Default::default(),
                            error: None,
                            preview,
                            angle: angle.clone(),
                            _subscriptions: vec![observer, subscription],
                        });
                        angle.read(cx).focus_handle(cx).focus(window, cx);
                    }
                    Err(error) => self.notice = Some(error),
                }
            }
            RotationAction::Preset(degrees) => {
                if let Some(edit) = &self.image_rotation {
                    edit.angle
                        .update(cx, |v, cx| v.set_value(degrees.to_string(), window, cx));
                }
                self.update_rotation_preview(cx);
            }
            RotationAction::Apply => {
                self.update_rotation_preview(cx);
                let Some(edit) = &self.image_rotation else {
                    return;
                };
                if edit.error.is_some() || !edit.preview.read(cx).ready_for(edit.spec) {
                    return;
                }
                if edit.spec.normalized_degrees().ok() == Some(0.) {
                    self.clear_image_rotation();
                    self.canvas.read(cx).focus_handle(cx).focus(window, cx);
                } else {
                    self.canvas.read(cx).focus_handle(cx).focus(window, cx);
                    self.apply_image_rotation(cx);
                }
            }
            RotationAction::CancelApply => {}
        }
        cx.notify();
    }
    pub(super) fn image_rotation_panel(&self, cx: &mut Context<Self>) -> Div {
        let Some(edit) = &self.image_rotation else {
            return div();
        };
        let busy = self.image_edit_cancel.is_some();
        let dimensions = edit
            .spec
            .output_dimensions(edit.asset.width, edit.asset.height)
            .ok();
        let mut presets = div().flex().items_center().gap_1();
        for angle in [-90., -15., 0., 15., 90.] {
            presets = presets.child(self.button(
                SharedString::from(format!("rotate-preset-{angle}")),
                format!("{angle}°"),
                Action::ImageRotation(RotationAction::Preset(angle)),
                busy,
                cx,
            ));
        }
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(label("自定义旋转预览"))
                    .child(label(
                        dimensions
                            .map(|(w, h)| format!("{w} × {h}"))
                            .unwrap_or_default(),
                    )),
            )
            .child(
                div()
                    .w_full()
                    .h(px(super::image_rotation_preview::PREVIEW_HEIGHT))
                    .child(edit.preview.clone()),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(label("角度"))
                    .child(
                        Input::new(&edit.angle)
                            .w(px(85.))
                            .h(px(28.))
                            .disabled(busy)
                            .aria_label("顺时针旋转角度"),
                    )
                    .child(label("° · 正数顺时针")),
            )
            .child(presets)
            .when_some(edit.error.clone(), |v, error| {
                v.child(div().text_xs().text_color(rgb(0xa25640)).child(error))
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(label("透明留边 · 原图保留"))
                    .child(self.button(
                        "rotation-cancel",
                        if busy { "取消处理" } else { "取消" },
                        Action::ImageRotation(if busy {
                            RotationAction::CancelApply
                        } else {
                            RotationAction::Cancel
                        }),
                        false,
                        cx,
                    ))
                    .child(self.button(
                        "rotation-apply",
                        "应用旋转",
                        Action::ImageRotation(RotationAction::Apply),
                        busy || edit.error.is_some() || !edit.preview.read(cx).ready_for(edit.spec),
                        cx,
                    )),
            )
    }
}
