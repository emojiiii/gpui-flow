use super::image_edit_preview::ImageEditPreview;
use super::*;
use crate::image_edit::ImageEditSpec;
use gpui_flow_core::Rect;
use gpui_flow_editor::{ImageCropGeometry, ImageCropPart, ResizeHandle};
use gpui_flow_scene::{Bounds as SceneBounds, Point as ScenePoint, Size as SceneSize};
use gpui_kit::component::input::{Input, InputState};

#[derive(Clone, Copy)]
pub(super) enum ImageEditAction {
    Begin,
    Cancel,
    Apply,
    CancelApply,
    Rotate(bool),
    Flip(bool),
    Reset,
    ToggleRatios,
    Ratio(Option<f32>),
    Dimensions,
    Undo(bool),
}
#[derive(Clone, Copy)]
enum DragPart {
    Existing(ImageCropPart),
    New,
}
struct CropDrag {
    spec: ImageEditSpec,
    part: DragPart,
    start: ScenePoint,
    geometry: ImageCropGeometry,
    content: Rect,
}
pub(super) struct ImageEditing {
    pub node: NodeId,
    pub asset: ProjectAsset,
    pub path: PathBuf,
    pub original_data: Value,
    pub spec: ImageEditSpec,
    preview: Entity<ImageEditPreview>,
    width: Entity<InputState>,
    height: Entity<InputState>,
    ratio: Option<f32>,
    ratios_open: bool,
    drag: Option<CropDrag>,
    undo: Vec<ImageEditSpec>,
    redo: Vec<ImageEditSpec>,
    _observer: Subscription,
}
const CORNERS: [ResizeHandle; 4] = [
    ResizeHandle::TopLeft,
    ResizeHandle::TopRight,
    ResizeHandle::BottomRight,
    ResizeHandle::BottomLeft,
];
fn content_rect(bounds: Rect, dimensions: (u32, u32)) -> Rect {
    let scale =
        (bounds.size.width / dimensions.0 as f32).min(bounds.size.height / dimensions.1 as f32);
    let size = WorldSize::new(dimensions.0 as f32 * scale, dimensions.1 as f32 * scale);
    Rect::new(
        WorldPoint::new(
            bounds.center().x - size.width / 2.,
            bounds.center().y - size.height / 2.,
        ),
        size,
    )
}
fn crop_geometry(spec: &ImageEditSpec, content: Rect) -> ImageCropGeometry {
    ImageCropGeometry {
        crop: SceneBounds::new(
            ScenePoint::new(spec.crop.origin.x, spec.crop.origin.y),
            SceneSize::new(spec.crop.size.width, spec.crop.size.height),
        ),
        preview_size: SceneSize::new(content.size.width, content.size.height),
    }
}
fn core_crop(crop: SceneBounds) -> Rect {
    Rect::new(
        WorldPoint::new(crop.origin.x, crop.origin.y),
        WorldSize::new(crop.size.width, crop.size.height),
    )
}
impl ImageEditing {
    fn record(&mut self, before: ImageEditSpec) {
        if before != self.spec {
            self.undo.push(before);
            if self.undo.len() > 32 {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
    }
    fn sync_fields(&mut self, window: &mut Window, cx: &mut Context<CreativeCanvas>) {
        if let Ok((_, _, w, h)) = self.spec.crop_pixels(self.asset.width, self.asset.height) {
            self.width
                .update(cx, |v, cx| v.set_value(w.to_string(), window, cx));
            self.height
                .update(cx, |v, cx| v.set_value(h.to_string(), window, cx));
        }
    }
}
impl CreativeCanvas {
    pub(super) fn clear_image_edit(&mut self) {
        if let Some(cancel) = &self.image_edit_cancel {
            cancel.store(true, std::sync::atomic::Ordering::Release);
        }
        self.image_editing = None;
        *self.editing_image.lock().unwrap() = None;
    }
    pub(super) fn is_image_editing(&self, node: &NodeId) -> bool {
        self.image_editing.as_ref().is_some_and(|e| &e.node == node)
    }
    pub(super) fn reconcile_image_edit(&mut self, cx: &App) {
        if self.image_editing.as_ref().is_some_and(|edit| {
            let canvas = self.canvas.read(cx);
            canvas.editor.selection.nodes.len() != 1
                || !canvas.editor.selection.nodes.contains(&edit.node)
                || canvas.editor.graph().node(&edit.node).is_none_or(|n| {
                    n.data != edit.original_data
                        || crate::layer_guard::node_is_locked(canvas.editor.graph(), &n.id)
                })
        }) {
            self.clear_image_edit();
        }
    }
    pub(super) fn handle_image_edit(
        &mut self,
        action: ImageEditAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(action, ImageEditAction::CancelApply) {
            if let Some(cancel) = &self.image_edit_cancel {
                cancel.store(true, std::sync::atomic::Ordering::Release);
            }
            return;
        }
        if self.importing || self.saving {
            return;
        }
        if matches!(action, ImageEditAction::Cancel) {
            self.clear_image_edit();
            self.canvas.read(cx).focus_handle(cx).focus(window, cx);
            cx.notify();
            return;
        }
        if matches!(action, ImageEditAction::Begin) {
            self.apply_form_edit(cx);
            let Some(node) = self.selected(cx).filter(|n| n.kind == "image" && !n.locked) else {
                return;
            };
            let result = (|| {
                let project = self
                    .project
                    .as_ref()
                    .ok_or_else(|| "请先打开本地项目".to_string())?;
                let asset = project
                    .assets()
                    .iter()
                    .find(|a| Some(a.key.as_str()) == node.data["asset"].as_str())
                    .filter(|a| matches!(a.media_type.as_str(), "image/png" | "image/jpeg"))
                    .cloned()
                    .ok_or_else(|| "当前图片编辑支持本地静态PNG/JPEG".to_string())?;
                ImageEditSpec::default()
                    .oriented_dimensions(asset.width, asset.height)
                    .map_err(|e| e.to_string())?;
                let path = project
                    .resolve_asset_path(&asset.key)
                    .map_err(|e| e.to_string())?;
                let preview = cx.new(|cx| ImageEditPreview::new(path.clone(), asset.clone(), cx));
                let observer = cx.observe(&preview, |_, _, cx| cx.notify());
                let width = cx.new(|cx| {
                    let mut v = InputState::new(window, cx).context_menu(false);
                    v.set_value(asset.width.to_string(), window, cx);
                    v
                });
                let height = cx.new(|cx| {
                    let mut v = InputState::new(window, cx).context_menu(false);
                    v.set_value(asset.height.to_string(), window, cx);
                    v
                });
                self.clear_mask_edit();
                self.clear_text_edit();
                self.show_colors = false;
                *self.editing_image.lock().unwrap() = Some(node.id.clone());
                self.image_editing = Some(ImageEditing {
                    node: node.id,
                    asset,
                    path,
                    original_data: node.data,
                    spec: ImageEditSpec::default(),
                    preview,
                    width,
                    height,
                    ratio: None,
                    ratios_open: false,
                    drag: None,
                    undo: vec![],
                    redo: vec![],
                    _observer: observer,
                });
                self.image_edit_focus.focus(window, cx);
                Ok::<_, String>(())
            })();
            if let Err(error) = result {
                self.notice = Some(error);
            }
            cx.notify();
            return;
        }
        if matches!(action, ImageEditAction::Apply | ImageEditAction::Dimensions) {
            if let Err(error) = self.apply_image_dimensions(window, cx) {
                self.notice = Some(error);
                cx.notify();
                return;
            }
            if matches!(action, ImageEditAction::Apply) {
                if self
                    .image_editing
                    .as_ref()
                    .is_some_and(|edit| edit.spec == ImageEditSpec::default())
                {
                    self.clear_image_edit();
                    self.canvas.read(cx).focus_handle(cx).focus(window, cx);
                    cx.notify();
                    return;
                }
                self.apply_image_edit(cx);
            }
            return;
        }
        let Some(edit) = &mut self.image_editing else {
            return;
        };
        let before = edit.spec;
        let result = match action {
            ImageEditAction::Rotate(clockwise) => {
                let result = if clockwise {
                    edit.spec.rotate_cw()
                } else {
                    edit.spec.rotate_ccw()
                };
                if result.is_ok() {
                    edit.ratio = edit.ratio.map(|r| 1. / r);
                }
                result
            }
            ImageEditAction::Flip(horizontal) => {
                if horizontal {
                    edit.spec.flip_horizontal()
                } else {
                    edit.spec.flip_vertical()
                }
            }
            ImageEditAction::Reset => {
                edit.spec = ImageEditSpec::default();
                edit.ratio = None;
                Ok(())
            }
            ImageEditAction::ToggleRatios => {
                edit.ratios_open = !edit.ratios_open;
                cx.notify();
                return;
            }
            ImageEditAction::Ratio(ratio) => {
                edit.ratio = ratio;
                edit.ratios_open = false;
                if let Some(ratio) = ratio
                    && let Ok((w, h)) = edit
                        .spec
                        .oriented_dimensions(edit.asset.width, edit.asset.height)
                {
                    let cw = edit.spec.crop.size.width * w as f32;
                    let ch = edit.spec.crop.size.height * h as f32;
                    let size = if cw / ch > ratio {
                        WorldSize::new(ch * ratio / w as f32, ch / h as f32)
                    } else {
                        WorldSize::new(cw / w as f32, cw / ratio / h as f32)
                    };
                    let center = edit.spec.crop.center();
                    edit.spec.crop = Rect::new(
                        WorldPoint::new(center.x - size.width / 2., center.y - size.height / 2.),
                        size,
                    );
                }
                Ok(())
            }
            ImageEditAction::Undo(redo) => {
                let (from, to) = if redo {
                    (&mut edit.redo, &mut edit.undo)
                } else {
                    (&mut edit.undo, &mut edit.redo)
                };
                if let Some(previous) = from.pop() {
                    to.push(edit.spec);
                    edit.spec = previous;
                    edit.ratio = None;
                }
                edit.sync_fields(window, cx);
                let spec = edit.spec;
                edit.preview.update(cx, |v, cx| v.request(spec, cx));
                cx.notify();
                return;
            }
            _ => Ok(()),
        };
        if let Err(error) = result {
            self.notice = Some(error.to_string());
        } else {
            edit.record(before);
            edit.sync_fields(window, cx);
            let spec = edit.spec;
            edit.preview.update(cx, |v, cx| v.request(spec, cx));
        }
        cx.notify();
    }
    fn apply_image_dimensions(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let Some(edit) = &mut self.image_editing else {
            return Err("图片编辑已结束".into());
        };
        let w = edit
            .width
            .read(cx)
            .value()
            .trim()
            .parse::<u32>()
            .map_err(|_| "宽度必须为正整数".to_string())?;
        let h = edit
            .height
            .read(cx)
            .value()
            .trim()
            .parse::<u32>()
            .map_err(|_| "高度必须为正整数".to_string())?;
        let (sw, sh) = edit
            .spec
            .oriented_dimensions(edit.asset.width, edit.asset.height)
            .map_err(|e| e.to_string())?;
        if w == 0 || h == 0 || w > sw || h > sh {
            return Err("裁剪尺寸必须在原图范围内，不会拉伸或放大".into());
        }
        if edit
            .spec
            .crop_pixels(edit.asset.width, edit.asset.height)
            .is_ok_and(|(_, _, cw, ch)| cw == w && ch == h)
        {
            return Ok(());
        }
        let before = edit.spec;
        edit.spec
            .resize_crop_pixels(edit.asset.width, edit.asset.height, w, h)
            .map_err(|e| e.to_string())?;
        edit.ratio = None;
        edit.record(before);
        edit.sync_fields(window, cx);
        cx.notify();
        Ok(())
    }
    fn image_crop_pointer(
        &mut self,
        content: Rect,
        position: Point<Pixels>,
        start: bool,
        shift: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.importing {
            return;
        }
        if self
            .image_editing
            .as_ref()
            .is_none_or(|edit| !edit.preview.read(cx).ready())
        {
            return;
        }
        let position = WorldPoint::new(
            f32::from(position.x) - self.canvas_origin.x,
            f32::from(position.y) - self.canvas_origin.y,
        );
        let Some(edit) = &mut self.image_editing else {
            return;
        };
        if start {
            let geometry = crop_geometry(&edit.spec, content);
            let local =
                ScenePoint::new(position.x - content.origin.x, position.y - content.origin.y);
            let part = CORNERS
                .into_iter()
                .filter_map(|h| {
                    let p = h.position(geometry.frame());
                    let d = (p.x - local.x).hypot(p.y - local.y);
                    (d <= 11.).then_some((h, d))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(h, _)| DragPart::Existing(ImageCropPart::Handle(h)))
                .unwrap_or_else(|| {
                    if geometry.frame().contains(local) {
                        DragPart::Existing(ImageCropPart::Move)
                    } else {
                        DragPart::New
                    }
                });
            edit.drag = Some(CropDrag {
                spec: edit.spec,
                part,
                start: local,
                geometry,
                content,
            });
            return;
        }
        let Some(drag) = &edit.drag else {
            return;
        };
        let local = ScenePoint::new(
            position.x - drag.content.origin.x,
            position.y - drag.content.origin.y,
        );
        let crop = match drag.part {
            DragPart::Existing(part) => drag
                .geometry
                .drag(part, drag.start, local, shift || edit.ratio.is_some())
                .map(core_crop),
            DragPart::New => {
                let x0 = (drag.start.x / drag.content.size.width).clamp(0., 1.);
                let y0 = (drag.start.y / drag.content.size.height).clamp(0., 1.);
                let mut x1 = (local.x / drag.content.size.width).clamp(0., 1.);
                let mut y1 = (local.y / drag.content.size.height).clamp(0., 1.);
                if let Some(ratio) = edit.ratio.or(shift.then_some(1.)) {
                    let dx = (x1 - x0) * drag.content.size.width;
                    let dy = (y1 - y0) * drag.content.size.height;
                    let width = dx.abs().min(dy.abs() * ratio);
                    x1 = x0 + dx.signum() * width / drag.content.size.width;
                    y1 = y0 + dy.signum() * width / ratio / drag.content.size.height;
                }
                Some(Rect::new(
                    WorldPoint::new(x0.min(x1), y0.min(y1)),
                    WorldSize::new((x1 - x0).abs(), (y1 - y0).abs()),
                ))
            }
        };
        if let Some(crop) = crop {
            let mut spec = edit.spec;
            spec.crop = crop;
            if spec
                .crop_pixels(edit.asset.width, edit.asset.height)
                .is_ok()
            {
                edit.spec = spec;
                edit.sync_fields(window, cx);
                cx.notify();
            }
        }
    }
    fn finish_image_crop(&mut self, cx: &mut Context<Self>) {
        if let Some(edit) = &mut self.image_editing
            && let Some(drag) = edit.drag.take()
        {
            edit.record(drag.spec);
        }
        cx.notify();
    }
    pub(super) fn image_edit_overlay(
        &self,
        node: &Node,
        bounds: Rect,
        cx: &mut Context<Self>,
    ) -> Option<Stateful<Div>> {
        let edit = self.image_editing.as_ref().filter(|e| e.node == node.id)?;
        let dimensions = edit
            .spec
            .oriented_dimensions(edit.asset.width, edit.asset.height)
            .ok()?;
        let content = content_rect(bounds, dimensions);
        let frame = crop_geometry(&edit.spec, content).frame();
        let w = content.size.width;
        let h = content.size.height;
        let mut body = div()
            .id("image-crop-surface")
            .occlude()
            .track_focus(&self.image_edit_focus)
            .absolute()
            .left(px(content.origin.x))
            .top(px(content.origin.y))
            .w(px(w))
            .h(px(h))
            .child(edit.preview.clone());
        for (x, y, cw, ch) in [
            (0., 0., w, frame.origin.y),
            (0., frame.max_y(), w, (h - frame.max_y()).max(0.)),
            (0., frame.origin.y, frame.origin.x, frame.size.height),
            (
                frame.max_x(),
                frame.origin.y,
                (w - frame.max_x()).max(0.),
                frame.size.height,
            ),
        ] {
            body = body.child(
                div()
                    .absolute()
                    .left(px(x))
                    .top(px(y))
                    .w(px(cw))
                    .h(px(ch))
                    .bg(rgba(0x00000077)),
            );
        }
        let mut crop = div()
            .absolute()
            .left(px(frame.origin.x))
            .top(px(frame.origin.y))
            .w(px(frame.size.width))
            .h(px(frame.size.height))
            .border_1()
            .border_color(rgb(0xffffff));
        for f in [1. / 3., 2. / 3.] {
            crop = crop
                .child(
                    div()
                        .absolute()
                        .left(px(frame.size.width * f))
                        .top_0()
                        .w(px(1.))
                        .h_full()
                        .bg(rgba(0xffffff66)),
                )
                .child(
                    div()
                        .absolute()
                        .top(px(frame.size.height * f))
                        .left_0()
                        .h(px(1.))
                        .w_full()
                        .bg(rgba(0xffffff66)),
                );
        }
        body = body.child(crop);
        for handle in CORNERS {
            let p = handle.position(frame);
            body = body.child(
                div()
                    .absolute()
                    .left(px(p.x - 3.))
                    .top(px(p.y - 3.))
                    .size(px(6.))
                    .rounded(px(2.))
                    .bg(rgb(0xffffff))
                    .border_1()
                    .border_color(rgb(0x738093)),
            );
        }
        Some(
            body.cursor_crosshair()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        window.prevent_default();
                        this.image_edit_focus.focus(window, cx);
                        this.image_crop_pointer(
                            content,
                            event.position,
                            true,
                            event.modifiers.shift,
                            window,
                            cx,
                        );
                    }),
                )
                .on_mouse_move(
                    cx.listener(move |this, event: &MouseMoveEvent, window, cx| {
                        if event.dragging() {
                            cx.stop_propagation();
                            this.image_crop_pointer(
                                content,
                                event.position,
                                false,
                                event.modifiers.shift,
                                window,
                                cx,
                            );
                        }
                    }),
                )
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| this.finish_image_crop(cx)),
                )
                .on_mouse_up_out(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| this.finish_image_crop(cx)),
                )
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    if event.keystroke.key == "escape" {
                        cx.stop_propagation();
                        this.handle_image_edit(ImageEditAction::Cancel, window, cx);
                    } else if event.keystroke.key == "z"
                        && (event.keystroke.modifiers.control || event.keystroke.modifiers.platform)
                    {
                        cx.stop_propagation();
                        this.handle_image_edit(
                            ImageEditAction::Undo(event.keystroke.modifiers.shift),
                            window,
                            cx,
                        );
                    }
                })),
        )
    }
    pub(super) fn image_crop_toolbar(&self, cx: &mut Context<Self>) -> Div {
        let Some(edit) = &self.image_editing else {
            return div();
        };
        let disabled = self.importing || self.saving;
        let valid = edit.preview.read(cx).ready()
            && edit
                .spec
                .crop_pixels(edit.asset.width, edit.asset.height)
                .is_ok();
        let ratio = edit
            .ratio
            .map(|r| {
                [
                    (1., "1:1"),
                    (4. / 3., "4:3"),
                    (3. / 4., "3:4"),
                    (16. / 9., "16:9"),
                    (9. / 16., "9:16"),
                ]
                .into_iter()
                .find(|(value, _)| (r - value).abs() < 0.001)
                .map(|(_, label)| label.to_string())
                .unwrap_or_else(|| format!("{r:.2}:1"))
            })
            .unwrap_or_else(|| "自由".into());
        let mut bar = div()
            .occlude()
            .h(px(38.))
            .px_2()
            .rounded(px(10.))
            .bg(rgb(0xffffff))
            .shadow_sm()
            .flex()
            .items_center()
            .gap_1()
            .child(chrome::icon("crop", 16.))
            .child(label("裁剪"))
            .child(
                self.button(
                    "crop-ratio",
                    ratio,
                    Action::ImageEdit(ImageEditAction::ToggleRatios),
                    disabled,
                    cx,
                )
                .min_w(px(60.)),
            )
            .child(
                div()
                    .id("crop-dimension-fields")
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(label("W"))
                    .child(
                        Input::new(&edit.width)
                            .w(px(80.))
                            .flex_shrink_0()
                            .h(px(28.))
                            .disabled(disabled)
                            .aria_label("裁剪宽度像素"),
                    )
                    .child(label("H"))
                    .child(
                        Input::new(&edit.height)
                            .w(px(80.))
                            .flex_shrink_0()
                            .h(px(28.))
                            .disabled(disabled)
                            .aria_label("裁剪高度像素"),
                    )
                    .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                        if event.keystroke.key == "enter" {
                            cx.stop_propagation();
                            window.prevent_default();
                            this.handle_image_edit(ImageEditAction::Dimensions, window, cx);
                        } else if event.keystroke.key == "escape" {
                            cx.stop_propagation();
                            window.prevent_default();
                            this.handle_image_edit(ImageEditAction::Cancel, window, cx);
                        }
                    })),
            )
            .child(
                self.button(
                    "crop-apply",
                    "应用",
                    Action::ImageEdit(ImageEditAction::Apply),
                    disabled || !valid,
                    cx,
                )
                .bg(rgb(0x282a30))
                .text_color(rgb(0xffffff)),
            )
            .child(self.button(
                "crop-cancel",
                "取消",
                Action::ImageEdit(if self.importing {
                    ImageEditAction::CancelApply
                } else {
                    ImageEditAction::Cancel
                }),
                false,
                cx,
            ));
        if edit.ratios_open {
            let mut menu = div()
                .occlude()
                .absolute()
                .left(px(48.))
                .top(px(44.))
                .w(px(140.))
                .p_2()
                .bg(rgb(0xffffff))
                .rounded_md()
                .shadow_md()
                .flex()
                .flex_col();
            for (label, ratio) in [
                ("自由", None),
                (
                    "原图",
                    edit.spec
                        .oriented_dimensions(edit.asset.width, edit.asset.height)
                        .ok()
                        .map(|(w, h)| w as f32 / h as f32),
                ),
                ("1:1", Some(1.)),
                ("4:3", Some(4. / 3.)),
                ("3:4", Some(3. / 4.)),
                ("16:9", Some(16. / 9.)),
                ("9:16", Some(9. / 16.)),
            ] {
                menu = menu.child(self.button(
                    SharedString::from(format!("crop-ratio-{label}")),
                    label,
                    Action::ImageEdit(ImageEditAction::Ratio(ratio)),
                    disabled,
                    cx,
                ));
            }
            bar = bar.child(menu);
        }
        bar
    }
    pub(super) fn image_edit_footer(&self, cx: &mut Context<Self>) -> Div {
        let disabled = self.importing || self.saving;
        let mut bar = div().flex().items_center().gap_1();
        for (id, icon, title, action) in [
            (
                "image-left",
                "rotate-ccw",
                "左转90°",
                ImageEditAction::Rotate(false),
            ),
            (
                "image-right",
                "rotate-cw",
                "右转90°",
                ImageEditAction::Rotate(true),
            ),
            (
                "image-flip-h",
                "flip-horizontal-2",
                "水平翻转",
                ImageEditAction::Flip(true),
            ),
            (
                "image-flip-v",
                "flip-vertical-2",
                "垂直翻转",
                ImageEditAction::Flip(false),
            ),
            (
                "image-reset",
                "undo-2",
                "重置本次编辑",
                ImageEditAction::Reset,
            ),
        ] {
            bar = bar.child(self.icon_button(
                id,
                icon,
                title,
                Action::ImageEdit(action),
                disabled,
                cx,
            ));
        }
        bar.child(label("Esc 取消 · 原图保留"))
    }
}
