//! Physical-height ownership fixtures; native axis shaping is tested in flow-text.
use gpui_flow_editor::SceneEditor;
use gpui_flow_layout::{
    AlignItems, AxisSizing, FlowDirection, FlowDocument, FlowItem, FlowLayout, FlowStyle, Insets,
    TextMeasureRequest, TextMeasurer, TextMetrics, TextSizing,
};
use gpui_flow_scene::{
    DisplayObject, Point, Scene, SceneError, Shape, Size,
    display::{Frame, Text, TextAlign, TextVerticalAlign, TextWritingMode},
};
use gpui_flow_text::{WritingMode, WritingModeError};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

#[derive(Default)]
struct AxisMetrics {
    requests: Mutex<Vec<TextMeasureRequest>>,
    fail: AtomicBool,
}
impl TextMeasurer for AxisMetrics {
    fn measure(&self, _: &TextMeasureRequest) -> Result<TextMetrics, String> {
        panic!("vertical measurement must use the axis-aware callback")
    }
    fn measure_writing_mode(
        &self,
        r: &TextMeasureRequest,
        mode: WritingMode,
    ) -> Result<TextMetrics, WritingModeError> {
        self.requests.lock().unwrap().push(r.clone());
        if self.fail.load(Ordering::SeqCst) {
            return Err("fixture backend unavailable".into());
        }
        // Named fixture advances, not a production width/cluster estimator.
        let inline = match r.content.as_str() {
            "" | "\n" => r.font_size,
            "ab\nc" | "ab\n" => r.font_size * 2.,
            "abcdef\n" => r.font_size * 6.,
            _ => panic!("unknown fixture"),
        };
        let extra_columns = if r.wrap_width.is_some_and(|limit| limit < inline) {
            // The only long paragraph fixture is six fixed advances; tests use
            // two or three advances per column, plus its trailing empty column.
            match r.wrap_width {
                Some(40.) => 2,
                Some(60.) => 1,
                _ => panic!("unknown wrapped fixture"),
            }
        } else {
            0
        };
        let lines = r.paragraph_breaks() + 1 + extra_columns;
        let inline = r.wrap_width.map_or(inline, |limit| inline.min(limit));
        let block = r.block_height(lines);
        Ok(TextMetrics {
            width: if mode.is_vertical() { block } else { inline },
            height: if mode.is_vertical() { inline } else { block },
            line_height: r.line_height,
            lines,
        })
    }
}
fn fixture(mode: TextWritingMode) -> (Scene, FlowLayout, Arc<AxisMetrics>) {
    let mut t = Text::new("ab\nc", Size::new(100., 100.));
    t.font_size = 20.;
    t.line_height = 1.;
    t.para_spacing = 7.;
    t.padding = [3., 5., 7., 11.];
    t.wrap = false;
    t.writing_mode = mode;
    t.auto_size_align = true;
    t.align = TextAlign::Center;
    t.vertical_align = TextVerticalAlign::Middle;
    let mut scene = Scene::new();
    scene
        .insert(DisplayObject::new("t", t), None, None)
        .unwrap();
    let mut layout = FlowLayout::default();
    layout.texts.insert("t".into(), TextSizing::AutoHeight);
    (scene, layout, Arc::new(AxisMetrics::default()))
}
fn text(scene: &Scene) -> &Text {
    let Shape::Text(t) = &scene.get(&"t".into()).unwrap().object.shape else {
        panic!()
    };
    t
}
fn edit(scene: &mut Scene, f: impl FnOnce(&mut Text)) -> Result<(), SceneError> {
    scene.update_object(&"t".into(), |object| {
        let Shape::Text(t) = &mut object.shape else {
            panic!()
        };
        f(t);
    })
}
fn with_parent(scene: &mut Scene, layout: &mut FlowLayout, style: FlowStyle) {
    scene
        .insert(DisplayObject::new("p", Frame::new(200., 300.)), None, None)
        .unwrap();
    scene
        .reparent(&"t".into(), Some("p".into()), None, false)
        .unwrap();
    layout.containers.insert("p".into(), style);
}
#[test]
fn only_physical_height_and_y_origin_are_owned_for_both_vertical_modes() {
    for mode in [TextWritingMode::Y, TextWritingMode::YReverse] {
        for (align, expected_y) in [
            (TextAlign::Start, if mode.is_reverse() { -50. } else { 0. }),
            (TextAlign::Center, -23.),
            (TextAlign::End, if mode.is_reverse() { 0. } else { -50. }),
        ] {
            for cross in [
                TextVerticalAlign::Top,
                TextVerticalAlign::Middle,
                TextVerticalAlign::Bottom,
            ] {
                for width in [24., 100.] {
                    let (mut scene, layout, backend) = fixture(mode);
                    edit(&mut scene, |t| {
                        t.align = align;
                        t.vertical_align = cross;
                        t.size.width = width;
                    })
                    .unwrap();
                    let measured = layout.with_text_measurer(backend);
                    measured.apply_to_scene(&mut scene).unwrap();
                    let t = text(&scene);
                    assert_eq!(t.size, Size::new(width, 50.));
                    assert_eq!(t.layout_origin, Point::new(0., expected_y));
                    assert_eq!(t.content_bounds().origin, Point::new(11., expected_y + 3.));
                    assert_eq!(t.content_bounds().size, Size::new(width - 16., 40.));
                    assert!(measured.arranged(&scene).unwrap().1.changed.is_empty());
                    edit(&mut scene, |t| t.auto_size_align = false).unwrap();
                    measured.apply_to_scene(&mut scene).unwrap();
                    assert_eq!(text(&scene).layout_origin, Point::ZERO);
                }
            }
        }
    }
}
#[test]
fn empty_content_and_trailing_paragraphs_keep_width_and_unwrapped_inline_height() {
    for mode in [TextWritingMode::Y, TextWritingMode::YReverse] {
        for (content, height, columns, cross) in [
            ("", 30., 1, 20.),
            ("\n", 30., 2, 47.),
            ("ab\n", 50., 2, 47.),
        ] {
            let (mut scene, layout, backend) = fixture(mode);
            edit(&mut scene, |t| {
                t.content = content.into();
                t.size.width = 24.;
            })
            .unwrap();
            layout
                .with_text_measurer(backend.clone())
                .apply_to_scene(&mut scene)
                .unwrap();
            let t = text(&scene);
            assert_eq!(t.size, Size::new(24., height));
            assert_eq!(t.content, content);
            assert_eq!(t.layout_origin.x, 0.);
            let requests = backend.requests.lock().unwrap();
            assert!(!requests.is_empty());
            for r in requests.iter() {
                assert_eq!(r.paragraph_breaks() + 1, columns);
                assert_eq!(r.block_height(columns), cross);
                assert_eq!(r.wrap_width, None);
            }
        }
    }
}
#[test]
fn parent_width_allocation_reacts_atomically_and_history_never_remeasures() {
    for mode in [TextWritingMode::Y, TextWritingMode::YReverse] {
        let (mut scene, mut layout, backend) = fixture(mode);
        with_parent(
            &mut scene,
            &mut layout,
            FlowStyle {
                direction: FlowDirection::Column,
                align_items: AlignItems::Stretch,
                height_sizing: AxisSizing::content(),
                padding: Insets::all(10.),
                ..Default::default()
            },
        );
        let measured = layout.with_text_measurer(backend.clone());
        measured.apply_to_scene(&mut scene).unwrap();
        assert_eq!(text(&scene).size, Size::new(180., 50.));
        assert_eq!(
            scene
                .get(&"p".into())
                .unwrap()
                .object
                .shape
                .local_bounds()
                .unwrap()
                .size
                .height,
            70.
        );
        let mut editor = SceneEditor::new(scene.clone()).unwrap();
        measured.install_reactive(&mut editor.engine).unwrap();
        editor
            .transaction(|s| {
                s.update_object(&"p".into(), |o| {
                    let Shape::Frame(p) = &mut o.shape else {
                        panic!()
                    };
                    p.size.width = 300.;
                })?;
                edit(s, |t| t.content = "abcdef\n".into())
            })
            .unwrap();
        assert_eq!(text(editor.scene()).size, Size::new(280., 130.));
        assert_eq!(text(editor.scene()).layout_origin, Point::new(0., -63.));
        assert_eq!(
            editor
                .scene()
                .get(&"p".into())
                .unwrap()
                .object
                .shape
                .local_bounds()
                .unwrap()
                .size
                .height,
            150.
        );
        let after = editor.scene().clone();
        let revision = editor.engine.revision();
        assert!(
            measured
                .apply_to_editor(&mut editor)
                .unwrap()
                .changed
                .is_empty()
        );
        assert_eq!(editor.engine.revision(), revision);
        let calls = backend.requests.lock().unwrap().len();
        backend.fail.store(true, Ordering::SeqCst);
        assert!(editor.undo());
        assert_eq!(editor.scene(), &scene);
        assert!(!editor.undo());
        assert!(editor.redo());
        assert_eq!(editor.scene(), &after);
        assert_eq!(backend.requests.lock().unwrap().len(), calls);
        backend.fail.store(false, Ordering::SeqCst);
        assert!(
            measured
                .arranged(editor.scene())
                .unwrap()
                .1
                .changed
                .is_empty()
        );
    }
}
#[test]
fn every_parent_height_owner_is_rejected_before_scene_revision_or_history_changes() {
    let cases = [
        (FlowDirection::Row, AlignItems::Stretch, FlowItem::default()),
        (
            FlowDirection::Column,
            AlignItems::Start,
            FlowItem {
                grow: 1.,
                ..Default::default()
            },
        ),
        (
            FlowDirection::Column,
            AlignItems::Start,
            FlowItem {
                basis: Some(90.),
                ..Default::default()
            },
        ),
        (
            FlowDirection::Row,
            AlignItems::Start,
            FlowItem {
                min_height: Some(40.),
                ..Default::default()
            },
        ),
        (
            FlowDirection::Row,
            AlignItems::Start,
            FlowItem {
                max_height: Some(90.),
                ..Default::default()
            },
        ),
        (
            FlowDirection::Row,
            AlignItems::Start,
            FlowItem {
                height_percent: Some(0.5),
                ..Default::default()
            },
        ),
    ];
    for mode in [TextWritingMode::Y, TextWritingMode::YReverse] {
        for (direction, align_items, item) in &cases {
            let (mut scene, mut layout, backend) = fixture(mode);
            with_parent(
                &mut scene,
                &mut layout,
                FlowStyle {
                    direction: *direction,
                    align_items: *align_items,
                    ..Default::default()
                },
            );
            layout.items.insert("t".into(), item.clone());
            let mut editor = SceneEditor::new(scene.clone()).unwrap();
            let revision = editor.engine.revision();
            let measured = layout.with_text_measurer(backend.clone());
            let error = measured
                .transaction_editor(&mut editor, |s| edit(s, |t| t.content = "abcdef\n".into()))
                .unwrap_err();
            assert!(
                error.to_string().contains("also allocated by its parent"),
                "{error}"
            );
            assert_eq!(editor.scene(), &scene);
            assert_eq!(editor.engine.revision(), revision);
            assert!(!editor.undo());
            assert!(backend.requests.lock().unwrap().is_empty());
        }
    }
}
#[test]
fn wrap_changes_and_legacy_backends_remain_explicit_atomic_failures() {
    struct Legacy;
    impl TextMeasurer for Legacy {
        fn measure(&self, _: &TextMeasureRequest) -> Result<TextMetrics, String> {
            panic!("legacy backend cannot measure vertical text")
        }
    }
    for mode in [TextWritingMode::Y, TextWritingMode::YReverse] {
        let (mut scene, layout, backend) = fixture(mode);
        let mut legacy_editor = SceneEditor::new(scene.clone()).unwrap();
        let revision = legacy_editor.engine.revision();
        assert!(
            layout
                .clone()
                .with_text_measurer(Arc::new(Legacy))
                .apply_to_editor(&mut legacy_editor)
                .unwrap_err()
                .to_string()
                .contains("unsupported")
        );
        assert_eq!(legacy_editor.scene(), &scene);
        assert_eq!(legacy_editor.engine.revision(), revision);
        assert!(!legacy_editor.undo());
        let measured = layout.with_text_measurer(backend.clone());
        measured.apply_to_scene(&mut scene).unwrap();
        let mut editor = SceneEditor::new(scene.clone()).unwrap();
        measured.install_reactive(&mut editor.engine).unwrap();
        let revision = editor.engine.revision();
        let calls = backend.requests.lock().unwrap().len();
        let error = editor
            .transaction(|s| {
                edit(s, |t| {
                    t.wrap = true;
                    t.content = "abcdef\n".into();
                })
            })
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("vertical AutoHeight requires wrap=false")
        );
        assert_eq!(editor.scene(), &scene);
        assert_eq!(editor.engine.revision(), revision);
        assert!(!editor.undo());
        assert_eq!(backend.requests.lock().unwrap().len(), calls);
    }
}

#[test]
fn autowidth_owns_only_physical_width_and_x_origin_in_every_writing_mode() {
    for mode in [
        TextWritingMode::X,
        TextWritingMode::XReverse,
        TextWritingMode::Y,
        TextWritingMode::YReverse,
    ] {
        for wrap in [false, true] {
            if wrap && !mode.is_vertical() {
                continue;
            }
            for align in [TextAlign::Start, TextAlign::Center, TextAlign::End] {
                for cross in [
                    TextVerticalAlign::Top,
                    TextVerticalAlign::Middle,
                    TextVerticalAlign::Bottom,
                ] {
                    let (mut scene, mut layout, backend) = fixture(mode);
                    layout.texts.insert("t".into(), TextSizing::AutoWidth);
                    edit(&mut scene, |t| {
                        t.wrap = wrap;
                        t.align = align;
                        t.vertical_align = cross;
                    })
                    .unwrap();
                    let measured = layout.with_text_measurer(backend.clone());
                    measured.apply_to_scene(&mut scene).unwrap();
                    let width = if mode.is_vertical() { 63. } else { 56. };
                    let x = if mode.is_vertical() {
                        match cross {
                            TextVerticalAlign::Top => 0.,
                            TextVerticalAlign::Middle => -34.5,
                            TextVerticalAlign::Bottom => -63.,
                        }
                    } else {
                        match align {
                            TextAlign::Center => -31.,
                            TextAlign::Start if !mode.is_reverse() => 0.,
                            TextAlign::End if mode.is_reverse() => 0.,
                            _ => -56.,
                        }
                    };
                    assert_eq!(text(&scene).size, Size::new(width, 100.));
                    assert_eq!(text(&scene).layout_origin, Point::new(x, 0.));
                    assert_eq!(
                        text(&scene).content_bounds().origin,
                        Point::new(x + 11., 3.)
                    );
                    assert!(
                        backend
                            .requests
                            .lock()
                            .unwrap()
                            .iter()
                            .all(|r| r.wrap_width == wrap.then_some(90.))
                    );
                    assert!(measured.arranged(&scene).unwrap().1.changed.is_empty());
                    edit(&mut scene, |t| t.auto_size_align = false).unwrap();
                    measured.apply_to_scene(&mut scene).unwrap();
                    assert_eq!(text(&scene).layout_origin, Point::ZERO);
                }
            }
        }
    }
}
#[test]
fn autowidth_parent_height_rewraps_reactively_and_roundtrips_one_history_step() {
    for mode in [TextWritingMode::Y, TextWritingMode::YReverse] {
        let (mut scene, mut layout, backend) = fixture(mode);
        layout.texts.insert("t".into(), TextSizing::AutoWidth);
        edit(&mut scene, |t| t.wrap = true).unwrap();
        with_parent(
            &mut scene,
            &mut layout,
            FlowStyle {
                direction: FlowDirection::Row,
                align_items: AlignItems::Stretch,
                width_sizing: AxisSizing::content(),
                padding: Insets::all(10.),
                ..Default::default()
            },
        );
        scene
            .update_object(&"p".into(), |o| {
                let Shape::Frame(p) = &mut o.shape else {
                    panic!()
                };
                p.size.height = 70.;
            })
            .unwrap();
        let measured = layout.clone().with_text_measurer(backend.clone());
        measured.apply_to_scene(&mut scene).unwrap();
        assert_eq!(text(&scene).size, Size::new(63., 50.));
        let document = FlowDocument {
            scene: scene.clone(),
            layout,
        };
        let json = document.to_json().unwrap();
        assert!(json.contains("auto_width"));
        assert_eq!(FlowDocument::from_json(&json).unwrap(), document);
        let mut editor = SceneEditor::new(scene.clone()).unwrap();
        measured.install_reactive(&mut editor.engine).unwrap();
        editor
            .transaction(|s| {
                s.update_object(&"p".into(), |o| {
                    let Shape::Frame(p) = &mut o.shape else {
                        panic!()
                    };
                    p.size.height = 90.;
                })?;
                edit(s, |t| t.content = "abcdef\n".into())
            })
            .unwrap();
        assert_eq!(text(editor.scene()).size, Size::new(83., 70.));
        assert_eq!(text(editor.scene()).layout_origin, Point::new(-44.5, 0.));
        let Shape::Frame(p) = &editor.scene().get(&"p".into()).unwrap().object.shape else {
            panic!()
        };
        assert_eq!(p.size, Size::new(103., 90.));
        assert!(
            backend
                .requests
                .lock()
                .unwrap()
                .iter()
                .any(|r| r.wrap_width == Some(60.))
        );
        let after = editor.scene().clone();
        let revision = editor.engine.revision();
        let error = editor
            .transaction(|s| edit(s, |t| t.writing_mode = TextWritingMode::X))
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("horizontal AutoWidth requires wrap=false")
        );
        assert_eq!(editor.scene(), &after);
        assert_eq!(editor.engine.revision(), revision);
        backend.fail.store(true, Ordering::SeqCst);
        assert!(
            editor
                .transaction(|s| edit(s, |t| t.content = "".into()))
                .is_err()
        );
        assert_eq!(editor.scene(), &after);
        assert_eq!(editor.engine.revision(), revision);
        let calls = backend.requests.lock().unwrap().len();
        assert!(editor.undo());
        assert_eq!(editor.scene(), &scene);
        assert!(!editor.undo());
        assert!(editor.redo());
        assert_eq!(editor.scene(), &after);
        assert_eq!(backend.requests.lock().unwrap().len(), calls);
        backend.fail.store(false, Ordering::SeqCst);
        assert!(
            measured
                .arranged(editor.scene())
                .unwrap()
                .1
                .changed
                .is_empty()
        );
    }
}
#[test]
fn autowidth_all_parent_width_owners_and_height_cycles_fail_atomically() {
    let cases = [
        (
            FlowDirection::Column,
            AlignItems::Stretch,
            FlowItem::default(),
        ),
        (
            FlowDirection::Row,
            AlignItems::Start,
            FlowItem {
                grow: 1.,
                ..Default::default()
            },
        ),
        (
            FlowDirection::Row,
            AlignItems::Start,
            FlowItem {
                basis: Some(90.),
                ..Default::default()
            },
        ),
        (
            FlowDirection::Row,
            AlignItems::Start,
            FlowItem {
                min_width: Some(40.),
                ..Default::default()
            },
        ),
        (
            FlowDirection::Row,
            AlignItems::Start,
            FlowItem {
                max_width: Some(90.),
                ..Default::default()
            },
        ),
        (
            FlowDirection::Row,
            AlignItems::Start,
            FlowItem {
                width_percent: Some(0.5),
                ..Default::default()
            },
        ),
    ];
    for mode in [
        TextWritingMode::X,
        TextWritingMode::XReverse,
        TextWritingMode::Y,
        TextWritingMode::YReverse,
    ] {
        for (direction, align_items, item) in &cases {
            let (mut scene, mut layout, backend) = fixture(mode);
            layout.texts.insert("t".into(), TextSizing::AutoWidth);
            with_parent(
                &mut scene,
                &mut layout,
                FlowStyle {
                    direction: *direction,
                    align_items: *align_items,
                    ..Default::default()
                },
            );
            layout.items.insert("t".into(), item.clone());
            let mut editor = SceneEditor::new(scene.clone()).unwrap();
            let revision = editor.engine.revision();
            assert!(
                layout
                    .with_text_measurer(backend.clone())
                    .apply_to_editor(&mut editor)
                    .unwrap_err()
                    .to_string()
                    .contains("also allocated by its parent")
            );
            assert_eq!(editor.scene(), &scene);
            assert_eq!(editor.engine.revision(), revision);
            assert!(!editor.undo());
            assert!(backend.requests.lock().unwrap().is_empty());
        }
    }
    for (direction, align_items, item) in [
        (FlowDirection::Row, AlignItems::Stretch, FlowItem::default()),
        (
            FlowDirection::Column,
            AlignItems::Start,
            FlowItem {
                grow: 1.,
                ..Default::default()
            },
        ),
        (
            FlowDirection::Row,
            AlignItems::Start,
            FlowItem {
                height_percent: Some(0.5),
                ..Default::default()
            },
        ),
    ] {
        let (mut scene, mut layout, backend) = fixture(TextWritingMode::Y);
        layout.texts.insert("t".into(), TextSizing::AutoWidth);
        edit(&mut scene, |t| t.wrap = true).unwrap();
        with_parent(
            &mut scene,
            &mut layout,
            FlowStyle {
                direction,
                align_items,
                height_sizing: AxisSizing::content(),
                ..Default::default()
            },
        );
        layout.items.insert("t".into(), item);
        let mut editor = SceneEditor::new(scene.clone()).unwrap();
        let revision = editor.engine.revision();
        assert!(
            layout
                .with_text_measurer(backend.clone())
                .apply_to_editor(&mut editor)
                .is_err()
        );
        assert_eq!(editor.scene(), &scene);
        assert_eq!(editor.engine.revision(), revision);
        assert!(!editor.undo());
        assert!(backend.requests.lock().unwrap().is_empty());
    }
}

#[test]
fn autowidth_accepts_every_parent_height_owner_and_uses_inner_allocated_height() {
    let cases = [
        (
            FlowDirection::RowReverse,
            AlignItems::Stretch,
            FlowItem::default(),
            300.,
        ),
        (
            FlowDirection::ColumnReverse,
            AlignItems::Start,
            FlowItem {
                grow: 1.,
                ..Default::default()
            },
            300.,
        ),
        (
            FlowDirection::Column,
            AlignItems::Start,
            FlowItem {
                basis: Some(70.),
                ..Default::default()
            },
            70.,
        ),
        (
            FlowDirection::Row,
            AlignItems::Start,
            FlowItem {
                min_height: Some(120.),
                ..Default::default()
            },
            120.,
        ),
        (
            FlowDirection::Row,
            AlignItems::Start,
            FlowItem {
                max_height: Some(50.),
                ..Default::default()
            },
            50.,
        ),
        (
            FlowDirection::Row,
            AlignItems::Start,
            FlowItem {
                height_percent: Some(0.5),
                ..Default::default()
            },
            150.,
        ),
    ];
    for mode in [TextWritingMode::Y, TextWritingMode::YReverse] {
        for (direction, align_items, item, height) in &cases {
            let (mut scene, mut layout, backend) = fixture(mode);
            layout.texts.insert("t".into(), TextSizing::AutoWidth);
            edit(&mut scene, |t| t.wrap = true).unwrap();
            with_parent(
                &mut scene,
                &mut layout,
                FlowStyle {
                    direction: *direction,
                    align_items: *align_items,
                    ..Default::default()
                },
            );
            layout.items.insert("t".into(), item.clone());
            let measured = layout.with_text_measurer(backend.clone());
            measured.apply_to_scene(&mut scene).unwrap();
            assert_eq!(text(&scene).size, Size::new(63., *height));
            assert_eq!(
                backend.requests.lock().unwrap().last().unwrap().wrap_width,
                Some(height - 10.)
            );
            assert!(measured.arranged(&scene).unwrap().1.changed.is_empty());
        }
    }
}
