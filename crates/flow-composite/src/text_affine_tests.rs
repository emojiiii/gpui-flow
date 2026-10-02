//! Synthetic solid ink isolates the producer's exact local clip contract.
use super::*;
use gpui_flow_scene::{
    Affine, BlendMode, Bounds, Color, DisplayObject, Point, Scene, Size, Style, display,
};
use std::sync::{Arc, atomic::AtomicBool};
fn fixture(m: Affine) -> (DomainSource, Arc<TextImage>) {
    let mut s = Scene::new();
    s.insert(
        DisplayObject::new("t", display::Text::new("synthetic", Size::new(20., 20.)))
            .with_transform(m)
            .with_style(Style {
                fill: Some(Color::WHITE.into()),
                blend_mode: BlendMode::Normal,
                ..Default::default()
            }),
        None,
        None,
    )
    .unwrap();
    let d = scene_units(&s, CompositeLimits::default())
        .unwrap()
        .into_iter()
        .find_map(|u| match u {
            SceneUnit::Domain(d) => Some(*d),
            _ => None,
        })
        .unwrap();
    let i = Arc::new(TextImage {
        image: Arc::new(
            StaticImage::from_premultiplied_rgba(
                20,
                20,
                vec![255; 1600],
                CompositeLimits::default(),
            )
            .unwrap(),
        ),
        local_bounds: Bounds::new(Point::ZERO, Size::new(20., 20.)),
    });
    (d, i)
}
fn render(
    d: &DomainSource,
    t: &TextInputs,
    limits: CompositeLimits,
) -> Result<Option<CpuFrame>, CompositeError> {
    let b = Bounds::new(Point::ZERO, Size::new(100., 100.));
    rasterize_with_inputs(
        d,
        &ImageInputs::default(),
        t,
        RasterViewport {
            world_to_screen: Affine::IDENTITY,
            canvas_bounds: b,
            content_mask: b,
            device_scale: 1.,
        },
        &mut PixelBudget::new(limits, 0).unwrap(),
        &AtomicBool::new(false),
    )
}
fn at(f: &CpuFrame, x: usize, y: usize) -> u8 {
    let x = x as f32 - f.screen_bounds.origin.x;
    let y = y as f32 - f.screen_bounds.origin.y;
    if x < 0. || y < 0. || x >= f.width as f32 || y >= f.height as f32 {
        return 0;
    }
    f.premul_rgba[((y as u32 * f.width + x as u32) * 4 + 3) as usize]
}
#[test]
fn text_exact_local_clip_follows_affine_and_plain_insert_clears_old_metadata() {
    let (d, i) = fixture(Affine::new(0., 1., -1., 0., 30., 10.));
    let mut t = TextInputs::default();
    t.insert_clipped(
        "t".into(),
        Ok(Some(i.clone())),
        Bounds::new(Point::new(3., 4.), Size::new(8., 7.)),
    );
    let f = render(&d, &t, CompositeLimits::default()).unwrap().unwrap();
    assert_eq!(at(&f, 20, 15), 255);
    assert_eq!(at(&f, 25, 11), 0);
    assert_eq!(at(&f, 17, 15), 0);
    t.insert("t".into(), Ok(Some(i)));
    let f = render(&d, &t, CompositeLimits::default()).unwrap().unwrap();
    assert_eq!(at(&f, 25, 11), 255);
    assert_eq!(at(&f, 17, 15), 255);
}
#[test]
fn text_exact_local_clip_validates_and_admits_mask_scratch() {
    let (d, i) = fixture(Affine::translation(10., 10.));
    let mut t = TextInputs::default();
    t.insert("t".into(), Ok(Some(i.clone())));
    let bounds = Bounds::new(Point::ZERO, Size::new(100., 100.));
    let mut baseline = PixelBudget::new(CompositeLimits::default(), 0).unwrap();
    rasterize_with_inputs(
        &d,
        &ImageInputs::default(),
        &t,
        RasterViewport {
            world_to_screen: Affine::IDENTITY,
            canvas_bounds: bounds,
            content_mask: bounds,
            device_scale: 1.,
        },
        &mut baseline,
        &AtomicBool::new(false),
    )
    .unwrap();
    let limits = CompositeLimits {
        max_surface_bytes: baseline.peak_bytes(),
        ..Default::default()
    };
    assert!(render(&d, &t, limits).is_ok());
    t.insert_clipped(
        "t".into(),
        Ok(Some(i.clone())),
        Bounds::new(Point::ZERO, Size::new(20., 20.)),
    );
    assert_eq!(
        render(&d, &t, limits).unwrap_err(),
        CompositeError::WorkingSetLimit
    );
    t.insert_clipped(
        "t".into(),
        Ok(Some(i)),
        Bounds::new(Point::new(f32::NAN, 0.), Size::new(20., 20.)),
    );
    assert!(matches!(
        render(&d, &t, CompositeLimits::default()),
        Err(CompositeError::InvalidGeometry(_))
    ));
}
