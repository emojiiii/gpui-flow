use super::*;
use gpui_flow_scene::{Bounds, Color, Point, Size, display::Canvas};
use std::{io::Cursor, sync::Arc};
fn surface() -> CanvasSurface {
    let mut c = Canvas::new(3., 2.);
    c.pixel_ratio = 2.;
    CanvasSurface::new(c, CompositeLimits::default()).unwrap()
}
fn data(w: u32, h: u32, pixels: Vec<u8>) -> CanvasPixels {
    CanvasPixels::from_rgba(w, h, pixels, CompositeLimits::default()).unwrap()
}
#[test]
fn canvas_pixel_exchange_is_physical_and_replaces_instead_of_blending() {
    let mut s = surface();
    s.fill_rect(Bounds::new(Point::ZERO, Size::new(3., 2.)), Color::WHITE)
        .unwrap();
    let input = data(2, 1, vec![255, 128, 0, 128, 90, 80, 70, 0]);
    let generation = s.generation();
    assert!(s.write_pixels(&input, 1, 2).unwrap());
    assert_eq!(s.generation(), generation + 1);
    assert_eq!(
        s.read_pixels(1, 2, 2, 1).unwrap().rgba(),
        &[255, 128, 0, 128, 0, 0, 0, 0]
    );
    assert_eq!(s.read_pixels(0, 2, 1, 1).unwrap().rgba(), &[255; 4]);
    assert_eq!((s.snapshot().width(), s.snapshot().height()), (6, 4));
}
#[test]
fn canvas_pixel_read_negative_regions_pad_transparent_without_mutation() {
    let mut s = surface();
    s.write_pixels(&data(1, 1, vec![0, 255, 0, 255]), 0, 0)
        .unwrap();
    let generation = s.generation();
    let old = s.snapshot();
    let read = s.read_pixels(-1, -1, 3, 3).unwrap();
    assert_eq!(&read.rgba()[16..20], &[0, 255, 0, 255]);
    assert_eq!(&read.rgba()[..16], &[0; 16]);
    assert_eq!(s.generation(), generation);
    assert!(Arc::ptr_eq(&old, &s.snapshot()));
    assert!(
        s.read_pixels(i32::MAX, i32::MIN, 2, 2)
            .unwrap()
            .rgba()
            .iter()
            .all(|n| *n == 0)
    );
}
#[test]
fn canvas_pixel_write_crops_source_at_negative_destination_and_noops_outside() {
    let mut s = surface();
    let input = data(
        2,
        2,
        vec![
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255,
        ],
    );
    s.write_pixels(&input, -1, -1).unwrap();
    assert_eq!(
        s.read_pixels(0, 0, 1, 1).unwrap().rgba(),
        &[255, 255, 0, 255]
    );
    let old = s.snapshot();
    let generation = s.generation();
    assert!(!s.write_pixels(&input, i32::MIN, i32::MAX).unwrap());
    assert_eq!(s.generation(), generation);
    assert!(Arc::ptr_eq(&old, &s.snapshot()));
}
#[test]
fn straight_rgba_exchange_preserves_representable_premultiplied_pixels() {
    let mut rgba = Vec::new();
    for a in 0..=255u8 {
        for c in 0..=255u8 {
            rgba.extend([c, 255 - c, c / 2, a]);
        }
    }
    let mut s = CanvasSurface::new(Canvas::new(256., 256.), CompositeLimits::default()).unwrap();
    s.write_pixels(&data(256, 256, rgba), 0, 0).unwrap();
    let first = s.snapshot();
    let read = s.read_pixels(0, 0, 256, 256).unwrap();
    s.write_pixels(&read, 0, 0).unwrap();
    assert_eq!(
        first.premultiplied_rgba(),
        s.snapshot().premultiplied_rgba()
    );
}
#[test]
fn pixel_input_and_read_reject_extents_lengths_and_budgets_atomically() {
    for (w, h, len) in [
        (0, 1, 0),
        (1, 0, 0),
        (1, 1, 3),
        (4097, 1, 0),
        (4096, 4096, 0),
    ] {
        assert!(CanvasPixels::from_rgba(w, h, vec![0; len], CompositeLimits::default()).is_err());
    }
    let limits = CompositeLimits {
        max_surface_bytes: 24,
        ..Default::default()
    };
    let mut s = CanvasSurface::new(Canvas::new(2., 2.), limits).unwrap();
    let old = s.snapshot();
    let g = s.generation();
    assert_eq!(
        s.read_pixels(0, 0, 2, 2).unwrap_err(),
        CompositeError::WorkingSetLimit
    );
    assert_eq!(
        s.write_pixels(&data(1, 1, vec![255; 4]), 0, 0).unwrap_err(),
        CompositeError::WorkingSetLimit
    );
    assert_eq!(s.generation(), g);
    assert!(Arc::ptr_eq(&old, &s.snapshot()));
}
#[test]
fn pixel_read_work_budget_precedes_allocation() {
    let limits = CompositeLimits {
        max_surface_writes: 12,
        ..Default::default()
    };
    let s = CanvasSurface::new(Canvas::new(2., 2.), limits).unwrap();
    assert_eq!(
        s.read_pixels(0, 0, 4, 4).unwrap_err(),
        CompositeError::WorkLimit
    );
}
fn decode(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
    let decoder = png::Decoder::new(Cursor::new(bytes));
    let mut reader = decoder.read_info().unwrap();
    let mut rgba = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut rgba).unwrap();
    assert_eq!(info.color_type, png::ColorType::Rgba);
    assert_eq!(info.bit_depth, png::BitDepth::Eight);
    rgba.truncate(info.buffer_size());
    (info.width, info.height, rgba)
}
#[test]
fn canvas_png_roundtrip_preserves_physical_alpha_without_mutating_surface() {
    let mut s = surface();
    s.write_pixels(&data(2, 1, vec![255, 128, 0, 128, 77, 88, 99, 255]), 1, 1)
        .unwrap();
    let old = s.snapshot();
    let generation = s.generation();
    let png = s.encode_png().unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    let (w, h, rgba) = decode(&png);
    assert_eq!((w, h), (6, 4));
    assert_eq!(rgba, s.read_pixels(0, 0, 6, 4).unwrap().rgba());
    assert_eq!(s.generation(), generation);
    assert!(Arc::ptr_eq(&old, &s.snapshot()));
}
#[test]
fn png_export_excludes_component_corners_and_survives_snapshot_owner_edits() {
    let mut c = Canvas::new(3., 2.).with_corners([2.; 4]);
    c.pixel_ratio = 2.;
    let mut s = CanvasSurface::new(c, CompositeLimits::default()).unwrap();
    s.fill_rect(Bounds::new(Point::ZERO, Size::new(3., 2.)), Color::WHITE)
        .unwrap();
    let old = s.snapshot();
    s.clear().unwrap();
    let (_, _, rgba) = decode(&old.encode_png(CompositeLimits::default()).unwrap());
    assert_eq!(&rgba[..4], &[255; 4]);
    assert!(
        s.read_pixels(0, 0, 6, 4)
            .unwrap()
            .rgba()
            .iter()
            .all(|n| *n == 0)
    );
}
#[test]
fn png_memory_work_and_encoded_output_budgets_fail_without_partial_publication() {
    let s = surface();
    let p = s.snapshot();
    let old = p.premultiplied_rgba().to_vec();
    for limits in [
        CompositeLimits {
            max_surface_bytes: 8 * 1024 * 1024 + p.byte_len() + 16,
            ..Default::default()
        },
        CompositeLimits {
            max_surface_bytes: 32,
            ..Default::default()
        },
        CompositeLimits {
            max_surface_writes: 1,
            ..Default::default()
        },
    ] {
        assert!(matches!(
            p.encode_png(limits),
            Err(CompositeError::WorkingSetLimit | CompositeError::WorkLimit)
        ));
    }
    assert_eq!(p.premultiplied_rgba(), old);
}
