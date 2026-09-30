use gpui::{App, AppContext, Bounds, WindowBounds, WindowOptions, px, size};
use gpui_flow::FlowCanvas;
fn main() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(|cx: &mut App| {
            gpui_kit::init(cx);
            let bounds = Bounds::centered(None, size(px(1440.), px(900.)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                |_, cx| cx.new(FlowCanvas::demo),
            )
            .expect("open studio window");
            cx.activate(true);
        });
}
