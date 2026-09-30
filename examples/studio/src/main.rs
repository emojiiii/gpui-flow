use gpui::{App, AppContext, Bounds, WindowBounds, WindowOptions, px, size};
use gpui_flow::FlowCanvas;
fn main() {
    let mut args = std::env::args().skip(1);
    let document_path = match args.next().as_deref() {
        None => None,
        Some("--file") => {
            let Some(path) = args.next() else {
                eprintln!("Usage: flow-studio [--file PATH]");
                std::process::exit(2);
            };
            Some(std::path::PathBuf::from(path))
        }
        _ => {
            eprintln!("Usage: flow-studio [--file PATH]");
            std::process::exit(2);
        }
    };
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx: &mut App| {
            gpui_kit::init(cx);
            let bounds = Bounds::centered(None, size(px(1440.), px(900.)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                move |window, cx| {
                    window.set_window_title("gpui-flow Studio");
                    gpui_kit::component::Theme::change(
                        gpui_kit::component::ThemeMode::Dark,
                        Some(window),
                        cx,
                    );
                    cx.new(|cx| {
                        let mut canvas = FlowCanvas::demo(cx);
                        canvas.document_path = document_path;
                        canvas
                    })
                },
            )
            .expect("open studio window");
            cx.activate(true);
        });
}
