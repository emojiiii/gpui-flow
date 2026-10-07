//! Creative image display sizing. Installing the plugin opts the host into
//! proportional image handles; reading an older document never changes it.
//! The current display ratio is distinct from the source image's pixel ratio.

use gpui_flow_core::{Node, NodeId, Point, Size};
use gpui_flow_interaction::{
    CanvasCommand, CanvasCursor, CanvasEffect, CanvasEvent, CanvasPlugin, Capability, HitTarget,
    PluginContext, PluginLifecycle, PluginOutput, PointerButton, PointerIdentity, PointerInput,
};
use serde_json::{Map, Value};

pub const IMAGE_ASPECT_LOCK_KEY: &str = "flow_creative_image_aspect_locked";
pub const MIN_IMAGE_DISPLAY_SIZE: f32 = 0.01;
pub const MAX_IMAGE_DISPLAY_SIZE: f32 = 16_384.0;

fn image_data(node: &Node) -> Result<&Map<String, Value>, String> {
    if node.kind != "image" {
        return Err("display aspect lock is only supported for image objects".into());
    }
    node.data
        .as_object()
        .ok_or_else(|| "image data must be an object".into())
}

/// An absent setting defaults to locked without modifying stored data.
/// Explicit values must be booleans; malformed data is never silently coerced.
pub fn aspect_locked(node: &Node) -> Result<bool, String> {
    match image_data(node)?.get(IMAGE_ASPECT_LOCK_KEY) {
        None => Ok(true),
        Some(Value::Bool(locked)) => Ok(*locked),
        Some(_) => Err("image aspect lock must be a boolean".into()),
    }
}

/// Return an edited copy, retaining asset, crop, presentation and host metadata.
pub fn with_aspect_lock(node: &Node, locked: bool) -> Result<Value, String> {
    let mut data = image_data(node)?.clone();
    data.insert(IMAGE_ASPECT_LOCK_KEY.into(), Value::Bool(locked));
    Ok(Value::Object(data))
}

fn checked_size(width: f64, height: f64) -> Result<Size, String> {
    let range = f64::from(MIN_IMAGE_DISPLAY_SIZE)..=f64::from(MAX_IMAGE_DISPLAY_SIZE);
    if !width.is_finite()
        || !height.is_finite()
        || !range.contains(&width)
        || !range.contains(&height)
    {
        return Err(
            "image display width and height must be finite and between 0.01 and 16384".into(),
        );
    }
    Ok(Size::new(width as f32, height as f32))
}

fn validate_size(size: Size) -> Result<(), String> {
    checked_size(f64::from(size.width), f64::from(size.height)).map(|_| ())
}

/// Set the exact requested display width, preserving the current display ratio
/// when locked. Out-of-range input or derived height is rejected, not clamped.
pub fn size_from_width(start: Size, width: f32, locked: bool) -> Result<Size, String> {
    validate_size(start)?;
    let height = if locked {
        f64::from(start.height) * (f64::from(width) / f64::from(start.width))
    } else {
        f64::from(start.height)
    };
    checked_size(f64::from(width), height)
}

/// Set the exact requested display height, preserving the current display ratio
/// when locked. Out-of-range input or derived width is rejected, not clamped.
pub fn size_from_height(start: Size, height: f32, locked: bool) -> Result<Size, String> {
    validate_size(start)?;
    let width = if locked {
        f64::from(start.width) * (f64::from(height) / f64::from(start.height))
    } else {
        f64::from(start.width)
    };
    checked_size(width, f64::from(height))
}

struct ImageResize {
    node: NodeId,
    pointer: PointerIdentity,
    start_screen: Point,
    start_zoom: f64,
    size: Size,
    locked: bool,
}

impl ImageResize {
    fn size_at(&self, pointer: &PointerInput) -> Result<Size, String> {
        // Subtract in f64 before dividing: opposite finite f32 coordinates can
        // overflow a f32 subtraction. The original viewport is fixed for this
        // gesture; wheel navigation cancels capture in the host.
        let dx = (f64::from(pointer.position.x) - f64::from(self.start_screen.x)) / self.start_zoom;
        let dy = (f64::from(pointer.position.y) - f64::from(self.start_screen.y)) / self.start_zoom;
        let width = f64::from(self.size.width);
        let height = f64::from(self.size.height);
        let minimum = f64::from(MIN_IMAGE_DISPLAY_SIZE);
        let maximum = f64::from(MAX_IMAGE_DISPLAY_SIZE);
        if !dx.is_finite() || !dy.is_finite() {
            return Err("image resize needs finite pointer coordinates".into());
        }
        if self.locked != pointer.modifiers.shift {
            let nx = dx / width;
            let ny = dy / height;
            // The axis with the greatest relative movement drives both sizes.
            // Horizontal movement wins ties, including zero movement. Always
            // use the original size so repeated moves and Shift cannot drift.
            let movement = if nx.abs() >= ny.abs() { nx } else { ny };
            let lower = (minimum / width).max(minimum / height);
            let upper = (maximum / width).min(maximum / height);
            let scale = (1.0 + movement).clamp(lower, upper);
            // A single scale bounds both axes; independent axis clamping here
            // would stretch the image. Cast only after bounded f64 arithmetic.
            let size = Size::new((width * scale) as f32, (height * scale) as f32);
            // Validate representable dimensions so a sub-ulp f64 error at the
            // minimum or maximum does not cancel an otherwise bounded drag.
            validate_size(size)?;
            Ok(size)
        } else {
            checked_size(
                (width + dx).clamp(minimum, maximum),
                (height + dy).clamp(minimum, maximum),
            )
        }
    }
}

/// Opt-in graph plugin for Creative's image bottom-right resize handle.
/// Shift temporarily inverts the stored lock; it never changes that setting.
/// All mutations pass through host commands and CreativeGuard. Other node
/// kinds and retained-scene handles keep their existing resize behavior.
#[derive(Default)]
pub struct CreativeImageResizePlugin {
    resize: Option<ImageResize>,
}

impl CanvasPlugin for CreativeImageResizePlugin {
    fn id(&self) -> &str {
        "creative-image-resize"
    }

    fn priority(&self) -> i32 {
        // Lower runs first: pan (-20), selection (0), then default resize (20).
        15
    }

    fn is_default_action(&self) -> bool {
        true
    }

    fn on_lifecycle(&mut self, event: PluginLifecycle) {
        if matches!(
            event,
            PluginLifecycle::Cancelled | PluginLifecycle::Disabled | PluginLifecycle::Uninstalled
        ) {
            // The host rolls back the transaction and releases capture first.
            self.resize = None;
        }
    }

    fn on_event(&mut self, event: &CanvasEvent, cx: &PluginContext<'_>, out: &mut PluginOutput) {
        if !cx.allows(Capability::Edit) {
            return;
        }
        match event {
            CanvasEvent::PointerDown(pointer)
                if pointer.button == Some(PointerButton::Left) && cx.capture_owner.is_none() =>
            {
                let HitTarget::ResizeHandle(id) = &pointer.hit else {
                    return;
                };
                let Some(node) = cx.editor.graph().node(id).filter(|n| n.kind == "image") else {
                    return;
                };
                // This image handle belongs to Creative even when its data or
                // lock makes it unavailable. Do not fall through to stretching
                // in the default plugin. No other hit target is intercepted.
                out.prevent_default();
                if pointer.modifiers.space
                    || crate::layer_guard::node_is_locked(cx.editor.graph(), id)
                    || !gpui_flow_core::node_is_visible(cx.editor.graph(), id)
                    || validate_size(node.size).is_err()
                {
                    return;
                }
                let Ok(locked) = aspect_locked(node) else {
                    return;
                };
                self.resize = Some(ImageResize {
                    node: id.clone(),
                    pointer: pointer.identity(),
                    start_screen: pointer.position,
                    start_zoom: f64::from(cx.editor.viewport.zoom),
                    size: node.size,
                    locked,
                });
                out.command(CanvasCommand::BeginGesture {
                    label: "Resize image".into(),
                });
                out.command(CanvasCommand::CapturePointer {
                    pointer_id: pointer.pointer_id,
                });
                out.effect(CanvasEffect::Cursor(CanvasCursor::Resize));
            }
            CanvasEvent::PointerMove(pointer) | CanvasEvent::PointerUp(pointer) => {
                let Some(resize) = &self.resize else {
                    return;
                };
                if resize.pointer != pointer.identity()
                    || cx.capture_owner != Some("creative-image-resize")
                {
                    return;
                }
                out.prevent_default();
                if crate::layer_guard::node_is_locked(cx.editor.graph(), &resize.node)
                    || cx
                        .editor
                        .graph()
                        .node(&resize.node)
                        .is_none_or(|node| node.kind != "image")
                {
                    self.resize = None;
                    out.command(CanvasCommand::CancelGesture);
                    return;
                }
                let Ok(size) = resize.size_at(pointer) else {
                    self.resize = None;
                    out.command(CanvasCommand::CancelGesture);
                    return;
                };
                out.command(CanvasCommand::ResizeNode {
                    node: resize.node.clone(),
                    size,
                });
                if matches!(event, CanvasEvent::PointerUp(_)) {
                    // Pointer-up can be newer than the last move (or be the
                    // first update). Apply its coordinates before committing.
                    self.resize = None;
                    out.command(CanvasCommand::CommitGesture);
                    out.command(CanvasCommand::ReleasePointer);
                    out.effect(CanvasEffect::Cursor(CanvasCursor::Default));
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CreativeGuard, standard_registry};
    use gpui_flow_core::{Editor, Graph, Group, Rect};
    use gpui_flow_interaction::{AccessPolicy, KeyCode, Modifiers, PluginHost, PointerDevice};
    use serde_json::json;
    use std::sync::Arc;

    fn image() -> Node {
        standard_registry()
            .make_node("image", "image", Point::new(35., 70.), None)
            .unwrap()
            .with_size(Size::new(320., 180.))
    }

    fn host(node: Node, zoom: f32, frame_locked: bool) -> (Editor, PluginHost) {
        let mut graph = Graph::new();
        let id = node.id.clone();
        graph.add_node(node).unwrap();
        if frame_locked {
            let mut group = Group::new(
                "frame",
                "Frame",
                Rect::new(Point::ZERO, Size::new(500., 400.)),
            );
            group.node_ids.insert(id);
            group.locked = true;
            graph.add_group(group).unwrap();
        }
        let mut editor = Editor::new(graph).unwrap();
        editor.viewport.zoom = zoom;
        editor.viewport.pan = Point::new(17., -23.);
        let mut host = PluginHost::with_defaults();
        host.install(CreativeGuard(Arc::new(standard_registry())))
            .unwrap();
        host.install(CreativeImageResizePlugin::default()).unwrap();
        (editor, host)
    }

    fn pointer(editor: &Editor, x: f32, y: f32, shift: bool) -> PointerInput {
        let mut pointer = PointerInput::left(
            editor.viewport.world_to_screen(Point::new(x, y)),
            HitTarget::ResizeHandle("image".into()),
        );
        pointer.pointer_id = 42;
        pointer.modifiers.shift = shift;
        pointer
    }

    fn dispatch(editor: &mut Editor, host: &mut PluginHost, event: CanvasEvent) {
        let report = host.dispatch(editor, event);
        assert!(report.errors.is_empty(), "{:?}", report.errors);
    }

    fn begin(editor: &mut Editor, host: &mut PluginHost) {
        let pointer = pointer(editor, 355., 250., false);
        dispatch(editor, host, CanvasEvent::PointerDown(pointer));
        assert_eq!(host.capture_owner(), Some("creative-image-resize"));
    }

    fn size(editor: &Editor) -> Size {
        editor.graph().node(&"image".into()).unwrap().size
    }

    fn close(actual: Size, expected: Size) {
        assert!((actual.width - expected.width).abs() <= expected.width * 0.000_001);
        assert!((actual.height - expected.height).abs() <= expected.height * 0.000_001);
    }

    #[test]
    fn lock_defaults_without_writing_and_preserves_all_metadata() {
        let mut node = image();
        node.data["crop"] = json!({"x":0.2,"width":0.6});
        node.data["host"] = json!({"tags":["one","two"],"nested":{"enabled":true}});
        let original = node.clone();
        assert!(aspect_locked(&node).unwrap());
        assert_eq!(node, original);
        let unlocked = with_aspect_lock(&node, false).unwrap();
        let mut expected = original.data.clone();
        expected[IMAGE_ASPECT_LOCK_KEY] = json!(false);
        assert_eq!(unlocked, expected);
        node.data = unlocked;
        assert!(!aspect_locked(&node).unwrap());
        node.data = with_aspect_lock(&node, true).unwrap();
        assert!(aspect_locked(&node).unwrap());
        assert_eq!(node.size, original.size);
    }

    #[test]
    fn only_images_and_explicit_boolean_settings_are_accepted() {
        for invalid in [json!(null), json!(0), json!("true"), json!([]), json!({})] {
            let mut node = image();
            node.data[IMAGE_ASPECT_LOCK_KEY] = invalid;
            assert!(aspect_locked(&node).is_err());
        }
        for invalid in [json!(null), json!(42), json!([])] {
            let mut node = image();
            node.data = invalid;
            assert!(aspect_locked(&node).is_err());
            assert!(with_aspect_lock(&node, true).is_err());
        }
        let other = image().with_kind("video");
        assert!(aspect_locked(&other).is_err());
        assert!(with_aspect_lock(&other, false).is_err());
    }

    #[test]
    fn dimensions_preserve_current_ratio_and_unlocked_other_axis() {
        let start = Size::new(300., 120.);
        assert_eq!(
            size_from_width(start, 750., true).unwrap(),
            Size::new(750., 300.)
        );
        assert_eq!(
            size_from_height(start, 300., true).unwrap(),
            Size::new(750., 300.)
        );
        assert_eq!(
            size_from_width(start, 750., false).unwrap(),
            Size::new(750., 120.)
        );
        assert_eq!(
            size_from_height(start, 300., false).unwrap(),
            Size::new(300., 300.)
        );
    }

    #[test]
    fn dimensions_reject_bad_input_and_derived_overflow() {
        for bad in [
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            -1.,
            0.,
            0.009,
            16_385.,
            f32::MAX,
        ] {
            for locked in [false, true] {
                assert!(size_from_width(Size::new(300., 120.), bad, locked).is_err());
                assert!(size_from_height(Size::new(300., 120.), bad, locked).is_err());
                assert!(size_from_width(Size::new(bad, 120.), 300., locked).is_err());
                assert!(size_from_height(Size::new(300., bad), 120., locked).is_err());
            }
        }
        assert!(size_from_width(Size::new(1., 10.), 2_000., true).is_err());
        assert!(size_from_height(Size::new(10., 1.), 2_000., true).is_err());
        assert!(size_from_width(Size::new(10., 1.), 0.01, true).is_err());
        assert!(size_from_height(Size::new(1., 10.), 0.01, true).is_err());
        assert_eq!(
            size_from_width(Size::new(1., 1.), 0.01, true).unwrap(),
            Size::new(0.01, 0.01)
        );
        assert_eq!(
            size_from_height(Size::new(1., 1.), 16_384., true).unwrap(),
            Size::new(16_384., 16_384.)
        );
    }

    #[test]
    fn real_host_preserves_ratio_at_half_and_double_zoom_with_one_undo() {
        for zoom in [0.5, 2.] {
            let (mut editor, mut host) = host(image(), zoom, false);
            let original = editor.document();
            begin(&mut editor, &mut host);
            for (x, y, expected) in [
                (515., 250., Size::new(480., 270.)),
                (375., 340., Size::new(480., 270.)),
                (435., 250., Size::new(400., 225.)),
            ] {
                let input = pointer(&editor, x, y, false);
                dispatch(&mut editor, &mut host, CanvasEvent::PointerMove(input));
                assert_eq!(size(&editor), expected);
            }
            let input = pointer(&editor, 435., 250., false);
            dispatch(&mut editor, &mut host, CanvasEvent::PointerUp(input));
            assert_eq!(editor.history_len(), 1);
            assert!(!editor.transaction_active());
            assert!(!host.has_capture());
            host.execute(&mut editor, CanvasCommand::Undo).unwrap();
            assert_eq!(editor.document(), original);
            assert!(!editor.undo());
        }
    }

    #[test]
    fn release_applies_final_coordinates_even_without_a_move() {
        for zoom in [0.5, 2.] {
            let (mut editor, mut host) = host(image(), zoom, false);
            begin(&mut editor, &mut host);
            let input = pointer(&editor, 515., 250., false);
            dispatch(&mut editor, &mut host, CanvasEvent::PointerUp(input));
            assert_eq!(size(&editor), Size::new(480., 270.));
            assert_eq!(editor.history_len(), 1);
            begin(&mut editor, &mut host);
            let input = pointer(&editor, 515., 250., false);
            dispatch(&mut editor, &mut host, CanvasEvent::PointerMove(input));
            let input = pointer(&editor, 675., 250., false);
            dispatch(&mut editor, &mut host, CanvasEvent::PointerUp(input));
            assert_eq!(size(&editor), Size::new(800., 450.));
        }
    }

    #[test]
    fn shift_inverts_both_settings_without_changing_stored_data_or_drifting() {
        for locked in [false, true] {
            let mut node = image();
            node.data = with_aspect_lock(&node, locked).unwrap();
            let before = node.data.clone();
            let (mut editor, mut host) = host(node, 1., false);
            begin(&mut editor, &mut host);
            for shift in [true, false, true, false] {
                let input = pointer(&editor, 515., 250., shift);
                dispatch(&mut editor, &mut host, CanvasEvent::PointerMove(input));
                let height = if locked != shift { 270. } else { 180. };
                assert_eq!(size(&editor), Size::new(480., height));
            }
            let input = pointer(&editor, 515., 250., true);
            dispatch(&mut editor, &mut host, CanvasEvent::PointerUp(input));
            assert_eq!(size(&editor).height, if locked { 180. } else { 270. });
            assert_eq!(editor.graph().node(&"image".into()).unwrap().data, before);
        }
    }

    #[test]
    fn pointer_bounds_clamp_with_one_scale_and_never_stretch() {
        for start in [
            Size::new(320., 180.),
            Size::new(180., 320.),
            Size::new(16_384., 0.01),
        ] {
            for extreme in [f32::MAX, -f32::MAX] {
                let (mut editor, mut host) = host(image().with_size(start), 0.5, false);
                begin(&mut editor, &mut host);
                let input = PointerInput {
                    position: Point::new(extreme, extreme),
                    pointer_id: 42,
                    ..PointerInput::left(Point::ZERO, HitTarget::Background)
                };
                dispatch(&mut editor, &mut host, CanvasEvent::PointerUp(input));
                let actual = size(&editor);
                validate_size(actual).unwrap();
                let factor = if extreme > 0. {
                    16_384. / start.width.max(start.height)
                } else {
                    0.01 / start.width.min(start.height)
                };
                close(
                    actual,
                    Size::new(start.width * factor, start.height * factor),
                );
                assert_eq!(host.cursor(), CanvasCursor::Default);
            }
        }
    }

    #[test]
    fn cancellation_rolls_back_and_next_gesture_works() {
        for interruption in [
            CanvasEvent::PointerCancel,
            CanvasEvent::PointerCancelFor(PointerIdentity {
                device: PointerDevice::Mouse,
                pointer_id: 42,
            }),
            CanvasEvent::FocusLost,
            CanvasEvent::KeyDown {
                key: KeyCode::Escape,
                modifiers: Modifiers::default(),
            },
            CanvasEvent::PointerMove(PointerInput {
                pointer_id: 42,
                ..Default::default()
            }),
        ] {
            let (mut editor, mut host) = host(image(), 1., false);
            let original = editor.document();
            begin(&mut editor, &mut host);
            let input = pointer(&editor, 515., 250., false);
            dispatch(&mut editor, &mut host, CanvasEvent::PointerMove(input));
            dispatch(&mut editor, &mut host, interruption);
            assert_eq!(editor.document(), original);
            assert!(!editor.transaction_active());
            assert!(!host.has_capture());
            assert_eq!(editor.history_len(), 0);
            begin(&mut editor, &mut host);
            let input = pointer(&editor, 515., 250., false);
            dispatch(&mut editor, &mut host, CanvasEvent::PointerUp(input));
            assert_eq!(size(&editor), Size::new(480., 270.));
            assert_eq!(editor.history_len(), 1);
        }
    }

    #[test]
    fn foreign_pointer_or_device_cannot_move_release_or_cancel_owned_gesture() {
        let (mut editor, mut host) = host(image(), 1., false);
        begin(&mut editor, &mut host);
        for (id, device) in [(99, PointerDevice::Mouse), (42, PointerDevice::Touch)] {
            let mut input = pointer(&editor, 515., 250., false);
            input.pointer_id = id;
            input.device = device;
            dispatch(
                &mut editor,
                &mut host,
                CanvasEvent::PointerMove(input.clone()),
            );
            dispatch(
                &mut editor,
                &mut host,
                CanvasEvent::PointerUp(input.clone()),
            );
            dispatch(
                &mut editor,
                &mut host,
                CanvasEvent::PointerCancelFor(input.identity()),
            );
            assert_eq!(size(&editor), Size::new(320., 180.));
            assert_eq!(host.capture_owner(), Some("creative-image-resize"));
        }
        let input = pointer(&editor, 515., 250., false);
        dispatch(&mut editor, &mut host, CanvasEvent::PointerUp(input));
        assert_eq!(size(&editor), Size::new(480., 270.));
    }

    #[test]
    fn locks_and_malformed_settings_cannot_fall_back_to_default_resize() {
        for mode in 0..3 {
            let mut node = image();
            node.locked = mode == 0;
            if mode == 2 {
                node.data[IMAGE_ASPECT_LOCK_KEY] = json!("false");
            }
            let (mut editor, mut host) = host(node, 1., mode == 1);
            let original = editor.document();
            let input = pointer(&editor, 355., 250., false);
            let report = host.dispatch(&mut editor, CanvasEvent::PointerDown(input));
            assert!(report.errors.is_empty());
            assert!(report.default_prevented);
            assert!(!host.has_capture());
            let input = pointer(&editor, 515., 250., false);
            dispatch(&mut editor, &mut host, CanvasEvent::PointerUp(input));
            assert_eq!(editor.document(), original);
            assert_eq!(editor.history_len(), 0);
        }
    }

    #[test]
    fn space_and_hand_pan_while_readonly_and_policy_changes_are_respected() {
        for editable in [false, true] {
            let (mut editor, mut host) = host(image(), 1., false);
            host.set_editable(editable, &mut editor);
            let mut input = pointer(&editor, 355., 250., false);
            // CanvasView maps both Space and persistent Hand to this modifier.
            input.modifiers.space = true;
            dispatch(
                &mut editor,
                &mut host,
                CanvasEvent::PointerDown(input.clone()),
            );
            assert_eq!(host.capture_owner(), Some("pan"));
            input.position.x += 50.;
            dispatch(
                &mut editor,
                &mut host,
                CanvasEvent::PointerMove(input.clone()),
            );
            dispatch(&mut editor, &mut host, CanvasEvent::PointerUp(input));
            assert_eq!(size(&editor), Size::new(320., 180.));
            assert_eq!(editor.viewport.pan, Point::new(67., -23.));
            assert_eq!(editor.history_len(), 0);
        }
        let (mut editor, mut host) = host(image(), 1., false);
        host.set_policy(
            AccessPolicy {
                navigation: false,
                ..AccessPolicy::editable()
            },
            &mut editor,
        );
        let mut input = pointer(&editor, 355., 250., false);
        input.modifiers.space = true;
        dispatch(&mut editor, &mut host, CanvasEvent::PointerDown(input));
        assert!(!host.has_capture());
        begin(&mut editor, &mut host);
        let input = pointer(&editor, 515., 250., false);
        dispatch(&mut editor, &mut host, CanvasEvent::PointerMove(input));
        host.set_editable(false, &mut editor);
        assert_eq!(size(&editor), Size::new(320., 180.));
        assert!(!host.has_capture());
        assert_eq!(editor.history_len(), 0);
        let input = pointer(&editor, 355., 250., false);
        dispatch(&mut editor, &mut host, CanvasEvent::PointerDown(input));
        assert!(!host.has_capture());
    }

    #[test]
    fn disabling_owned_plugin_cancels_without_committing() {
        let (mut editor, mut host) = host(image(), 1., false);
        begin(&mut editor, &mut host);
        let input = pointer(&editor, 515., 250., false);
        dispatch(&mut editor, &mut host, CanvasEvent::PointerMove(input));
        host.set_enabled("creative-image-resize", false, &mut editor)
            .unwrap();
        assert_eq!(size(&editor), Size::new(320., 180.));
        assert!(!host.has_capture());
        assert!(!editor.transaction_active());
        assert_eq!(editor.history_len(), 0);
    }

    #[test]
    fn unrelated_shape_keeps_default_free_resize_semantics() {
        let node = standard_registry()
            .make_node("shape", "image", Point::ZERO, None)
            .unwrap()
            .with_size(Size::new(320., 180.));
        let (mut editor, mut host) = host(node, 1., false);
        let input = pointer(&editor, 355., 250., false);
        let report = host.dispatch(&mut editor, CanvasEvent::PointerDown(input));
        assert!(!report.default_prevented);
        assert!(report.errors.is_empty());
        assert_eq!(host.capture_owner(), Some("resize"));
        let input = pointer(&editor, 515., 250., false);
        dispatch(
            &mut editor,
            &mut host,
            CanvasEvent::PointerMove(input.clone()),
        );
        dispatch(&mut editor, &mut host, CanvasEvent::PointerUp(input));
        assert_eq!(size(&editor), Size::new(480., 180.));
        assert_eq!(editor.history_len(), 1);
    }

    #[test]
    fn loading_an_existing_stretched_image_never_changes_geometry_or_data() {
        let (mut editor, mut host) = host(image(), 1., false);
        let mut document = editor.document();
        let node = document.graph.nodes.get_mut(&"image".into()).unwrap();
        node.size = Size::new(500., 100.);
        node.data
            .as_object_mut()
            .unwrap()
            .remove(IMAGE_ASPECT_LOCK_KEY);
        host.execute(&mut editor, CanvasCommand::LoadDocument(document.clone()))
            .unwrap();
        assert_eq!(editor.document(), document);
        assert!(aspect_locked(editor.graph().node(&"image".into()).unwrap()).unwrap());
        assert_eq!(editor.document(), document);
        begin(&mut editor, &mut host);
        let input = pointer(&editor, 605., 250., false);
        dispatch(&mut editor, &mut host, CanvasEvent::PointerUp(input));
        assert_eq!(size(&editor), Size::new(750., 150.));
    }
}
