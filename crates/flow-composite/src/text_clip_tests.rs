use super::*;
use gpui_flow_scene::{
    Affine, BlendMode, Bounds, Color, DisplayObject, Point, Scene, Shape, Size, Style, display,
};
use std::sync::{Arc, atomic::AtomicBool};
fn scene(rounded: bool, nested: bool, transform: Affine) -> Scene {
    let mut s = Scene::new();
    let mut frame = display::Frame::new(40., 40.);
    if rounded {
        frame.corner_radii = Some([20., 0., 15., 0.].into());
    }
    s.insert(
        DisplayObject::new("outer", frame)
            .with_transform(transform)
            .with_style(Style {
                fill: None,
                ..Default::default()
            }),
        None,
        None,
    )
    .unwrap();
    let parent = if nested {
        s.insert(
            DisplayObject::new(
                "inner",
                display::Box::new(40., 40.)
                    .clipped(true)
                    .with_corners([0., 20., 0., 0.]),
            )
            .with_style(Style {
                fill: None,
                ..Default::default()
            }),
            Some("outer".into()),
            None,
        )
        .unwrap();
        "inner"
    } else {
        "outer"
    };
    s.insert(
        DisplayObject::new("text", display::Text::new("fixture", Size::new(40., 40.)))
            .with_style(Style::fill(Color::WHITE)),
        Some(parent.into()),
        None,
    )
    .unwrap();
    s
}
fn domain(s: &Scene) -> DomainSource {
    scene_units(s, CompositeLimits::default())
        .unwrap()
        .into_iter()
        .find_map(|u| match u {
            SceneUnit::Domain(d) => Some(*d),
            _ => None,
        })
        .unwrap()
}
fn at(f: &CpuFrame, x: u32, y: u32) -> [u8; 4] {
    let x = x as f32 - f.screen_bounds.origin.x;
    let y = y as f32 - f.screen_bounds.origin.y;
    if x < 0. || y < 0. || x >= f.width as f32 || y >= f.height as f32 {
        return [0; 4];
    }
    f.premul_rgba[((y as u32 * f.width + x as u32) * 4) as usize..][..4]
        .try_into()
        .unwrap()
}
fn render(s: &Scene) -> CpuFrame {
    // Synthetic solid ink isolates mask composition. Real glyphs are exercised
    // by the UI bridge regression, not represented by this fixture.
    let mut texts = TextInputs::default();
    texts.insert(
        "text".into(),
        Ok(Some(Arc::new(TextImage {
            image: Arc::new(
                StaticImage::from_premultiplied_rgba(
                    40,
                    40,
                    [255, 0, 0, 255].repeat(1600),
                    CompositeLimits::default(),
                )
                .unwrap(),
            ),
            local_bounds: Bounds::new(Point::ZERO, Size::new(40., 40.)),
        }))),
    );
    let b = Bounds::new(Point::ZERO, Size::new(100., 100.));
    rasterize_with_inputs(
        &domain(s),
        &ImageInputs::default(),
        &texts,
        RasterViewport {
            world_to_screen: Affine::IDENTITY,
            canvas_bounds: b,
            content_mask: b,
            device_scale: 1.,
        },
        &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap()
}
#[test]
fn rounded_text_ancestors_create_domains_but_rectangles_keep_native_fast_path() {
    let plain = scene(false, false, Affine::IDENTITY);
    assert!(
        scene_units(&plain, CompositeLimits::default())
            .unwrap()
            .iter()
            .any(|u| matches!(u,SceneUnit::Native(id) if id == &"text".into()))
    );
    for (rounded, transform) in [
        (true, Affine::IDENTITY),
        (false, Affine::rotation(20.)),
        (false, Affine::scale(2., 1.)),
    ] {
        let s = scene(rounded, false, transform);
        let d = domain(&s);
        assert_eq!(d.root_id(), &"text".into());
        assert_eq!(d.clips.len(), 1);
        assert_eq!(d.text_sources().len(), 1);
    }
}
#[test]
fn text_clip_domains_preserve_opposite_and_nested_corner_pixels() {
    let single = render(&scene(true, false, Affine::IDENTITY));
    assert_eq!(at(&single, 1, 1), [0; 4]);
    assert_eq!(at(&single, 38, 1), [255, 0, 0, 255]);
    assert_eq!(at(&single, 38, 38), [0; 4]);
    assert_eq!(at(&single, 1, 38), [255, 0, 0, 255]);
    let nested = render(&scene(true, true, Affine::IDENTITY));
    assert_eq!(at(&nested, 38, 1), [0; 4]);
    assert_eq!(at(&nested, 20, 20), [255, 0, 0, 255]);
}
#[test]
fn text_ancestor_masks_match_an_explicit_container_domain() {
    let automatic = scene(true, true, Affine::translation(5., 5.));
    let mut explicit = automatic.clone();
    explicit
        .update_object(&"outer".into(), |o| o.style.blend_mode = BlendMode::Normal)
        .unwrap();
    let a = render(&automatic);
    let b = render(&explicit);
    for y in 0..50 {
        for x in 0..50 {
            assert_eq!(at(&a, x, y), at(&b, x, y), "{x},{y}");
        }
    }
}
#[test]
fn text_clip_domain_budget_and_disabled_clips_are_explicit() {
    let mut s = scene(true, false, Affine::IDENTITY);
    let limits = CompositeLimits {
        max_domains: 0,
        ..Default::default()
    };
    assert!(
        scene_units(&s, limits)
            .unwrap()
            .iter()
            .any(|u| matches!(u,SceneUnit::Rejected(i) if i.error == CompositeError::InputLimit))
    );
    s.update_object(&"outer".into(), |o| {
        if let Shape::Frame(f) = &mut o.shape {
            f.clip_children = false;
        }
    })
    .unwrap();
    assert!(
        scene_units(&s, CompositeLimits::default())
            .unwrap()
            .iter()
            .all(|u| !matches!(u, SceneUnit::Domain(_)))
    );
}
