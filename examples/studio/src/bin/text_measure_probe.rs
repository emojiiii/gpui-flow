//! Real native shaping and reactive editor regression, without opening a window.
use gpui::{FontWeight, font};
use gpui_flow::{
    editor::SceneEditor,
    layout::*,
    scene::{DisplayObject, Scene, Shape, Size, display},
};
use gpui_flow_text::{GpuiTextMeasurer, TextMeasureRequest, TextMeasurer};
use std::sync::Arc;
fn request(content: &str, width: Option<f32>) -> TextMeasureRequest {
    TextMeasureRequest {
        content: content.into(),
        font_family: "DejaVu Sans".into(),
        font_size: 16.,
        font_weight: 400,
        italic: false,
        line_height: 19.2,
        wrap_width: width,
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app = gpui_kit::platform::headless();
    let system = app.text_system();
    let backend = Arc::new(GpuiTextMeasurer::new(system.clone()));
    println!("available_font_families={}", system.all_font_names().len());
    let cases = [
        ("empty", "", "DejaVu Sans", 400, None),
        ("narrow_glyphs", "iiii", "DejaVu Sans", 400, None),
        ("wide_glyphs", "WWWW", "DejaVu Sans", 400, None),
        ("bold", "Typography", "DejaVu Sans", 700, None),
        (
            "wrapped",
            "English words wrap into lines",
            "DejaVu Sans",
            400,
            Some(80.),
        ),
        (
            "cjk",
            "你好，画布世界。",
            "Noto Sans CJK SC",
            400,
            Some(80.),
        ),
        ("emoji", "Hello 👩‍💻🌍", "DejaVu Sans", 400, Some(80.)),
        ("trailing_newline", "A\n", "DejaVu Sans", 400, None),
        (
            "fallback",
            "Text fallback",
            "Missing-Flow-Font-Deliberate",
            700,
            None,
        ),
    ];
    for (label, content, family, weight, width) in cases {
        let mut input = request(content, width);
        input.font_family = family.into();
        input.font_weight = weight;
        let metrics = backend.measure(&input)?;
        assert_eq!(backend.measure(&input)?, metrics, "cached metrics");
        assert_eq!(metrics.height, input.line_height * metrics.lines as f32);
        if label == "empty" {
            assert_eq!(metrics.lines, 1);
            assert_eq!(metrics.width, 0.);
        }
        if label == "trailing_newline" {
            assert_eq!(metrics.lines, 2);
        }
        println!(
            "{label}: width={:.3}, height={:.3}, rows={}",
            metrics.width, metrics.height, metrics.lines
        );
    }
    assert!(
        backend.measure(&request("WWWW", None))?.width
            > backend.measure(&request("iiii", None))?.width * 2.
    );
    let mut missing = font("Missing-Flow-Font-Deliberate");
    missing.weight = FontWeight(700.);
    let fallback = system
        .get_font_for_id(system.resolve_font(&missing))
        .expect("resolved fallback");
    assert_ne!(fallback.family.as_ref(), "Missing-Flow-Font-Deliberate");
    println!("actual_fallback_family={}", fallback.family);
    let base = request("zoom keeps model metrics", Some(150.));
    let before = backend.measure(&base)?;
    for scale in [0.5, 2., 1.] {
        let mut paint = base.clone();
        paint.font_size *= scale;
        paint.line_height *= scale;
        paint.wrap_width = paint.wrap_width.map(|w| w * scale);
        let scaled = backend.measure(&paint)?;
        assert_eq!(scaled.lines, before.lines);
    }
    assert_eq!(backend.measure(&base)?, before);
    backend.invalidate();
    assert_eq!(backend.measure(&base)?, before);
    let mut invalid = base.clone();
    invalid.font_size = f32::INFINITY;
    assert!(backend.measure(&invalid).is_err());
    let mut scene = Scene::new();
    scene.insert(
        DisplayObject::new("frame", display::Frame::new(180., 500.)),
        None,
        None,
    )?;
    scene.insert(
        DisplayObject::new(
            "text",
            display::Text::new(
                "你好，world 👩‍💻. English words wrap into measured lines.\n",
                Size::new(80., 30.),
            ),
        ),
        Some("frame".into()),
        None,
    )?;
    let mut config = FlowLayout::default();
    config.containers.insert(
        "frame".into(),
        FlowStyle {
            direction: FlowDirection::Column,
            align_items: AlignItems::Stretch,
            height_sizing: AxisSizing::content(),
            padding: Insets::all(10.),
            ..Default::default()
        },
    );
    config.texts.insert("text".into(), TextSizing::AutoHeight);
    let measured = config.with_text_measurer(backend);
    measured.apply_to_scene(&mut scene)?;
    for item in scene.paint_list()? {
        if matches!(item.object.shape, Shape::Text(_)) {
            assert!(
                gpui_flow::display::RenderObject::from_paint_item(
                    &item,
                    gpui_flow::core::Viewport::default()
                )
                .diagnostics()
                .is_empty()
            );
        }
    }
    let mut editor = SceneEditor::new(scene.clone())?;
    measured.install_reactive(&mut editor.engine)?;
    let old_height = scene
        .get(&"frame".into())
        .unwrap()
        .object
        .shape
        .local_bounds()
        .unwrap()
        .size
        .height;
    editor.transaction(|s| {
        s.update_object(&"frame".into(), |o| {
            let Shape::Frame(f) = &mut o.shape else {
                unreachable!()
            };
            f.size.width = 320.;
        })
    })?;
    let new_height = editor
        .scene()
        .get(&"frame".into())
        .unwrap()
        .object
        .shape
        .local_bounds()
        .unwrap()
        .size
        .height;
    assert!(new_height < old_height);
    assert!(measured.arranged(editor.scene())?.1.changed.is_empty());
    assert!(editor.undo());
    assert_eq!(editor.scene(), &scene);
    assert!(!editor.undo());
    measured.refresh_text_metrics(&mut editor)?;
    assert_eq!(editor.scene(), &scene);
    assert!(!editor.undo());
    println!(
        "native_reactive: parent height {old_height:.3} -> {new_height:.3}; one Undo; invalidation is stable"
    );
    Ok(())
}
