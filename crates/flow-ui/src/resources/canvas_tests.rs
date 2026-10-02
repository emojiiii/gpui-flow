use super::*;
use gpui_flow_engine::PropertyKey;
use gpui_flow_scene::{Bounds, Color, DisplayObject, Point, Scene, Size, display};
fn setup() -> (SceneEngine, SceneResourceAdapter, ElementHandle) {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("c", display::Canvas::new(10., 8.)),
            None,
            None,
        )
        .unwrap();
    let mut engine = SceneEngine::new(scene).unwrap();
    let handle = engine.handle(&"c".into()).unwrap();
    let adapter = SceneResourceAdapter::new(&mut engine).unwrap();
    (engine, adapter, handle)
}
fn fill(
    s: &mut gpui_flow_composite::CanvasSurface,
) -> Result<(), gpui_flow_composite::CompositeError> {
    s.fill_rect(
        Bounds::new(Point::ZERO, Size::new(10., 8.)),
        Color::hex(0xff0000),
    )
}
fn at(r: &SceneResourceAdapter, e: &SceneEngine, h: ElementHandle, x: u32, y: u32) -> [u8; 4] {
    let p = r.canvas_snapshot(e, h).unwrap();
    p.premultiplied_rgba()[((y * p.width() + x) * 4) as usize..][..4]
        .try_into()
        .unwrap()
}
#[test]
fn canvas_resource_initialization_and_pixel_edits_leave_scene_history_untouched() {
    let (mut e, mut r, h) = setup();
    let before = e.scene().clone();
    let revision = e.revision();
    let events = r.edit_canvas(&mut e, h, fill).unwrap();
    assert!(
        events
            .iter()
            .any(|v| v.state == ResourceState::Loaded && v.ticket.kind == ResourceKind::Canvas)
    );
    assert_eq!(at(&r, &e, h, 2, 2), [255, 0, 0, 255]);
    assert_eq!(e.scene(), &before);
    assert_eq!(e.revision(), revision);
    assert!(r.images.is_empty());
    assert!(r.playbacks.is_empty());
    let saved = serde_json::to_string(e.scene()).unwrap();
    assert!(!saved.contains("pixels"));
    let mut second = SceneEngine::new(serde_json::from_str(&saved).unwrap()).unwrap();
    let mut other = SceneResourceAdapter::new(&mut second).unwrap();
    other.prepare_canvases(&mut second).unwrap();
    let h2 = second.handle(&"c".into()).unwrap();
    assert_eq!(at(&other, &second, h2, 2, 2), [0; 4]);
    assert!(r.edit_canvas(&mut second, h2, fill).is_err());
}
#[test]
fn intermediate_safe_shrinks_and_unsafe_a_b_a_cannot_resurrect_pixels() {
    let (mut e, mut r, h) = setup();
    r.edit_canvas(&mut e, h, fill).unwrap();
    let mut c = display::Canvas::new(4., 4.);
    c.safe_resize = true;
    e.set_property(
        h,
        PropertyKey::Shape,
        serde_json::to_value(Shape::from(c.clone())).unwrap(),
    )
    .unwrap();
    c.size = Size::new(10., 8.);
    e.set_property(
        h,
        PropertyKey::Shape,
        serde_json::to_value(Shape::from(c.clone())).unwrap(),
    )
    .unwrap();
    r.prepare_canvases(&mut e).unwrap();
    assert_eq!(at(&r, &e, h, 3, 3), [255, 0, 0, 255]);
    assert_eq!(at(&r, &e, h, 5, 3), [0; 4]);
    r.edit_canvas(&mut e, h, fill).unwrap();
    c.safe_resize = false;
    c.size = Size::new(4., 4.);
    e.set_property(
        h,
        PropertyKey::Shape,
        serde_json::to_value(Shape::from(c.clone())).unwrap(),
    )
    .unwrap();
    c.safe_resize = true;
    c.size = Size::new(10., 8.);
    e.set_property(
        h,
        PropertyKey::Shape,
        serde_json::to_value(Shape::from(c)).unwrap(),
    )
    .unwrap();
    r.prepare_canvases(&mut e).unwrap();
    assert_eq!(at(&r, &e, h, 3, 3), [0; 4]);
}
#[test]
fn corner_and_smooth_edits_preserve_surface_but_change_composite_observation() {
    let (mut e, mut r, h) = setup();
    r.edit_canvas(&mut e, h, fill).unwrap();
    let cfg = match &e.object(h).unwrap().shape {
        Shape::Canvas(c) => c.clone(),
        _ => unreachable!(),
    };
    let before = r.composite_canvas(&e, &"c".into(), &cfg);
    let mut next = cfg.clone().rounded(3.);
    next.smooth = false;
    e.set_property(
        h,
        PropertyKey::Shape,
        serde_json::to_value(Shape::from(next.clone())).unwrap(),
    )
    .unwrap();
    assert!(r.composite_canvas(&e, &"c".into(), &cfg).image.is_err());
    r.prepare_canvases(&mut e).unwrap();
    assert_eq!(at(&r, &e, h, 2, 2), [255, 0, 0, 255]);
    let after = r.composite_canvas(&e, &"c".into(), &next);
    assert!(!before.resource_eq(&after));
}
#[test]
fn detached_destroyed_retyped_and_disposed_canvas_release_owned_pixels_immediately() {
    for mode in 0..4 {
        let (mut e, mut r, h) = setup();
        r.edit_canvas(&mut e, h, fill).unwrap();
        let pixels = r.canvas_snapshot(&e, h).unwrap();
        let weak = Arc::downgrade(&pixels);
        drop(pixels);
        match mode {
            0 => {
                let (sub, _) = e.detach(h).unwrap();
                assert!(weak.upgrade().is_none());
                e.attach(&sub, None, None).unwrap();
                r.prepare_canvases(&mut e).unwrap();
                assert_eq!(at(&r, &e, h, 2, 2), [0; 4]);
            }
            1 => {
                e.destroy(h).unwrap();
            }
            2 => {
                e.set_property(
                    h,
                    PropertyKey::Shape,
                    serde_json::to_value(Shape::from(display::Rect::new(10., 8.))).unwrap(),
                )
                .unwrap();
            }
            _ => {
                r.dispose(&mut e).unwrap();
            }
        }
        assert!(weak.upgrade().is_none());
    }
}
#[test]
fn failed_pixel_edit_is_atomic_and_cannot_resize_outside_document_transaction() {
    let (mut e, mut r, h) = setup();
    r.edit_canvas(&mut e, h, fill).unwrap();
    let before = r.canvas_snapshot(&e, h).unwrap();
    assert!(
        r.edit_canvas(&mut e, h, |s| {
            s.clear()?;
            Err(gpui_flow_composite::CompositeError::TooLarge)
        })
        .is_err()
    );
    assert!(Arc::ptr_eq(&before, &r.canvas_snapshot(&e, h).unwrap()));
    assert!(
        r.edit_canvas(&mut e, h, |s| s
            .reconfigure(display::Canvas::new(2., 2.))
            .map(|_| ()))
            .is_err()
    );
    assert!(Arc::ptr_eq(&before, &r.canvas_snapshot(&e, h).unwrap()));
    e.destroy(h).unwrap();
    e.create("canvas", "c", serde_json::json!({}), None, None)
        .unwrap();
    assert!(r.edit_canvas(&mut e, h, fill).is_err());
}
#[test]
fn surface_capacity_fails_explicitly_and_disposal_frees_admission() {
    let (mut e, mut r, _) = setup();
    e.transaction(|scene| {
        scene.remove(&"c".into())?;
        for i in 0..5 {
            scene.insert(
                DisplayObject::new(format!("c{i}"), display::Canvas::new(2048., 2048.)),
                None,
                None,
            )?;
        }
        Ok(())
    })
    .unwrap();
    let events = r.prepare_canvases(&mut e).unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|v| v.state == ResourceState::Loaded)
            .count(),
        2
    );
    assert_eq!(
        events
            .iter()
            .filter(|v| matches!(v.state, ResourceState::Failed(_)))
            .count(),
        3
    );
    let h = e.handle(&"c0".into()).unwrap();
    e.destroy(h).unwrap();
    r.retry_canvases(&mut e).unwrap();
    // Existing immutable sources plus candidates remain budgeted conservatively.
    assert!(
        r.canvases
            .image(
                r.tracker
                    .ticket(&e, e.handle(&"c2".into()).unwrap())
                    .unwrap()
            )
            .is_some()
    );
}
