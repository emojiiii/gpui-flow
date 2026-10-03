use crate::scene::{
    SceneCommand, SceneResizeGeometry, SceneSelectionResizeGeometry, object_editable,
};
use crate::*;
use gpui_flow_core::{Point, Rect};
use gpui_flow_editor::{
    BuiltinResizeGeometry, BuiltinResizePolicy, ResizeHandle, ResizeOptions, SceneEditor,
    TextBoxResizeGeometry, TextFontResizeGeometry, resize_transform, resize_transform_with_options,
};
use gpui_flow_scene::{Affine, DisplayId, Shape};
struct Resize {
    id: DisplayId,
    target: gpui_flow_engine::ElementHandle,
    inverse: Affine,
    original: Affine,
    bounds: Rect,
    start_local: Point,
    uniform: bool,
    handle: Option<ResizeHandle>,
    minimum_scale: Point,
    text_box: Option<TextBoxResizeGeometry>,
    text_font: Option<TextFontResizeGeometry>,
    builtin: Option<BuiltinResizeGeometry>,
}
struct SelectionResize {
    geometry: SceneSelectionResizeGeometry,
    start_world: Point,
    handle: ResizeHandle,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextResizeMode {
    #[default]
    Transform,
    /// Named single-selection plain Text only; other targets retain transforms.
    Box,
    /// Scale the fixed Text box, font and padding without scaling its matrix.
    FontSize,
}
#[derive(Default)]
pub struct SceneResizePlugin {
    text_mode: TextResizeMode,
    builtin_intrinsic: bool,
    resize: Option<Resize>,
    selection_resize: Option<SelectionResize>,
}
impl SceneResizePlugin {
    /// Opt-in named single-selection policies for non-Text built-ins. Text keeps
    /// its independently configured mode; shared selections retain transforms.
    pub fn with_builtin_resize(mut self, intrinsic: bool) -> Self {
        self.builtin_intrinsic = intrinsic;
        self
    }
    pub fn with_text_resize_mode(mut self, mode: TextResizeMode) -> Self {
        self.text_mode = mode;
        self
    }
}
impl CanvasPlugin<SceneEditor, SceneCommand> for SceneResizePlugin {
    fn id(&self) -> &str {
        "resize"
    }
    fn priority(&self) -> i32 {
        10
    }
    fn is_default_action(&self) -> bool {
        true
    }
    fn resize_bounds(&self, editor: &SceneEditor, id: &str, paint_bounds: Rect) -> Option<Rect> {
        let id = DisplayId::from(id);
        let node = editor.scene().get(&id)?;
        if let Shape::Text(text) = &node.object.shape {
            return Some(
                if self.text_mode == TextResizeMode::Transform
                    || !crate::scene::resize_handles::affine_text_supported(
                        &node.object,
                        TransformCapabilities { affine_text: true },
                    )
                {
                    paint_bounds
                } else {
                    text.bounds()
                },
            );
        }
        if self.builtin_intrinsic {
            Some(
                BuiltinResizeGeometry::new(editor.scene(), &id)
                    .ok()?
                    .bounds(),
            )
        } else {
            Some(paint_bounds)
        }
    }
    fn on_lifecycle(&mut self, event: PluginLifecycle) {
        if matches!(
            event,
            PluginLifecycle::Cancelled | PluginLifecycle::Disabled | PluginLifecycle::Uninstalled
        ) {
            self.resize = None;
            self.selection_resize = None;
        }
    }
    fn on_event(
        &mut self,
        event: &CanvasEvent,
        cx: &PluginContext<'_, SceneEditor>,
        out: &mut PluginOutput<SceneCommand>,
    ) {
        if !cx.allows(Capability::Edit) {
            return;
        }
        match event {
            CanvasEvent::PointerDown(p) if p.button == Some(PointerButton::Left) => {
                if let HitTarget::SelectionResizeHandle { handle } = p.hit {
                    if cx.editor.editing() || p.modifiers.space {
                        return;
                    }
                    let Some(geometry) =
                        SceneSelectionResizeGeometry::for_selection_with_capabilities(
                            cx.editor,
                            cx.transforms,
                        )
                    else {
                        return;
                    };
                    let start_world = cx.editor.viewport.screen_to_world(p.position);
                    if !start_world.is_finite() {
                        return;
                    }
                    let cursor = geometry
                        .screen_handles(cx.editor)
                        .and_then(|handles| handles.into_iter().find(|h| h.handle == handle))
                        .map_or(CanvasCursor::Resize, |h| h.cursor);
                    out.command(SceneCommand::BeginGesture);
                    out.command(SceneCommand::CapturePointerForSelection {
                        pointer_id: p.pointer_id,
                        targets: geometry.roots.iter().map(|root| root.target).collect(),
                    });
                    self.selection_resize = Some(SelectionResize {
                        geometry,
                        start_world,
                        handle,
                    });
                    out.effect(CanvasEffect::Cursor(cursor));
                    return;
                }
                let (id, handle) = match &p.hit {
                    HitTarget::ObjectResize(id) => (id, None),
                    HitTarget::ObjectResizeHandle { id, handle } => (id, Some(*handle)),
                    _ => return,
                };
                let id = DisplayId::from(id.clone());
                if cx.editor.editing() {
                    // A Text-owned edit or another gesture cannot be stolen
                    // by a manually supplied handle hit.
                    return;
                }
                if p.modifiers.space
                    || (handle.is_some()
                        && (cx.editor.selection.ids.len() != 1
                            || !cx.editor.selection.ids.contains(&id)))
                {
                    return;
                }
                if !object_editable(cx.editor.scene(), &id) {
                    return;
                }
                let Some(node) = cx.editor.scene().get(&id) else {
                    return;
                };
                let Ok(Some(bounds)) = cx
                    .editor
                    .scene()
                    .local_bounds_for_view(&id, cx.editor.viewport.zoom)
                else {
                    return;
                };
                let Some(inverse) = cx
                    .editor
                    .scene()
                    .world_transform(&id)
                    .ok()
                    .and_then(Affine::inverse)
                else {
                    return;
                };
                let geometry = if handle.is_some() {
                    let Some(geometry) = SceneResizeGeometry::for_target_with_capabilities(
                        cx.editor,
                        &id,
                        cx.transforms,
                    ) else {
                        return;
                    };
                    let Some(bounds) = self.resize_bounds(cx.editor, &id.0, geometry.bounds) else {
                        return;
                    };
                    let Some(geometry) = geometry.with_bounds(bounds) else {
                        return;
                    };
                    Some(geometry)
                } else {
                    None
                };
                let Some(target) = cx.editor.engine.handle(&id) else {
                    return;
                };
                let start_local =
                    inverse.transform_point(cx.editor.viewport.screen_to_world(p.position));
                if !start_local.is_finite() {
                    return;
                }
                let text_box = if handle.is_some()
                    && self.text_mode == TextResizeMode::Box
                    && crate::scene::resize_handles::affine_text_supported(
                        &node.object,
                        TransformCapabilities { affine_text: true },
                    ) {
                    let Shape::Text(text) = &node.object.shape else {
                        unreachable!()
                    };
                    TextBoxResizeGeometry::new(text).ok()
                } else {
                    None
                };
                let text_font = if handle.is_some()
                    && self.text_mode == TextResizeMode::FontSize
                    && crate::scene::resize_handles::affine_text_supported(
                        &node.object,
                        TransformCapabilities { affine_text: true },
                    ) {
                    let Shape::Text(text) = &node.object.shape else {
                        unreachable!()
                    };
                    TextFontResizeGeometry::new(text).ok()
                } else {
                    None
                };
                let builtin = if handle.is_some()
                    && self.builtin_intrinsic
                    && !matches!(node.object.shape, Shape::Text(_))
                {
                    let Ok(geometry) = BuiltinResizeGeometry::new(cx.editor.scene(), &id) else {
                        return;
                    };
                    Some(geometry)
                } else {
                    None
                };
                self.resize = Some(Resize {
                    id,
                    target,
                    text_box,
                    text_font,
                    builtin,
                    inverse,
                    original: node.object.transform,
                    bounds: geometry.map_or(bounds, |g| g.bounds),
                    start_local,
                    uniform: geometry.map_or(
                        matches!(node.object.shape, Shape::Text(_) | Shape::Image(_)),
                        |g| g.uniform,
                    ),
                    handle,
                    minimum_scale: geometry.map_or(Point::new(0.05, 0.05), |g| g.minimum_scale),
                });
                out.command(SceneCommand::BeginGesture);
                out.command(if handle.is_some() {
                    // Named single handles share the revision/selection guard
                    // already used by shared resize and rotation capture.
                    SceneCommand::CapturePointerForSelection {
                        pointer_id: p.pointer_id,
                        targets: vec![target],
                    }
                } else {
                    SceneCommand::CapturePointerForTarget {
                        pointer_id: p.pointer_id,
                        target,
                    }
                });
                let cursor = geometry
                    .and_then(|g| g.screen_handles(cx.editor))
                    .and_then(|handles| handles.into_iter().find(|h| Some(h.handle) == handle))
                    .map_or(CanvasCursor::Resize, |h| h.cursor);
                out.effect(CanvasEffect::Cursor(cursor));
            }
            CanvasEvent::PointerMove(p) => {
                if let Some(resize) = &self.selection_resize {
                    let geometry = resize.geometry.geometry;
                    let Some(delta) = resize_transform_with_options(
                        Affine::IDENTITY,
                        geometry.bounds,
                        resize.start_world,
                        cx.editor.viewport.screen_to_world(p.position),
                        ResizeOptions {
                            handle: resize.handle,
                            uniform: geometry.uniform || p.modifiers.shift,
                            minimum_scale: geometry.minimum_scale,
                        },
                    ) else {
                        out.command(SceneCommand::CancelGesture);
                        return;
                    };
                    let Some(transforms) = resize.geometry.transforms(delta) else {
                        out.command(SceneCommand::CancelGesture);
                        return;
                    };
                    out.command(SceneCommand::SetResizeTransforms { transforms });
                } else if let Some(resize) = &self.resize {
                    let current = resize
                        .inverse
                        .transform_point(cx.editor.viewport.screen_to_world(p.position));
                    if let Some(geometry) = &resize.builtin {
                        let Some(result) = geometry.resize(
                            resize.original,
                            resize.start_local,
                            current,
                            ResizeOptions {
                                handle: resize.handle.expect("named intrinsic handle"),
                                uniform: p.modifiers.shift
                                    || (geometry.policy() == BuiltinResizePolicy::Children
                                        && resize.uniform),
                                minimum_scale: resize.minimum_scale,
                            },
                        ) else {
                            out.command(SceneCommand::CancelGesture);
                            return;
                        };
                        out.command(SceneCommand::SetBuiltinResize {
                            target: resize.target,
                            resize: result,
                        });
                        return;
                    }
                    if let Some(geometry) = resize.text_font {
                        let Some(result) = geometry.resize(
                            resize.original,
                            resize.start_local,
                            current,
                            ResizeOptions {
                                handle: resize.handle.expect("named Text font handle"),
                                uniform: false,
                                minimum_scale: resize.minimum_scale,
                            },
                        ) else {
                            out.command(SceneCommand::CancelGesture);
                            return;
                        };
                        out.command(SceneCommand::SetTextFontResize {
                            target: resize.target,
                            resize: result,
                        });
                        return;
                    }
                    if let Some(geometry) = resize.text_box {
                        let Some(result) = geometry.resize(
                            resize.original,
                            resize.start_local,
                            current,
                            ResizeOptions {
                                handle: resize.handle.expect("named Text box handle"),
                                uniform: p.modifiers.shift,
                                minimum_scale: resize.minimum_scale,
                            },
                        ) else {
                            out.command(SceneCommand::CancelGesture);
                            return;
                        };
                        out.command(SceneCommand::SetTextBoxResize {
                            target: resize.target,
                            size: result.size,
                            transform: result.transform,
                        });
                        return;
                    }
                    let transform = if let Some(handle) = resize.handle {
                        let Some(transform) = resize_transform_with_options(
                            resize.original,
                            resize.bounds,
                            resize.start_local,
                            current,
                            ResizeOptions {
                                handle,
                                uniform: resize.uniform || p.modifiers.shift,
                                minimum_scale: resize.minimum_scale,
                            },
                        ) else {
                            out.command(SceneCommand::CancelGesture);
                            return;
                        };
                        transform
                    } else {
                        // Keep the legacy untyped bottom-right API's max-factor
                        // uniform rule. Named handles use signed projection above.
                        let mut transform = resize_transform(
                            resize.original,
                            resize.bounds,
                            resize.start_local,
                            current,
                        );
                        if (resize.uniform || p.modifiers.shift)
                            && let Some(inverse) = resize.original.inverse()
                        {
                            let relative = inverse * transform;
                            let factor = relative.a.max(relative.d);
                            transform = resize.original
                                * Affine::scale(factor, factor).around(resize.bounds.origin);
                        }
                        transform
                    };
                    if resize.handle.is_some()
                        && !SceneResizeGeometry::accepts_transform_with_capabilities(
                            cx.editor,
                            &resize.id,
                            transform,
                            cx.transforms,
                        )
                    {
                        out.command(SceneCommand::CancelGesture);
                        return;
                    }
                    out.command(if resize.handle.is_some() {
                        SceneCommand::SetResizeTransform {
                            target: resize.target,
                            transform,
                        }
                    } else {
                        SceneCommand::SetTransform {
                            id: resize.id.clone(),
                            transform,
                        }
                    });
                } else if cx.capture_owner.is_none() {
                    let cursor = if let HitTarget::ObjectResizeHandle { id, handle } = &p.hit {
                        SceneResizeGeometry::for_target_with_capabilities(
                            cx.editor,
                            &DisplayId::from(id.clone()),
                            cx.transforms,
                        )
                        .and_then(|g| g.with_bounds(self.resize_bounds(cx.editor, id, g.bounds)?))
                        .and_then(|g| g.screen_handles(cx.editor))
                        .and_then(|handles| handles.into_iter().find(|h| h.handle == *handle))
                        .map_or(CanvasCursor::Default, |h| h.cursor)
                    } else if let HitTarget::SelectionResizeHandle { handle } = p.hit {
                        SceneSelectionResizeGeometry::for_selection_with_capabilities(
                            cx.editor,
                            cx.transforms,
                        )
                        .and_then(|g| g.screen_handles(cx.editor))
                        .and_then(|handles| handles.into_iter().find(|h| h.handle == handle))
                        .map_or(CanvasCursor::Default, |h| h.cursor)
                    } else {
                        CanvasCursor::Default
                    };
                    out.effect(CanvasEffect::Cursor(cursor));
                }
            }
            CanvasEvent::PointerLeave
                if self.resize.is_none()
                    && self.selection_resize.is_none()
                    && cx.capture_owner.is_none() =>
            {
                out.effect(CanvasEffect::Cursor(CanvasCursor::Default));
            }
            CanvasEvent::PointerUp(_)
                if self.resize.take().is_some() || self.selection_resize.take().is_some() =>
            {
                out.command(SceneCommand::CommitGesture);
                out.command(SceneCommand::ReleasePointer);
                out.effect(CanvasEffect::Cursor(CanvasCursor::Default));
            }
            _ => {}
        }
    }
}
