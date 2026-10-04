//! Public adapter-level CUS-01 boundaries. No window/GPU receipt is implied.
use gpui::{AnyElement, App, Window};
use gpui_flow::{
    composite::{
        CanvasBackdrop, CompositeError, CompositeLimits, CpuFrame, DomainSource, PixelBudget,
        RasterViewport, SceneUnit, owned_canvas, rasterize, scene_units,
    },
    core::Viewport,
    display::{DisplayRegistry, DisplayRenderer, RenderObject},
    editor::SceneEditor,
    engine::{
        CustomDefinition, DirtyFlags, ElementRegistry, PropertyDescriptor, PropertyKey,
        PropertySchema, SceneEngine,
    },
    scene::{
        Affine, Bounds, Color, Contour, DisplayObject, EraserMode, HitTestOptions, MaskMode, Point,
        Scene, Shape, Size, Style, display,
    },
};
use serde_json::{Value, json};
use std::sync::{Arc, atomic::AtomicBool};

fn engine() -> SceneEngine {
    let mut registry = ElementRegistry::default();
    registry
        .register(CustomDefinition::new(
            "acceptance.diamond",
            PropertySchema {
                properties: vec![PropertyDescriptor::number(
                    "radius",
                    20.,
                    1.,
                    100.,
                    DirtyFlags::GEOMETRY | DirtyFlags::HIT | DirtyFlags::PAINT,
                )],
                allow_unknown: false,
            },
            |props: &Value| {
                let r = props["radius"].as_f64().unwrap() as f32;
                Ok((
                    Bounds::new(Point::ZERO, Size::new(2. * r, 2. * r)),
                    vec![Contour {
                        points: vec![
                            Point::new(r, 0.),
                            Point::new(2. * r, r),
                            Point::new(r, 2. * r),
                            Point::new(0., r),
                        ],
                        closed: true,
                    }],
                ))
            },
        ))
        .unwrap();
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("root", display::Group::new()),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("sibling", display::Rect::new(30., 30.))
                .with_style(Style::fill(Color::WHITE)),
            Some("root".into()),
            None,
        )
        .unwrap();
    scene
        .insert(
            registry
                .create("acceptance.diamond", "custom", json!({}))
                .unwrap()
                .with_transform(Affine::translation(50., 20.))
                .with_style(Style::fill(Color::hex(0x808080))),
            Some("root".into()),
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("outside", display::Rect::new(20., 20.))
                .with_transform(Affine::translation(200., 0.)),
            None,
            None,
        )
        .unwrap();
    SceneEngine::with_registry(scene, registry).unwrap()
}
fn custom_render_object(engine: &SceneEngine) -> RenderObject {
    let paint = engine.paint_list().unwrap();
    let item = paint
        .iter()
        .find(|item| item.object.id.as_str() == "custom")
        .unwrap();
    RenderObject::from_paint_item(
        item,
        Viewport {
            pan: Point::new(7., 11.),
            zoom: 2.,
            ..Default::default()
        },
    )
}

#[test]
fn editor_undo_redo_restores_geometry_bounds_hit_and_native_render_input() {
    let mut editor = SceneEditor::from_engine(engine());
    let handle = editor.engine.handle(&"custom".into()).unwrap();
    let before = editor.scene().clone();
    let old_render = custom_render_object(&editor.engine);
    let probe = Point::new(110., 50.);
    assert!(
        editor
            .engine
            .hit_test(probe, HitTestOptions::default())
            .unwrap()
            .is_none()
    );
    editor
        .set_property(handle, PropertyKey::Custom("radius".into()), json!(40.))
        .unwrap();
    let changed = custom_render_object(&editor.engine);
    assert_eq!(
        changed.screen_transform,
        Affine::translation(107., 51.) * Affine::scale(2., 2.)
    );
    let Shape::Custom(custom) = &changed.object.shape else {
        panic!("lost Custom")
    };
    assert_eq!(custom.bounds.size, Size::new(80., 80.));
    assert!(
        editor
            .engine
            .hit_test(probe, HitTestOptions::default())
            .unwrap()
            .is_some()
    );
    assert!(editor.undo());
    assert_eq!(editor.scene(), &before);
    assert!(custom_render_object(&editor.engine) == old_render);
    assert_eq!(editor.engine.handle(&"custom".into()).unwrap(), handle);
    assert!(
        editor
            .engine
            .hit_test(probe, HitTestOptions::default())
            .unwrap()
            .is_none()
    );
    assert!(editor.redo());
    assert!(custom_render_object(&editor.engine) == changed);
    assert!(
        editor
            .engine
            .hit_test(probe, HitTestOptions::default())
            .unwrap()
            .is_some()
    );
}

struct UncalledRenderer;
impl DisplayRenderer for UncalledRenderer {
    fn render(&self, _: &RenderObject, _: &mut Window, _: &mut App) -> AnyElement {
        panic!("registry ownership test must never fabricate a native window");
    }
}
#[test]
fn explicit_renderer_registration_replacement_and_removal_are_per_registry() {
    let mut left = DisplayRegistry::default();
    let mut right = DisplayRegistry::default();
    assert!(left.get("acceptance.diamond").is_none());
    left.register("acceptance.diamond", UncalledRenderer);
    assert!(right.get("acceptance.diamond").is_none());
    let old = left.get("acceptance.diamond").unwrap();
    right.register("acceptance.diamond", UncalledRenderer);
    assert!(!Arc::ptr_eq(
        &old,
        &right.get("acceptance.diamond").unwrap()
    ));
    left.register("acceptance.diamond", UncalledRenderer);
    assert!(!Arc::ptr_eq(&old, &left.get("acceptance.diamond").unwrap()));
    assert!(left.remove("acceptance.diamond"));
    assert!(!left.remove("acceptance.diamond"));
    assert!(right.get("acceptance.diamond").is_some());
}
#[test]
fn custom_rejects_entire_required_domain_without_sibling_or_native_fallback() {
    let mut editor = SceneEditor::from_engine(engine());
    let original = editor.scene().clone();
    let native = scene_units(editor.scene(), CompositeLimits::default()).unwrap();
    assert!(native.contains(&SceneUnit::Native("custom".into())));
    editor
        .set_property(
            editor.engine.handle(&"root".into()).unwrap(),
            PropertyKey::Opacity,
            json!(0.5),
        )
        .unwrap();
    let rejected = scene_units(editor.scene(), CompositeLimits::default()).unwrap();
    assert_eq!(rejected.len(), 2);
    assert!(matches!(&rejected[0], SceneUnit::Rejected(issue)
        if issue.object.as_str() == "root" && issue.error == CompositeError::UnsupportedShape("custom".into())));
    assert_eq!(rejected[1], SceneUnit::Native("outside".into()));
    // The valid document edit is kept; only output for the complete domain rejects.
    assert_eq!(
        editor
            .scene()
            .get(&"root".into())
            .unwrap()
            .object
            .style
            .opacity,
        0.5
    );
    assert!(editor.undo());
    assert_eq!(editor.scene(), &original);
    assert_eq!(
        scene_units(editor.scene(), CompositeLimits::default()).unwrap(),
        native
    );
}
#[test]
fn owned_canvas_custom_rejection_never_produces_partial_output_or_mutates_input() {
    let engine = engine();
    let before = engine.scene().clone();
    let revision = engine.revision();
    assert_eq!(
        owned_canvas(
            engine.scene(),
            CanvasBackdrop {
                color: Color::WHITE,
                grid: None
            },
            CompositeLimits::default()
        ),
        Err(CompositeError::UnsupportedShape("custom".into()))
    );
    assert_eq!(engine.scene(), &before);
    assert_eq!(engine.revision(), revision);
}

fn path_domain(scene: &Scene) -> DomainSource {
    let mut units = scene_units(scene, CompositeLimits::default()).unwrap();
    assert_eq!(units.len(), 2);
    assert_eq!(units[1], SceneUnit::Native("outside".into()));
    let SceneUnit::Domain(source) = units.remove(0) else {
        panic!("Custom Path geometry must remain in the complete root domain")
    };
    assert_eq!(source.root_id().as_str(), "root");
    *source
}
fn path_pixels(source: &DomainSource) -> CpuFrame {
    let bounds = Bounds::new(Point::ZERO, Size::new(150., 120.));
    rasterize(
        source,
        RasterViewport {
            world_to_screen: Affine::IDENTITY,
            canvas_bounds: bounds,
            content_mask: bounds,
            device_scale: 1.,
        },
        &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap()
}
fn pixel(frame: &CpuFrame, x: u32, y: u32) -> [u8; 4] {
    let x = x as f32 - frame.screen_bounds.origin.x;
    let y = y as f32 - frame.screen_bounds.origin.y;
    if x < 0. || y < 0. || x >= frame.width as f32 || y >= frame.height as f32 {
        return [0; 4];
    }
    frame.premul_rgba[((y as u32 * frame.width + x as u32) * 4) as usize..][..4]
        .try_into()
        .unwrap()
}

#[test]
fn dynamic_custom_path_masks_and_erasers_rebuild_pixels_and_undo_without_stale_placement() {
    for mask in [false, true] {
        let mut engine = engine();
        engine
            .transaction(|scene| {
                scene.remove(&"sibling".into())?;
                scene.update_object(&"custom".into(), |o| {
                    o.style.fill = None;
                    if mask {
                        o.mask = MaskMode::Path;
                    } else {
                        o.eraser = EraserMode::Path;
                    }
                })?;
                scene.insert(
                    DisplayObject::new("ink", display::Rect::new(140., 110.))
                        .with_style(Style::fill(Color::WHITE)),
                    Some("root".into()),
                    if mask { None } else { Some(0) },
                )?;
                Ok(())
            })
            .unwrap();
        let mut editor = SceneEditor::from_engine(engine);
        let handle = editor.engine.handle(&"custom".into()).unwrap();
        let before_scene = editor.scene().clone();
        let before = path_domain(editor.scene());
        let small = path_pixels(&before);
        assert_eq!(pixel(&small, 110, 50), if mask { [0; 4] } else { [255; 4] });
        assert_eq!(pixel(&small, 70, 40), if mask { [255; 4] } else { [0; 4] });
        editor
            .set_property(handle, PropertyKey::Custom("radius".into()), json!(40.))
            .unwrap();
        assert!(
            editor.engine.last_change().dirty[&"custom".into()]
                .contains(DirtyFlags::GEOMETRY | DirtyFlags::HIT | DirtyFlags::PAINT)
        );
        let grown = path_domain(editor.scene());
        assert!(!before.placement_eq(&grown));
        let large = path_pixels(&grown);
        assert_eq!(pixel(&large, 110, 50), if mask { [255; 4] } else { [0; 4] });
        assert_eq!(pixel(&large, 52, 22), if mask { [0; 4] } else { [255; 4] });
        assert_eq!(
            editor.engine.world_bounds(&"custom".into()).unwrap(),
            editor.scene().world_bounds(&"custom".into()).unwrap()
        );
        assert_eq!(
            editor.engine.local_bounds(&"root".into()).unwrap(),
            editor.scene().local_bounds(&"root".into()).unwrap()
        );
        assert!(editor.undo());
        assert_eq!(editor.scene(), &before_scene);
        assert_eq!(editor.engine.handle(&"custom".into()).unwrap(), handle);
        let restored = path_domain(editor.scene());
        assert_eq!(restored, before);
        assert_eq!(path_pixels(&restored).premul_rgba, small.premul_rgba);
        assert!(editor.redo());
        assert_eq!(path_domain(editor.scene()), grown);
        assert_eq!(path_pixels(&grown).premul_rgba, large.premul_rgba);
    }
}
