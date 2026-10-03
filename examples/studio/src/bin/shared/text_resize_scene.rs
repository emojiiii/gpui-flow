use gpui_flow::scene::{Affine, Color, DisplayObject, Scene, Size, Style, display};
fn text(label: &str) -> display::Text {
    let mut t = display::Text::new(label, Size::new(220., 100.));
    t.font_family = "DejaVu Sans".into();
    t.font_size = 23.;
    t.padding = [8.; 4];
    t.overflow = display::TextOverflow::Ellipsis;
    t.decoration = display::TextDecoration::Underline;
    t
}
pub fn scene() -> Scene {
    let mut scene = Scene::new();
    let image = format!("{}/assets/checker.png", env!("CARGO_MANIFEST_DIR"));
    scene
        .insert(
            DisplayObject::new("text", text("Text can resize\nand rotate · ffi café"))
                .with_transform(Affine::translation(80., 90.))
                .with_style(Style::fill(Color::hex(0xf4dc94))),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("rect", display::Rect::new(100., 80.))
                .with_transform(Affine::translation(320., 100.))
                .with_style(Style::fill(Color::hex(0x55a7ed))),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("affine-parent", display::Group::new()).with_transform(
                Affine::translation(560., 170.) * Affine::rotation(-12.) * Affine::scale(1.2, 0.8),
            ),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("affine-text", text("Affine parent\nSame glyph backend"))
                .with_transform(Affine::rotation(30.))
                .with_style(Style::fill(Color::hex(0x8bd7ff))),
            Some("affine-parent".into()),
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new(
                "image",
                display::Image::new(image, Size::new(160., 100.)).with_corners([28., 0., 18., 0.]),
            )
            .with_transform(Affine::translation(80., 340.)),
            None,
            None,
        )
        .unwrap();
    scene
}
