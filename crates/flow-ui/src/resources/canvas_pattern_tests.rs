//! Pattern handles are immutable external snapshots; Canvas resource lifetimes
//! and document history still own only the target surface/configuration.
use super::*;
use gpui_flow_composite::{
    CanvasCommand, CanvasPathTarget, CanvasPattern, CanvasPatternRepeat, CanvasStrokeOptions,
    CanvasSurface, ImageInputs, StaticImage,
};
use gpui_flow_engine::PropertyKey;
use gpui_flow_scene::{Affine, Color, DisplayObject, FillRule, Point, Scene, Size, display};

fn setup() -> (SceneEngine, SceneResourceAdapter, ElementHandle) {
    let mut scene = Scene::new();
    let mut config = display::Canvas::new(12., 10.);
    config.smooth = false;
    scene
        .insert(DisplayObject::new("c", config), None, None)
        .unwrap();
    let mut engine = SceneEngine::new(scene).unwrap();
    let handle = engine.handle(&"c".into()).unwrap();
    let resources = SceneResourceAdapter::new(&mut engine).unwrap();
    (engine, resources, handle)
}

fn source(color: [u8; 4]) -> ImageInputs {
    let image =
        StaticImage::from_premultiplied_rgba(2, 2, color.repeat(4), Default::default()).unwrap();
    let mut images = ImageInputs::default();
    images.insert("tile".into(), Ok(Arc::new(image)));
    images
}

fn pattern(images: &ImageInputs) -> CanvasPattern {
    CanvasSurface::new(display::Canvas::new(12., 10.), Default::default())
        .unwrap()
        .create_pattern(&"tile".into(), images, CanvasPatternRepeat::Repeat)
        .unwrap()
}

fn fill(pattern: &CanvasPattern, current: bool) -> CanvasCommand {
    CanvasCommand::FillPattern {
        path: if current {
            CanvasPathTarget::Current
        } else {
            CanvasPathTarget::Explicit(
                display::Path::from_svg("M 0 0 L 12 0 L 12 10 L 0 10 Z").unwrap(),
            )
        },
        pattern: pattern.clone(),
        rule: FillRule::NonZero,
    }
}

fn current_path() -> [CanvasCommand; 6] {
    [
        CanvasCommand::BeginPath,
        CanvasCommand::MoveTo {
            to: Point::new(1., 1.),
        },
        CanvasCommand::LineTo {
            to: Point::new(7., 1.),
        },
        CanvasCommand::LineTo {
            to: Point::new(7., 6.),
        },
        CanvasCommand::LineTo {
            to: Point::new(1., 6.),
        },
        CanvasCommand::ClosePath,
    ]
}

fn at(
    resources: &SceneResourceAdapter,
    engine: &SceneEngine,
    handle: ElementHandle,
    x: u32,
    y: u32,
) -> [u8; 4] {
    let pixels = resources.canvas_snapshot(engine, handle).unwrap();
    pixels.premultiplied_rgba()[((y * pixels.width() + x) * 4) as usize..][..4]
        .try_into()
        .unwrap()
}

#[test]
fn canvas_patterns_publish_runtime_pixels_without_history_and_capture_source_replacement() {
    let (engine, mut resources, handle) = setup();
    let mut editor = gpui_flow_editor::SceneEditor::from_engine(engine);
    resources.prepare_canvases(&mut editor.engine).unwrap();
    let scene = editor.scene().clone();
    let revision = editor.engine.revision();
    let ticket = resources.tracker.ticket(&editor.engine, handle).unwrap();
    let (upload, generation) = resources.canvases.image(ticket).unwrap();
    let before = resources.canvas_snapshot(&editor.engine, handle).unwrap();
    let mut inputs = source([230, 30, 15, 255]);
    let captured = pattern(&inputs);
    // Replacing the same source ID cannot change an existing pattern handle.
    inputs = source([20, 210, 50, 255]);
    resources
        .edit_canvas(&mut editor.engine, handle, |surface| {
            surface.draw_commands_with_images(&[fill(&captured, false)], &inputs)?;
            Ok(())
        })
        .unwrap();
    let (after_upload, after_generation) = resources.canvases.image(ticket).unwrap();
    assert!(!Arc::ptr_eq(&upload, &after_upload));
    assert!(!Arc::ptr_eq(
        &before,
        &resources.canvas_snapshot(&editor.engine, handle).unwrap()
    ));
    assert_eq!(after_generation, generation + 1);
    assert_eq!(
        at(&resources, &editor.engine, handle, 2, 2),
        [230, 30, 15, 255]
    );
    assert_eq!(editor.scene(), &scene);
    assert_eq!(editor.engine.revision(), revision);
    assert!(!editor.undo());
    assert!(resources.images.is_empty());
    let serialized = serde_json::to_string(editor.scene()).unwrap();
    assert!(!serialized.contains("pattern"));
    assert!(!serialized.contains("pixels"));
    let recaptured = pattern(&inputs);
    resources
        .edit_canvas(&mut editor.engine, handle, |surface| {
            surface
                .draw_commands(&[fill(&recaptured, false)])
                .map(|_| ())
        })
        .unwrap();
    assert_eq!(
        at(&resources, &editor.engine, handle, 2, 2),
        [20, 210, 50, 255]
    );
}

#[test]
fn canvas_patterns_late_rejection_preserves_path_pixels_generation_and_upload() {
    for invalid_stroke in [false, true] {
        let (mut engine, mut resources, handle) = setup();
        let captured = pattern(&source([255, 255, 255, 255]));
        resources
            .edit_canvas(&mut engine, handle, |surface| {
                surface.draw_commands(&current_path()).map(|_| ())
            })
            .unwrap();
        let ticket = resources.tracker.ticket(&engine, handle).unwrap();
        let (upload, generation) = resources.canvases.image(ticket).unwrap();
        let pixels = resources.canvas_snapshot(&engine, handle).unwrap();
        let scene = engine.scene().clone();
        let result = resources.edit_canvas(&mut engine, handle, |surface| {
            let mut commands = vec![
                fill(&captured, true),
                CanvasCommand::BeginPath,
                CanvasCommand::MoveTo {
                    to: Point::new(8., 1.),
                },
                CanvasCommand::LineTo {
                    to: Point::new(11., 1.),
                },
                CanvasCommand::LineTo {
                    to: Point::new(11., 9.),
                },
                CanvasCommand::ClosePath,
                fill(&captured, true),
            ];
            commands.push(if invalid_stroke {
                CanvasCommand::StrokePattern {
                    path: CanvasPathTarget::Current,
                    pattern: captured.clone(),
                    options: CanvasStrokeOptions {
                        width: f32::NAN,
                        ..Default::default()
                    },
                }
            } else {
                CanvasCommand::Restore
            });
            surface.draw_commands(&commands).map(|_| ())
        });
        assert!(result.is_err());
        let (after_upload, after_generation) = resources.canvases.image(ticket).unwrap();
        assert!(Arc::ptr_eq(&upload, &after_upload));
        assert_eq!(generation, after_generation);
        assert!(Arc::ptr_eq(
            &pixels,
            &resources.canvas_snapshot(&engine, handle).unwrap()
        ));
        assert_eq!(engine.scene(), &scene);
        resources
            .edit_canvas(&mut engine, handle, |surface| {
                surface.draw_commands(&[fill(&captured, true)]).map(|_| ())
            })
            .unwrap();
        assert_eq!(at(&resources, &engine, handle, 2, 2), [255; 4]);
        assert_eq!(at(&resources, &engine, handle, 9, 2), [0; 4]);
    }
}

#[test]
fn canvas_pattern_snapshot_survives_source_canvas_detach_retype_destroy_and_dispose() {
    for boundary in 0..4 {
        let (mut engine, mut resources, handle) = setup();
        resources
            .edit_canvas(&mut engine, handle, |surface| {
                surface.fill_rect(
                    gpui_flow_scene::Bounds::new(Point::ZERO, Size::new(12., 10.)),
                    Color::hex(0xeb4628),
                )
            })
            .unwrap();
        let snapshot = resources.canvas_snapshot(&engine, handle).unwrap();
        let source_weak = Arc::downgrade(&snapshot);
        let mut images = ImageInputs::default();
        images.insert("tile".into(), Ok(snapshot));
        let captured = pattern(&images);
        drop(images);
        // Clearing publishes a new source snapshot. The captured Arc remains red.
        resources
            .edit_canvas(&mut engine, handle, |surface| surface.clear())
            .unwrap();
        match boundary {
            0 => {
                engine.detach(handle).unwrap();
            }
            1 => {
                engine
                    .set_property(
                        handle,
                        PropertyKey::Shape,
                        serde_json::json!(Shape::from(display::Rect::new(12., 10.))),
                    )
                    .unwrap();
            }
            2 => {
                engine.destroy(handle).unwrap();
            }
            _ => {
                resources.dispose(&mut engine).unwrap();
            }
        }
        assert!(
            source_weak.upgrade().is_some(),
            "pattern owns its immutable source"
        );
        let mut target =
            CanvasSurface::new(display::Canvas::new(12., 10.), Default::default()).unwrap();
        target.draw_commands(&[fill(&captured, false)]).unwrap();
        assert_eq!(
            &target.snapshot().premultiplied_rgba()[0..4],
            &[235, 70, 40, 255]
        );
        drop(captured);
        assert!(
            source_weak.upgrade().is_none(),
            "target pixels do not retain pattern input"
        );
    }
}

#[test]
fn canvas_patterns_reject_detached_and_stale_handles_without_restoring_target_state() {
    for boundary in 0..3 {
        let (mut engine, mut resources, handle) = setup();
        let captured = pattern(&source([255; 4]));
        resources
            .edit_canvas(&mut engine, handle, |surface| {
                surface.draw_commands(&current_path())?;
                surface.draw_commands(&[fill(&captured, true)]).map(|_| ())
            })
            .unwrap();
        let old_pixels = resources.canvas_snapshot(&engine, handle).unwrap();
        let weak = Arc::downgrade(&old_pixels);
        drop(old_pixels);
        let next_handle = match boundary {
            0 => {
                let (detached, _) = engine.detach(handle).unwrap();
                assert!(
                    resources
                        .edit_canvas(&mut engine, handle, |surface| {
                            surface.draw_commands(&[fill(&captured, false)]).map(|_| ())
                        })
                        .is_err()
                );
                assert!(resources.canvas_snapshot(&engine, handle).is_none());
                engine.attach(&detached, None, None).unwrap();
                handle
            }
            1 => {
                engine
                    .set_property(
                        handle,
                        PropertyKey::Shape,
                        serde_json::json!(Shape::from(display::Rect::new(12., 10.))),
                    )
                    .unwrap();
                let rect = engine.handle(&"c".into()).unwrap();
                engine
                    .set_property(
                        rect,
                        PropertyKey::Shape,
                        serde_json::json!(Shape::from(display::Canvas::new(12., 10.))),
                    )
                    .unwrap();
                assert!(
                    resources
                        .edit_canvas(&mut engine, handle, |_| Ok(()))
                        .is_err()
                );
                engine.handle(&"c".into()).unwrap()
            }
            _ => {
                engine.destroy(handle).unwrap();
                engine
                    .create("canvas", "c", serde_json::json!({}), None, None)
                    .unwrap();
                assert!(
                    resources
                        .edit_canvas(&mut engine, handle, |_| Ok(()))
                        .is_err()
                );
                engine.handle(&"c".into()).unwrap()
            }
        };
        assert!(
            weak.upgrade().is_none(),
            "target ownership ends at the lifecycle boundary"
        );
        resources
            .edit_canvas(&mut engine, next_handle, |surface| {
                assert_eq!(surface.current_path_byte_len(), 0);
                assert!(!surface.draw_commands(&[fill(&captured, true)])?);
                assert!(
                    surface
                        .snapshot()
                        .premultiplied_rgba()
                        .iter()
                        .all(|byte| *byte == 0)
                );
                surface.draw_commands(&[fill(&captured, false)]).map(|_| ())
            })
            .unwrap();
        assert_eq!(at(&resources, &engine, next_handle, 2, 2), [255; 4]);
    }
}

#[test]
fn canvas_patterns_survive_size_and_dpr_boundaries_while_current_geometry_resets() {
    for dpr in [false, true] {
        for safe in [false, true] {
            let (mut engine, mut resources, handle) = setup();
            let captured = pattern(&source([255; 4]));
            resources
                .edit_canvas(&mut engine, handle, |surface| {
                    surface.draw_commands(&current_path())?;
                    surface.draw_commands(&[fill(&captured, false)]).map(|_| ())
                })
                .unwrap();
            let mut changed = display::Canvas::new(12., 10.);
            changed.smooth = false;
            changed.safe_resize = safe;
            if dpr {
                changed.pixel_ratio = 2.;
            } else {
                changed.size = Size::new(6., 6.);
            }
            engine
                .set_property(
                    handle,
                    PropertyKey::Shape,
                    serde_json::json!(Shape::from(changed)),
                )
                .unwrap();
            resources.prepare_canvases(&mut engine).unwrap();
            let before_empty_draw = resources.canvas_snapshot(&engine, handle).unwrap();
            assert_eq!(
                at(&resources, &engine, handle, 2, 2),
                if safe { [255; 4] } else { [0; 4] }
            );
            if dpr {
                assert_eq!(at(&resources, &engine, handle, 16, 2), [0; 4]);
            }
            resources
                .edit_canvas(&mut engine, handle, |surface| {
                    assert_eq!(surface.current_path_byte_len(), 0);
                    assert!(!surface.draw_commands(&[fill(&captured, true)])?);
                    Ok(())
                })
                .unwrap();
            assert!(Arc::ptr_eq(
                &before_empty_draw,
                &resources.canvas_snapshot(&engine, handle).unwrap()
            ));
            resources
                .edit_canvas(&mut engine, handle, |surface| {
                    surface.draw_commands(&[fill(&captured, false)]).map(|_| ())
                })
                .unwrap();
            assert_eq!(at(&resources, &engine, handle, 2, 2), [255; 4]);
            if dpr {
                assert_eq!(at(&resources, &engine, handle, 16, 2), [255; 4]);
            }
        }
    }
}

#[test]
fn canvas_pattern_configuration_undo_never_replays_pixels_paths_or_pattern_handles() {
    let (engine, mut resources, handle) = setup();
    let mut editor = gpui_flow_editor::SceneEditor::from_engine(engine);
    let mut captured = pattern(&source([255; 4]));
    captured.set_transform(Affine::translation(2., 1.)).unwrap();
    resources
        .edit_canvas(&mut editor.engine, handle, |surface| {
            surface.draw_commands(&current_path())?;
            surface.draw_commands(&[fill(&captured, true)]).map(|_| ())
        })
        .unwrap();
    assert!(!editor.undo());
    editor
        .set_property(
            handle,
            PropertyKey::Shape,
            serde_json::json!(Shape::from(display::Canvas::new(20., 14.))),
        )
        .unwrap();
    resources.prepare_canvases(&mut editor.engine).unwrap();
    assert!(editor.undo());
    resources
        .edit_canvas(&mut editor.engine, handle, |surface| {
            assert_eq!(surface.config().size, Size::new(12., 10.));
            assert_eq!(surface.current_path_byte_len(), 0);
            assert!(!surface.draw_commands(&[fill(&captured, true)])?);
            assert!(
                surface
                    .snapshot()
                    .premultiplied_rgba()
                    .iter()
                    .all(|byte| *byte == 0)
            );
            surface.draw_commands(&[fill(&captured, false)]).map(|_| ())
        })
        .unwrap();
    assert_eq!(at(&resources, &editor.engine, handle, 2, 2), [255; 4]);
    assert!(editor.redo());
    resources
        .edit_canvas(&mut editor.engine, handle, |surface| {
            assert_eq!(surface.current_path_byte_len(), 0);
            assert!(
                surface
                    .snapshot()
                    .premultiplied_rgba()
                    .iter()
                    .all(|byte| *byte == 0)
            );
            surface.draw_commands(&[fill(&captured, false)]).map(|_| ())
        })
        .unwrap();
}
