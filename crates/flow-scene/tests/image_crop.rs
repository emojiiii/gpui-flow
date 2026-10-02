use gpui_flow_scene::{
    Bounds, Point, Size,
    display::{Image, ImageFit},
};
fn crop(x: f32, y: f32, w: f32, h: f32) -> Bounds {
    Bounds::new(Point::new(x, y), Size::new(w, h))
}
#[test]
fn crop_legacy_json_roundtrip_and_invalid_rectangles() {
    let original = Image::new("test", Size::new(100., 100.));
    let encoded = serde_json::to_value(&original).unwrap();
    assert!(encoded.get("source_crop").is_none());
    assert_eq!(serde_json::from_value::<Image>(encoded).unwrap(), original);
    for c in [
        crop(-0.1, 0., 1., 1.),
        crop(0., 0., 0., 1.),
        crop(0.5, 0., 0.6, 1.),
        crop(0., 0., 1., f32::NAN),
        crop(0., 0., f32::INFINITY, 1.),
    ] {
        let mut image = original.clone();
        image.source_crop = Some(c);
        assert!(image.validate().is_err(), "{c:?}");
    }
    let mut image = original;
    image.source_crop = Some(crop(0.2, 0.4, 0.8, 0.6));
    image.validate().unwrap();
    assert_eq!(
        serde_json::from_value::<Image>(serde_json::to_value(&image).unwrap()).unwrap(),
        image
    );
}
#[test]
fn crop_fit_maps_actual_decoded_pixels_and_never_metadata() {
    let mut image = Image::new("test", Size::new(100., 100.));
    image.natural_size = Some(Size::new(5., 400.));
    image.source_crop = Some(crop(0.5, 0.25, 0.5, 0.5));
    for (fit, expected) in [
        (ImageFit::Fill, crop(0., 0., 100., 100.)),
        (ImageFit::Contain, crop(0., 25., 100., 50.)),
        (ImageFit::Cover, crop(-50., 0., 200., 100.)),
    ] {
        image.fit = fit;
        let m = image.mapping(Size::new(400., 200.)).unwrap();
        assert_eq!(m.source, crop(200., 50., 200., 100.));
        assert_eq!(m.destination, expected);
        assert_eq!(
            m.full_source.origin.x,
            expected.origin.x - expected.size.width
        );
        assert_eq!(
            m.full_source.origin.y,
            expected.origin.y - expected.size.height * 0.5
        );
    }
    assert!(image.mapping(Size::new(0., 200.)).is_none());
}
