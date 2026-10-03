use gpui_flow_editor::SceneEditor;
use gpui_flow_engine::PropertyKey;
use gpui_flow_interaction::{scene::*, *};
use gpui_flow_scene::{Color, DisplayObject, Point, Scene, Style, display};
use serde_json::json;

#[test]
fn state_paint_preserves_background_blur_for_every_visual_state() {
    let base = Style {
        background_blur: 4.,
        blur: 2.,
        grayscale: 0.25,
        ..Default::default()
    };
    let patch = StatePaint {
        fill: Some(Color::WHITE),
        opacity: Some(0.5),
    };
    let states = StateStyles {
        selected: patch.clone(),
        focus: patch.clone(),
        hover: patch.clone(),
        press: patch.clone(),
        disabled: patch,
    };
    for state in [
        VisualState {
            selected: true,
            ..Default::default()
        },
        VisualState {
            focus: true,
            ..Default::default()
        },
        VisualState {
            hover: true,
            ..Default::default()
        },
        VisualState {
            press: true,
            ..Default::default()
        },
        VisualState {
            disabled: true,
            ..Default::default()
        },
    ] {
        let presented = states.resolve(&base, state);
        assert_eq!(presented.background_blur, 4.);
        assert_eq!(presented.blur, 2.);
        assert_eq!(presented.grayscale, 0.25);
        assert_eq!(presented.opacity, 0.5);
        assert_eq!(presented.fill, Some(Color::WHITE.into()));
    }
    assert_eq!(base.opacity, 1.);
    assert_eq!(base.fill, Some(Color::BLACK.into()));
}

#[test]
fn hover_presentation_keeps_current_background_blur_without_history_or_geometry_changes() {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("glass", display::Rect::new(20., 20.)).with_style(Style {
                background_blur: 4.,
                ..Default::default()
            }),
            None,
            None,
        )
        .unwrap();
    let mut editor = SceneEditor::new(scene).unwrap();
    editor.selection.select("glass".into(), false);
    let handle = editor.engine.handle(&"glass".into()).unwrap();
    let identity = editor.engine.instance(handle).unwrap();
    let selection = editor.selection.clone();
    let bounds = editor.engine.world_bounds(&"glass".into()).unwrap();
    let canonical = editor.scene().clone();
    let mut styles = SceneStateStyles::new(&editor.engine);
    styles
        .bind(
            &editor.engine,
            handle,
            StateStyles {
                hover: StatePaint {
                    fill: Some(Color::WHITE),
                    opacity: Some(0.),
                },
                ..Default::default()
            },
        )
        .unwrap();
    let mut router = SceneEventRouter::new();
    let mut host = ScenePluginHost::new();
    router.dispatch(
        &mut editor,
        &mut host,
        CanvasEvent::PointerMove(PointerInput::left(
            Point::new(5., 5.),
            HitTarget::Object("glass".into()),
        )),
    );
    let revision = editor.engine.revision();
    {
        let presented = styles.presentation(&editor, &router).unwrap();
        let style = &presented.get(&"glass".into()).unwrap().object.style;
        assert_eq!(style.background_blur, 4.);
        assert_eq!(style.opacity, 0.);
    }
    assert_eq!(editor.scene(), &canonical);
    assert_eq!(editor.engine.revision(), revision);
    assert!(!editor.undo());

    editor
        .set_property(handle, PropertyKey::BackgroundBlur, json!(9.))
        .unwrap();
    assert_eq!(
        styles
            .presentation(&editor, &router)
            .unwrap()
            .get(&"glass".into())
            .unwrap()
            .object
            .style
            .background_blur,
        9.
    );
    router.dispatch(&mut editor, &mut host, CanvasEvent::PointerLeave);
    {
        let presented = styles.presentation(&editor, &router).unwrap();
        let style = &presented.get(&"glass".into()).unwrap().object.style;
        assert_eq!(style.background_blur, 9.);
        assert_eq!(style.opacity, 1.);
    }
    assert!(editor.undo());
    assert_eq!(editor.scene(), &canonical);
    assert!(editor.redo());
    assert_eq!(editor.selection, selection);
    assert_eq!(editor.engine.instance(handle).unwrap(), identity);
    assert_eq!(editor.engine.world_bounds(&"glass".into()).unwrap(), bounds);
    assert_eq!(
        editor.engine.effect_bounds(&"glass".into()).unwrap(),
        bounds
    );
}
