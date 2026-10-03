use gpui_flow::scene::{
    Affine, Color, DisplayObject, Point as ScenePoint, Scene, Size, Style, display,
};
pub fn scene() -> Scene {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("rect", display::Rect::new(180., 110.))
                .with_transform(Affine::translation(80., 80.))
                .with_style(Style::fill(Color::hex(0x55a7ed))),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("affine-parent", display::Group::new()).with_transform(
                Affine::translation(470., 160.) * Affine::rotation(-12.) * Affine::scale(1.4, 0.8),
            ),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new(
                "rotated",
                display::Polygon::new(vec![
                    ScenePoint::new(-70., -40.),
                    ScenePoint::new(70., -40.),
                    ScenePoint::new(70., 40.),
                    ScenePoint::new(-70., 40.),
                ]),
            )
            .with_transform(Affine::rotation(32.))
            .with_style(Style::fill(Color::hex(0x9c7aea))),
            Some("affine-parent".into()),
            None,
        )
        .unwrap();
    let mut text = display::Text::new("Uniform Text\nDouble-click to edit", Size::new(240., 70.));
    text.font_size = 24.;
    scene
        .insert(
            DisplayObject::new("text", text)
                .with_transform(Affine::translation(740., 100.))
                .with_style(Style::fill(Color::hex(0xf4dc94))),
            None,
            None,
        )
        .unwrap();
    let image = format!("{}/assets/checker.png", env!("CARGO_MANIFEST_DIR"));
    scene
        .insert(
            DisplayObject::new("image", display::Image::new(image, Size::new(160., 100.)))
                .with_transform(Affine::translation(90., 330.)),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("mixed", display::Box::new(240., 110.))
                .with_transform(Affine::translation(390., 330.))
                .with_style(Style::fill(Color::hex(0x31594f))),
            None,
            None,
        )
        .unwrap();
    let mut child = display::Text::new("Native child\nParent stays uniform", Size::new(210., 70.));
    child.font_size = 20.;
    scene
        .insert(
            DisplayObject::new("mixed-text", child)
                .with_transform(Affine::translation(15., 20.))
                .with_style(Style::fill(Color::hex(0xd8f5e6))),
            Some("mixed".into()),
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("small", display::Rect::new(12., 8.))
                .with_transform(Affine::translation(790., 370.))
                .with_style(Style::fill(Color::hex(0xed8896))),
            None,
            None,
        )
        .unwrap();
    scene
}
