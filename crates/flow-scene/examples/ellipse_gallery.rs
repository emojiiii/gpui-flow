//! Emit a geometry-generated SVG gallery without a window or native backend.
use gpui_flow_scene::display::Ellipse;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="660" height="300" viewBox="0 0 660 300"><rect width="660" height="300" fill="#f6f8fc"/>"##
    );
    let samples = [
        ("Full oval", Ellipse::new(120., 80.), true),
        ("Open arc", Ellipse::new(120., 80.).arc(-45., 160.), false),
        ("Pie sector", Ellipse::new(120., 80.).sector(0., 120.), true),
        ("Donut", Ellipse::new(120., 80.).ring(0.55), true),
        (
            "Ring sector",
            Ellipse::new(120., 80.).sector(-90., 90.).ring(0.55),
            true,
        ),
        (
            "Clockwise wrap",
            Ellipse::new(120., 80.).sector(90., 0.),
            true,
        ),
    ];
    for (index, (label, ellipse, fill)) in samples.into_iter().enumerate() {
        let x = 45 + index % 3 * 220;
        let y = 25 + index / 3 * 145;
        let data = ellipse.to_path()?.to_svg()?;
        let fill = if fill { "#6689df" } else { "none" };
        println!(
            r##"<g transform="translate({x} {y})"><path d="{data}" fill="{fill}" fill-rule="nonzero" stroke="#223b6d" stroke-width="2"/><text x="60" y="110" text-anchor="middle" font-family="sans-serif" font-size="14" fill="#223b6d">{label}</text></g>"##
        );
    }
    println!("</svg>");
    Ok(())
}
