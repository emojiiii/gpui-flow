//! Real GPUI font raster evidence. Run on the same desktop backend, without a window.
use gpui::{RenderGlyphParams, TextAlign, WindowTextSystem, point, px};
use gpui_flow_text::{
    GpuiTextRasterizer, TextMeasureRequest, TextRasterRequest, gpui_backend::shape_text,
};
use std::sync::{Arc, atomic::AtomicBool};
fn request(content: &str) -> TextRasterRequest {
    TextRasterRequest {
        overflow: gpui_flow_text::TextOverflow::Clip,
        decoration: gpui_flow_text::TextDecoration::None,
        vertical_align: gpui_flow_text::VerticalAlign::Top,
        text: TextMeasureRequest {
            content: content.into(),
            font_family: "DejaVu Sans".into(),
            font_size: 28.,
            font_weight: 400,
            italic: false,
            text_case: Default::default(),
            para_spacing: 0.,
            letter_spacing: 0.,
            word_spacing: 0.,
            para_indent: 0.,
            line_height: 36.,
            wrap_width: Some(240.),
        },
        align: TextAlign::Left,
        bounds: [10.25, 12., 240., 180.],
        clip: [0., 0., 800., 600.],
        device_scale: 1.,
        color: [255, 255, 255, 255],
    }
}
fn main() -> Result<(), String> {
    let platform = gpui_kit::platform::current_platform(true);
    let raw = platform.text_system();
    let (app, bridge) = GpuiTextRasterizer::application(platform);
    let system = app.text_system();
    assert!(bridge.matches_text_system(&system));
    let cancel = AtomicBool::new(false);
    let cases = [
        ("latin", "office ffi café e\u{301}", "DejaVu Sans"),
        ("cjk", "你好，画布世界。 Mixed English", "Noto Sans CJK SC"),
        ("emoji", "Hello 👩‍💻🌍", "DejaVu Sans"),
        ("rtl", "مرحبا بالعالم שלום", "DejaVu Sans"),
        (
            "wrap",
            "English words wrap into several rows naturally.",
            "DejaVu Sans",
        ),
        ("newline", "A\n\nB\n", "DejaVu Sans"),
        ("fallback", "Fallback shape", "Missing-Flow-Font-Deliberate"),
        ("empty", "", "DejaVu Sans"),
        ("spaces", "    ", "DejaVu Sans"),
    ];
    let names = system.all_font_names();
    println!("available_font_families={}", names.len());
    for (label, content, family) in cases {
        let mut r = request(content);
        r.text.font_family = family.into();
        if label == "cjk" && !names.iter().any(|n| n == family) {
            println!("SKIP cjk: {family} not installed");
            continue;
        }
        for align in [TextAlign::Left, TextAlign::Center, TextAlign::Right] {
            r.align = align;
            let shaped = shape_text(
                &WindowTextSystem::new(system.clone()),
                &r.text,
                gpui::white(),
            )?;
            let rows = shaped
                .iter()
                .map(|line| line.wrap_boundaries.len() + 1)
                .sum::<usize>();
            let glyphs = shaped
                .iter()
                .flat_map(|line| &line.unwrapped_layout.runs)
                .map(|run| run.glyphs.len())
                .sum::<usize>();
            let missing = shaped
                .iter()
                .flat_map(|line| &line.unwrapped_layout.runs)
                .flat_map(|run| &run.glyphs)
                .filter(|glyph| glyph.id.0 == 0)
                .count();
            let mut work = 32_000_000;
            let pixels = bridge
                .rasterize(&r, bridge.epoch(), &cancel, 32 * 1024 * 1024, &mut work)?
                .unwrap();
            assert_eq!(pixels.rows, rows);
            assert_eq!(pixels.glyphs, glyphs);
            assert!(pixels.peak_bytes <= 32 * 1024 * 1024);
            assert_eq!(pixels.pixel_writes, 32_000_000 - work);
            assert!(
                pixels
                    .premul_rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .all(|p| p[..3].iter().all(|c| *c <= p[3]))
            );
            let ink = pixels
                .premul_rgba
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|p| p[3] > 0)
                .count();
            if label == "empty" || label == "spaces" {
                assert_eq!(ink, 0);
            } else {
                assert!(ink > 0);
            }
            if label == "newline" {
                assert_eq!(rows, 4);
            }
            if label == "cjk" {
                assert_eq!(missing, 0, "CJK must not pass as missing-glyph boxes");
            }
            if label == "emoji" && names.iter().any(|n| n.contains("Color Emoji")) {
                assert!(
                    pixels.color_glyphs > 0,
                    "installed emoji must produce color pixels"
                );
                assert!(
                    pixels
                        .premul_rgba
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .any(|p| p[0] != p[1] || p[1] != p[2])
                );
            }
            println!(
                "{label} {align:?}: rows={rows} glyphs={glyphs} missing={missing} color={} ink={ink} peak={}",
                pixels.color_glyphs, pixels.peak_bytes
            );
        }
    }
    // Compare every pixel of one real A glyph with the exact backend data and
    // the same baseline/quarter-pixel parameters used by native GPUI.
    let mut r = request("A");
    r.text.wrap_width = None;
    r.bounds = [10., 12., 100., 80.];
    let shaped = shape_text(&WindowTextSystem::new(system), &r.text, gpui::white())?;
    let layout = &shaped[0].unwrapped_layout;
    let run = &layout.runs[0];
    let glyph = &run.glyphs[0];
    let baseline = (r.text.line_height - f32::from(layout.ascent) - f32::from(layout.descent)) / 2.
        + f32::from(layout.ascent);
    let round = |x: f32| (x.abs() - 0.5).ceil().copysign(x);
    for dpr in [1., 1.25, 2.] {
        r.device_scale = dpr;
        let x = round((r.bounds[0] + f32::from(glyph.position.x)) * dpr * 4.) / 4.;
        let y = round((r.bounds[1] + baseline + f32::from(glyph.position.y)) * dpr);
        let p = RenderGlyphParams {
            font_id: run.font_id,
            glyph_id: glyph.id,
            font_size: px(r.text.font_size),
            subpixel_variant: point((x.fract() * 4.) as u8, 0),
            scale_factor: dpr,
            is_emoji: false,
            subpixel_rendering: false,
            dilation: raw.glyph_dilation_for_color(gpui::white()),
        };
        let b = raw.glyph_raster_bounds(&p).map_err(|e| e.to_string())?;
        let (size, bytes) = raw.rasterize_glyph(&p, b).map_err(|e| e.to_string())?;
        assert_eq!(bytes.len(), (size.width.0 * size.height.0) as usize);
        let mut work = 32_000_000;
        let pixels = bridge
            .rasterize(&r, bridge.epoch(), &cancel, 32 * 1024 * 1024, &mut work)?
            .unwrap();
        let mut expected = vec![0; pixels.premul_rgba.len()];
        for gy in 0..size.height.0 {
            for gx in 0..size.width.0 {
                let dx = x.trunc() as i32 + b.origin.x.0 + gx - pixels.origin[0];
                let dy = y as i32 + b.origin.y.0 + gy - pixels.origin[1];
                if dx >= 0 && dy >= 0 && dx < pixels.width as i32 && dy < pixels.height as i32 {
                    let v = bytes[(gy * size.width.0 + gx) as usize];
                    let i = (dy as usize * pixels.width as usize + dx as usize) * 4;
                    expected[i..i + 4].fill(v);
                }
            }
        }
        assert_eq!(
            pixels.premul_rgba, expected,
            "native grayscale reference at DPR {dpr}"
        );
        println!("native_glyph_reference dpr={dpr}: all pixels match");
    }
    // The exact pair is Send + Sync; native work and font epoch rejection work
    // off the UI thread without holding an App, Window or Platform Rc.
    let worker = bridge.clone();
    let r = request("Worker 中文 👩‍💻");
    let result = std::thread::spawn(move || {
        let mut work = 32_000_000;
        worker.rasterize(
            &r,
            worker.epoch(),
            &AtomicBool::new(false),
            32 * 1024 * 1024,
            &mut work,
        )
    })
    .join()
    .map_err(|_| "worker panic")??;
    assert!(result.unwrap().glyphs > 0);
    let epoch = bridge.epoch();
    bridge.invalidate();
    let mut work = 32_000_000;
    assert!(
        bridge
            .rasterize(
                &request("stale"),
                epoch,
                &cancel,
                32 * 1024 * 1024,
                &mut work
            )
            .is_err()
    );
    assert!(
        bridge
            .rasterize(
                &request("fresh"),
                bridge.epoch(),
                &cancel,
                32 * 1024 * 1024,
                &mut work
            )?
            .is_some()
    );
    assert!(Arc::strong_count(&bridge) >= 1);
    println!("worker same-backend + stale font epoch rejection: passed");
    Ok(())
}
