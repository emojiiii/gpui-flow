use gpui_flow_core::{Point, Rect, Size};
use gpui_flow_editor::SceneEditor;
use gpui_flow_interaction::{
    scene::{plugins::*, *},
    *,
};
use gpui_flow_scene::{Affine, Color, DisplayObject, HitPolicy, Scene, Style, display as d};
use serde_json::json;
fn editor() -> SceneEditor {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("a", d::Rect::new(100.0, 80.0))
                .with_transform(Affine::translation(30.0, 20.0)),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("b", d::Rect::new(100.0, 80.0))
                .with_transform(Affine::translation(250.0, 20.0)),
            None,
            None,
        )
        .unwrap();
    SceneEditor::new(scene).unwrap()
}
fn input(x: f32, y: f32, hit: HitTarget) -> PointerInput {
    PointerInput::left(Point::new(x, y), hit)
}
fn down(
    host: &mut ScenePluginHost,
    editor: &mut SceneEditor,
    x: f32,
    y: f32,
    hit: HitTarget,
) -> DispatchReport {
    host.dispatch(editor, CanvasEvent::PointerDown(input(x, y, hit)))
}
fn movement(
    host: &mut ScenePluginHost,
    editor: &mut SceneEditor,
    x: f32,
    y: f32,
) -> DispatchReport {
    host.dispatch(
        editor,
        CanvasEvent::PointerMove(input(x, y, HitTarget::Background)),
    )
}
fn up(host: &mut ScenePluginHost, editor: &mut SceneEditor, x: f32, y: f32) -> DispatchReport {
    host.dispatch(
        editor,
        CanvasEvent::PointerUp(input(x, y, HitTarget::Background)),
    )
}
fn begin_move(host: &mut ScenePluginHost, editor: &mut SceneEditor) {
    down(host, editor, 50.0, 40.0, HitTarget::Object("a".into()));
    movement(host, editor, 90.0, 75.0);
}
#[test]
fn shared_trait_drives_real_scene_selection_drag_and_undo() {
    let mut host = ScenePluginHost::with_defaults();
    let mut editor = editor();
    begin_move(&mut host, &mut editor);
    assert_eq!(host.capture_owner(), Some("drag"));
    assert!(editor.editing());
    assert_eq!(
        editor.scene.get(&"a".into()).unwrap().object.transform.tx,
        70.0
    );
    up(&mut host, &mut editor, 90.0, 75.0);
    assert!(!editor.editing());
    assert!(editor.undo());
    assert_eq!(
        editor.scene.get(&"a".into()).unwrap().object.transform.tx,
        30.0
    );
}
#[test]
fn disabling_scene_selection_does_not_move_unrelated_selection() {
    let mut host = ScenePluginHost::with_defaults();
    let mut editor = editor();
    editor.selection.select("b".into(), false);
    host.set_enabled("selection", false, &mut editor).unwrap();
    begin_move(&mut host, &mut editor);
    assert!(!host.has_capture());
    assert_eq!(
        editor.scene.get(&"b".into()).unwrap().object.transform.tx,
        250.0
    );
}
#[test]
fn uninstall_drag_rolls_back_scene_transaction() {
    let mut host = ScenePluginHost::with_defaults();
    let mut editor = editor();
    let before = editor.scene.clone();
    begin_move(&mut host, &mut editor);
    let result = host.uninstall("drag", &mut editor).unwrap();
    assert!(result.document_changed);
    assert_eq!(editor.scene, before);
    assert!(!editor.editing());
    assert!(!host.has_capture());
    assert!(!editor.undo());
}
#[test]
fn readonly_midgesture_cancels_and_blocks_scene_mutations() {
    let mut host = ScenePluginHost::with_defaults();
    let mut editor = editor();
    let before = editor.scene.clone();
    begin_move(&mut host, &mut editor);
    host.set_editable(false, &mut editor);
    assert_eq!(editor.scene, before);
    let commands = vec![
        SceneCommand::Undo,
        SceneCommand::Redo,
        SceneCommand::DeleteSelection,
        SceneCommand::SetObjectData {
            id: "a".into(),
            data: json!({"blocked":true}),
        },
        SceneCommand::PasteScene {
            scene: Scene::new(),
            offset: Point::ZERO,
        },
        SceneCommand::LoadScene(Scene::new()),
        SceneCommand::ImportScene(Scene::new()),
        SceneCommand::SetTransform {
            id: "a".into(),
            transform: Affine::scale(2.0, 2.0),
        },
    ];
    for command in commands {
        assert!(matches!(
            host.execute(&mut editor, command),
            Err(InteractionError::CapabilityDenied(Capability::Edit))
        ));
    }
    assert_eq!(editor.scene, before);
}
#[test]
fn scene_navigation_and_selection_policies_are_independent() {
    let mut host = ScenePluginHost::with_defaults();
    let mut editor = editor();
    host.set_policy(
        AccessPolicy {
            selection: false,
            ..AccessPolicy::read_only()
        },
        &mut editor,
    );
    down(
        &mut host,
        &mut editor,
        50.0,
        40.0,
        HitTarget::Object("a".into()),
    );
    assert!(editor.selection.ids.is_empty());
    host.execute(
        &mut editor,
        SceneCommand::Pan {
            delta: Point::new(30.0, 20.0),
        },
    )
    .unwrap();
    assert_eq!(editor.viewport.pan, Point::new(30.0, 20.0));
    host.set_policy(
        AccessPolicy {
            navigation: false,
            ..AccessPolicy::read_only()
        },
        &mut editor,
    );
    down(
        &mut host,
        &mut editor,
        50.0,
        40.0,
        HitTarget::Object("a".into()),
    );
    assert!(editor.selection.ids.contains(&"a".into()));
    assert!(
        host.execute(&mut editor, SceneCommand::Pan { delta: Point::ZERO })
            .is_err()
    );
}
#[test]
fn scene_missing_button_and_focus_loss_roll_back_not_commit() {
    for event in [
        CanvasEvent::PointerMove(PointerInput {
            position: Point::new(100.0, 100.0),
            ..Default::default()
        }),
        CanvasEvent::FocusLost,
        CanvasEvent::PointerCancel,
    ] {
        let mut host = ScenePluginHost::with_defaults();
        let mut editor = editor();
        begin_move(&mut host, &mut editor);
        host.dispatch(&mut editor, event);
        assert_eq!(
            editor.scene.get(&"a".into()).unwrap().object.transform.tx,
            30.0
        );
        assert!(!editor.editing());
        assert!(!host.has_capture());
        assert!(!editor.undo());
    }
}
#[test]
fn background_pan_and_shift_marquee_are_owned_by_separate_plugins() {
    let mut host = ScenePluginHost::with_defaults();
    let mut editor = editor();
    down(&mut host, &mut editor, 500.0, 500.0, HitTarget::Background);
    movement(&mut host, &mut editor, 520.0, 510.0);
    assert_eq!(editor.viewport.pan, Point::new(20.0, 10.0));
    up(&mut host, &mut editor, 520.0, 510.0);
    host.set_enabled("pan", false, &mut editor).unwrap();
    let mut start = input(0.0, 0.0, HitTarget::Background);
    start.modifiers.shift = true;
    host.dispatch(&mut editor, CanvasEvent::PointerDown(start));
    movement(&mut host, &mut editor, 180.0, 140.0);
    assert!(host.marquee().is_some());
    up(&mut host, &mut editor, 180.0, 140.0);
    assert!(editor.selection.ids.contains(&"a".into()));
    assert_eq!(editor.viewport.pan, Point::new(20.0, 10.0));
}
#[test]
fn scene_marquee_obeys_visibility_clipping_and_hit_policy() {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("frame", d::Frame::new(100.0, 100.0)),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("outside", d::Rect::new(50.0, 50.0))
                .with_transform(Affine::translation(150.0, 0.0)),
            Some("frame".into()),
            None,
        )
        .unwrap();
    let mut hidden = DisplayObject::new("hidden", d::Rect::new(50.0, 50.0));
    hidden.visible = false;
    scene.insert(hidden, None, None).unwrap();
    let mut nohit = DisplayObject::new("nohit", d::Rect::new(50.0, 50.0));
    nohit.hit_policy = HitPolicy::None;
    scene.insert(nohit, None, None).unwrap();
    let mut editor = SceneEditor::new(scene).unwrap();
    let mut host = ScenePluginHost::with_defaults();
    host.execute(
        &mut editor,
        SceneCommand::SelectRect {
            rect: Rect::new(Point::ZERO, Size::new(300.0, 300.0)),
            additive: false,
        },
    )
    .unwrap();
    assert_eq!(editor.selection.ids.len(), 1);
    assert!(editor.selection.ids.contains(&"frame".into()));
}
#[test]
fn scene_resize_preserves_negative_origin_anchor_and_no_motion_is_noop() {
    let mut scene = Scene::new();
    let shape = d::Polygon::new(vec![
        Point::new(-50.0, -40.0),
        Point::new(50.0, -40.0),
        Point::new(50.0, 40.0),
        Point::new(-50.0, 40.0),
    ]);
    scene
        .insert(
            DisplayObject::new("p", shape).with_transform(Affine::translation(100.0, 100.0)),
            None,
            None,
        )
        .unwrap();
    let mut editor = SceneEditor::new(scene).unwrap();
    let mut host = ScenePluginHost::with_defaults();
    down(
        &mut host,
        &mut editor,
        152.0,
        138.0,
        HitTarget::ObjectResize("p".into()),
    );
    movement(&mut host, &mut editor, 152.0, 138.0);
    assert_eq!(
        editor.scene.get(&"p".into()).unwrap().object.transform,
        Affine::translation(100.0, 100.0)
    );
    movement(&mut host, &mut editor, 252.0, 218.0);
    let matrix = editor.scene.get(&"p".into()).unwrap().object.transform;
    assert!(
        matrix
            .transform_point(Point::new(-50.0, -40.0))
            .distance(Point::new(50.0, 60.0))
            < 0.001
    );
    up(&mut host, &mut editor, 252.0, 218.0);
    assert!(editor.undo());
}
#[test]
fn text_and_image_resize_remain_uniform() {
    for shape in [
        gpui_flow_scene::Shape::from(d::Text::new("text", Size::new(100.0, 50.0))),
        d::Image::new("asset://image", Size::new(100.0, 50.0)).into(),
    ] {
        let mut scene = Scene::new();
        scene
            .insert(DisplayObject::new("object", shape), None, None)
            .unwrap();
        let mut editor = SceneEditor::new(scene).unwrap();
        let mut host = ScenePluginHost::with_defaults();
        down(
            &mut host,
            &mut editor,
            100.0,
            50.0,
            HitTarget::ObjectResize("object".into()),
        );
        movement(&mut host, &mut editor, 150.0, 60.0);
        let matrix = editor.scene.get(&"object".into()).unwrap().object.transform;
        assert!((matrix.a - matrix.d).abs() < 0.001);
        assert!((matrix.a - 1.5).abs() < 0.001);
        host.cancel(&mut editor);
    }
}
#[test]
fn inherited_lock_prevents_drag_capture_and_data_edits() {
    let mut scene = Scene::new();
    let mut parent = DisplayObject::new("parent", d::Group::new());
    parent.locked = true;
    scene.insert(parent, None, None).unwrap();
    scene
        .insert(
            DisplayObject::new("child", d::Rect::new(100.0, 50.0)),
            Some("parent".into()),
            None,
        )
        .unwrap();
    let mut editor = SceneEditor::new(scene).unwrap();
    let mut host = ScenePluginHost::with_defaults();
    down(
        &mut host,
        &mut editor,
        10.0,
        10.0,
        HitTarget::Object("child".into()),
    );
    assert!(!host.has_capture());
    assert!(
        host.execute(
            &mut editor,
            SceneCommand::SetObjectData {
                id: "child".into(),
                data: json!({})
            }
        )
        .is_err()
    );
}
#[test]
fn world_drag_under_rotated_parent_uses_parent_inverse() {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("parent", d::Group::new()).with_transform(
                Affine::translation(300.0, 100.0)
                    * Affine::rotation(90.0)
                    * Affine::scale(2.0, 1.0),
            ),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("child", d::Rect::new(50.0, 50.0)),
            Some("parent".into()),
            None,
        )
        .unwrap();
    let mut editor = SceneEditor::new(scene).unwrap();
    let before = editor
        .scene
        .world_transform(&"child".into())
        .unwrap()
        .transform_point(Point::ZERO);
    let mut host = ScenePluginHost::with_defaults();
    down(
        &mut host,
        &mut editor,
        280.0,
        120.0,
        HitTarget::Object("child".into()),
    );
    movement(&mut host, &mut editor, 310.0, 160.0);
    let after = editor
        .scene
        .world_transform(&"child".into())
        .unwrap()
        .transform_point(Point::ZERO);
    assert!((after - before).distance(Point::new(30.0, 40.0)) < 0.001);
}
#[test]
fn independent_scene_snap_plugin_changes_move_and_disable_rolls_back() {
    let mut host = ScenePluginHost::with_defaults();
    host.install(SceneSnappingPlugin::default()).unwrap();
    let mut editor = editor();
    down(
        &mut host,
        &mut editor,
        50.0,
        40.0,
        HitTarget::Object("a".into()),
    );
    movement(&mut host, &mut editor, 58.0, 58.0);
    assert_eq!(
        editor.scene.get(&"a".into()).unwrap().object.transform.tx,
        40.0
    );
    host.set_enabled("snapping", false, &mut editor).unwrap();
    assert_eq!(
        editor.scene.get(&"a".into()).unwrap().object.transform.tx,
        30.0
    );
    assert!(!host.has_capture());
}
#[test]
fn scene_paste_remaps_ids_preserves_parent_links_and_undoes() {
    let mut fragment = Scene::new();
    fragment
        .insert(DisplayObject::new("group", d::Group::new()), None, None)
        .unwrap();
    fragment
        .insert(
            DisplayObject::new("a", d::Rect::new(20.0, 20.0)).with_style(Style::fill(Color::WHITE)),
            Some("group".into()),
            None,
        )
        .unwrap();
    let mut editor = editor();
    let mut host = ScenePluginHost::with_defaults();
    host.execute(
        &mut editor,
        SceneCommand::PasteScene {
            scene: fragment,
            offset: Point::new(100.0, 100.0),
        },
    )
    .unwrap();
    assert_eq!(editor.scene.len(), 4);
    assert_eq!(editor.selection.ids.len(), 1);
    let root = editor.selection.ids.iter().next().unwrap();
    assert_eq!(editor.scene.get(root).unwrap().children.len(), 1);
    assert_eq!(editor.scene.get(root).unwrap().object.transform.tx, 100.0);
    host.execute(&mut editor, SceneCommand::Undo).unwrap();
    assert_eq!(editor.scene.len(), 2);
}
struct Prevent;
impl CanvasPlugin<SceneEditor, SceneCommand> for Prevent {
    fn id(&self) -> &str {
        "custom-scene"
    }
    fn priority(&self) -> i32 {
        -100
    }
    fn on_event(
        &mut self,
        _: &CanvasEvent,
        _: &PluginContext<'_, SceneEditor>,
        out: &mut PluginOutput<SceneCommand>,
    ) {
        out.prevent_default();
    }
}
#[test]
fn same_generic_plugin_contract_can_replace_scene_default_behavior() {
    let mut host = ScenePluginHost::with_defaults();
    host.install(Prevent).unwrap();
    let mut editor = editor();
    let report = down(
        &mut host,
        &mut editor,
        50.0,
        40.0,
        HitTarget::Object("a".into()),
    );
    assert!(report.default_prevented);
    assert!(editor.selection.ids.is_empty());
    assert!(!host.has_capture());
}
#[test]
fn scene_plugin_replacement_cancels_even_unrelated_active_behavior() {
    let mut host = ScenePluginHost::with_defaults();
    let mut editor = editor();
    begin_move(&mut host, &mut editor);
    assert!(host.install(Prevent).is_err());
    host.replace(Prevent, &mut editor).unwrap();
    assert_eq!(
        editor.scene.get(&"a".into()).unwrap().object.transform.tx,
        30.0
    );
    assert!(!editor.editing());
}
