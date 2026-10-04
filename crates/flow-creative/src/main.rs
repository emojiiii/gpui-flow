use gpui::{AppContext, Bounds, WindowBounds, WindowOptions, px, size};
use gpui_flow_creative::*;
use std::{path::PathBuf, sync::Arc};
fn main() {
    let args: Vec<_> = std::env::args()
        .skip(1)
        .filter(|a| a != "--trace")
        .collect();
    let path = match args.as_slice() {
        [] => None,
        [flag, path] if flag == "--file" => Some(PathBuf::from(path)),
        _ => {
            eprintln!("Usage: flow-creative [--file PATH] [--trace]");
            std::process::exit(2);
        }
    };
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(1400.), px(900.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                cx,
                move |window, cx| {
                    window.set_window_title("Flow Creative · 原生创作画布");
                    gpui_kit::component::Theme::change(
                        gpui_kit::component::ThemeMode::Light,
                        Some(window),
                        cx,
                    );
                    let registry = Arc::new(standard_registry());
                    let mut document = registry
                        .map_records(sample_records(), LayoutPolicy::RequirePositions)
                        .expect("valid sample");
                    document.viewport.zoom = 0.62;
                    document.viewport.pan = gpui_flow_core::Point::new(15., 20.);
                    cx.new(|cx| {
                        let mut view = CreativeCanvas::new(
                            registry,
                            document,
                            demo_renderers(CreativeAssets::demo()),
                            window,
                            cx,
                        )
                        .expect("valid creative view");
                        view.set_document_path(path, cx);
                        view
                    })
                },
            )
            .expect("open creative canvas");
            cx.activate(true);
        });
}
