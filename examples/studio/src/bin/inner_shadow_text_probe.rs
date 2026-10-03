//! Real-font effect-source boundary probe, using the same bounded native bridge.
use gpui::TextAlign;
use gpui_flow::{
    composite::*,
    scene::{Affine, Bounds, Color, DisplayObject, Point, Scene, Shadow, Size, Style, display},
    text::{GpuiTextRasterizer, TextMeasureRequest, TextRasterRequest},
};
use std::sync::{Arc, atomic::AtomicBool};
fn main() -> Result<(), String> {
    let (_app, bridge) =
        GpuiTextRasterizer::application(gpui_kit::platform::current_platform(true));
    let cancel = AtomicBool::new(false);
    for ancestor in [false, true] {
        for dpr in [1., 1.25, 2.] {
            let x = if ancestor { -120. } else { -10. };
            let width = if ancestor { 100. } else { 200. };
            let mut scene = Scene::new();
            if ancestor {
                scene
                    .insert(
                        DisplayObject::new("g", display::Group::new()).with_style(Style {
                            shadow: Some(Shadow::new(140., 0., 0., Color::BLACK)),
                            ..Default::default()
                        }),
                        None,
                        None,
                    )
                    .unwrap();
            }
            let mut text = display::Text::new("████", Size::new(width, 80.));
            text.font_size = 60.;
            text.wrap = false;
            text.line_height = 70. / 60.;
            text.font_family = "DejaVu Sans".into();
            scene
                .insert(
                    DisplayObject::new("t", text)
                        .with_transform(Affine::translation(x, 20.))
                        .with_style(Style {
                            fill: Some(Color::WHITE.into()),
                            inner_shadow_layers: if ancestor {
                                vec![]
                            } else {
                                vec![Shadow::new(5., 0., 0., Color::rgb(255, 0, 0))]
                            },
                            ..Default::default()
                        }),
                    ancestor.then(|| "g".into()),
                    None,
                )
                .unwrap();
            let SceneUnit::Domain(source) = scene_units(&scene, CompositeLimits::default())
                .unwrap()
                .remove(0)
            else {
                panic!("effect domain");
            };
            assert!(source.text_sources()[0].2);
            let bounds = [x, 20., width, 80.];
            let mut request = TextRasterRequest {
                overflow: gpui_flow_text::TextOverflow::Clip,
                decoration: gpui_flow_text::TextDecoration::None,
                vertical_align: gpui_flow_text::VerticalAlign::Top,
                text: TextMeasureRequest {
                    content: "████".into(),
                    font_family: "DejaVu Sans".into(),
                    font_size: 60.,
                    font_weight: 400,
                    italic: false,
                    text_case: Default::default(),
                    para_spacing: 0.,
                    letter_spacing: 0.,
                    word_spacing: 0.,
                    para_indent: 0.,
                    line_height: 70.,
                    wrap_width: None,
                },
                align: TextAlign::Left,
                bounds,
                clip: bounds,
                device_scale: dpr,
                color: [255; 4],
            };
            let viewport = RasterViewport {
                world_to_screen: Affine::IDENTITY,
                canvas_bounds: Bounds::new(Point::ZERO, Size::new(160., 120.)),
                content_mask: Bounds::new(Point::ZERO, Size::new(160., 120.)),
                device_scale: dpr,
            };
            let mut frames = Vec::new();
            for full in [true, false] {
                request.clip = if full { bounds } else { [0., 0., 160., 120.] };
                let mut work = 32_000_000;
                let raster = bridge.rasterize(
                    &request,
                    bridge.epoch(),
                    &cancel,
                    32 * 1024 * 1024,
                    &mut work,
                )?;
                let mut inputs = TextInputs::default();
                let image = raster.map(|r| {
                    let local_bounds = Bounds::new(
                        Point::new(r.origin[0] as f32 / dpr - x, r.origin[1] as f32 / dpr - 20.),
                        Size::new(r.width as f32 / dpr, r.height as f32 / dpr),
                    );
                    let image = Arc::new(
                        StaticImage::from_premultiplied_rgba(
                            r.width,
                            r.height,
                            r.premul_rgba,
                            CompositeLimits::default(),
                        )
                        .unwrap(),
                    );
                    Arc::new(TextImage {
                        image,
                        local_bounds,
                    })
                });
                inputs.insert("t".into(), Ok(image));
                frames.push(
                    rasterize_with_inputs(
                        &source,
                        &ImageInputs::default(),
                        &inputs,
                        viewport,
                        &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
                        &cancel,
                    )
                    .unwrap(),
                );
            }
            let correct = frames[0]
                .as_ref()
                .expect("full source survives viewport crop");
            if ancestor {
                assert!(
                    correct
                        .premul_rgba
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .any(|p| p[3] > 200)
                );
                assert!(
                    frames[1].is_none(),
                    "offscreen text was removed by old crop"
                );
            } else {
                let cropped = frames[1].as_ref().unwrap();
                let sample = |f: &CpuFrame, x: u32, y: u32| -> [u8; 4] {
                    let ix = (x as f32 - f.screen_bounds.origin.x * dpr).floor() as i32;
                    let iy = (y as f32 - f.screen_bounds.origin.y * dpr).floor() as i32;
                    if ix < 0 || iy < 0 || ix >= f.width as i32 || iy >= f.height as i32 {
                        return [0; 4];
                    }
                    f.premul_rgba[((iy as u32 * f.width + ix as u32) * 4) as usize..][..4]
                        .try_into()
                        .unwrap()
                };
                assert!(
                    (0..(5. * dpr) as u32).any(|x| (20..(100. * dpr) as u32).any(|y| {
                        let a = sample(correct, x, y);
                        let b = sample(cropped, x, y);
                        a == [255; 4] && b[0] == 255 && b[1] < 200
                    })),
                    "cropped glyph alpha must expose the false boundary regression"
                );
            }
            println!("real-font complete source: ancestor={ancestor}, DPR={dpr}: passed");
        }
    }
    let request = TextRasterRequest {
        overflow: gpui_flow_text::TextOverflow::Clip,
        decoration: gpui_flow_text::TextDecoration::None,
        vertical_align: gpui_flow_text::VerticalAlign::Top,
        text: TextMeasureRequest {
            content: "too large".into(),
            font_family: "DejaVu Sans".into(),
            font_size: 20.,
            font_weight: 400,
            italic: false,
            text_case: Default::default(),
            para_spacing: 0.,
            letter_spacing: 0.,
            word_spacing: 0.,
            para_indent: 0.,
            line_height: 24.,
            wrap_width: None,
        },
        align: TextAlign::Left,
        bounds: [-5000., 0., 6000., 100.],
        clip: [-5000., 0., 6000., 100.],
        device_scale: 1.,
        color: [255; 4],
    };
    let mut work = 32_000_000;
    assert!(
        bridge
            .rasterize(
                &request,
                bridge.epoch(),
                &cancel,
                32 * 1024 * 1024,
                &mut work
            )
            .is_err()
    );
    assert_eq!(work, 32_000_000);
    println!("oversized full text source rejects before pixel work: passed");
    Ok(())
}
