//! Emit the actual per-corner Rect geometry as an SVG gallery.
use gpui_flow_scene::display::Rect;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="540" height="350" viewBox="0 0 540 350"><rect width="540" height="350" fill="#f6f8fc"/>"##
    );
    for (index, (label, rect)) in [
        (
            "Scalar radius (unchanged)",
            Rect::new(180., 100.).rounded(16.),
        ),
        (
            "Independent diagonal corners",
            Rect::new(180., 100.).with_corners([40., 0., 20., 0.]),
        ),
        (
            "Four distinct radii",
            Rect::new(180., 100.).with_corners([10., 20., 30., 40.]),
        ),
        (
            "Clamped to half the short side",
            Rect::new(180., 100.).with_corners([200., 0., 150., 0.]),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let x = 45 + index % 2 * 270;
        let y = 25 + index / 2 * 170;
        let data = rect.to_path()?.to_svg()?;
        println!(
            r##"<g transform="translate({x} {y})"><path d="{data}" fill="#6689df" stroke="#223b6d" stroke-width="2"/><text x="90" y="128" text-anchor="middle" font-family="sans-serif" font-size="13" fill="#223b6d">{label}</text></g>"##
        );
    }
    println!("</svg>");
    Ok(())
}
