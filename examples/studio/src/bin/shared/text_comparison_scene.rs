use gpui_flow::scene::{
    Affine, BlendMode, Color, DisplayObject, Scene, Size as SceneSize, Stroke, Style,
    display::{self, TextOverflow},
};
pub fn document(content: &str) -> Scene {
    let mut scene = Scene::new();
    for (id, x, blend, label) in [
        ("native", 35., BlendMode::PassThrough, "Native glyph path"),
        (
            "cpu",
            365.,
            BlendMode::Normal,
            "CPU glyph path, same backend",
        ),
    ] {
        let mut title = display::Text::new(label, SceneSize::new(270., 30.));
        title.font_size = 17.;
        title.wrap = false;
        scene
            .insert(
                DisplayObject::new(format!("{id}-title"), title)
                    .with_transform(Affine::translation(x, 25.))
                    .with_style(Style::fill(Color::WHITE)),
                None,
                None,
            )
            .unwrap();
        scene
            .insert(
                DisplayObject::new(format!("{id}-box"), display::Rect::new(280., 130.))
                    .with_transform(Affine::translation(x, 70.))
                    .with_style(Style {
                        fill: Some(Color::hex(0x142133).into()),
                        stroke: Some(Stroke::new(Color::hex(0x4176a5), 2.)),
                        ..Default::default()
                    }),
                None,
                None,
            )
            .unwrap();
        let mut text = display::Text::new(content, SceneSize::new(280., 130.));
        text.font_family = "DejaVu Sans".into();
        text.font_size = 22.;
        text.overflow = TextOverflow::Ellipsis;
        text.line_height = 1.4;
        text.padding = [12.; 4];
        scene
            .insert(
                DisplayObject::new(id, text)
                    .with_transform(Affine::translation(x, 70.))
                    .with_style(Style {
                        fill: Some(Color::hex(0xf2d289).into()),
                        blend_mode: blend,
                        ..Default::default()
                    }),
                None,
                None,
            )
            .unwrap();
    }
    scene
}
