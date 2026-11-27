use gpui::{prelude::*, *};
struct CreativeRoot {
    content: AnyView,
}
impl Render for CreativeRoot {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.content.clone())
    }
}
use gpui_flow_creative::*;
use std::{path::PathBuf, sync::Arc};
fn main() {
    let args: Vec<_> = std::env::args()
        .skip(1)
        .filter(|a| a != "--trace")
        .collect();
    let (root, file, demo) = match args.as_slice() {
        [] => (
            ProjectStore::default_root().unwrap_or_else(|e| {
                eprintln!("{e}");
                std::process::exit(2)
            }),
            None,
            false,
        ),
        [flag, path] if flag == "--projects-dir" => (PathBuf::from(path), None, false),
        [flag, path] if flag == "--file" => (PathBuf::new(), Some(PathBuf::from(path)), false),
        [flag] if flag == "--demo" => (PathBuf::new(), None, true),
        _ => {
            eprintln!(
                "Usage: flow-creative [--projects-dir PATH | --file PATH | --demo] [--trace]"
            );
            std::process::exit(2)
        }
    };
    let (application, text_backend) =
        gpui_flow::GpuiTextRasterizer::application(gpui_kit::platform::current_platform(false));
    application
        .with_assets(gpui_kit::assets::AllAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            initialize_creative_language(cx);
            install_creative_text_export_backend(text_backend.clone(), cx)
                .expect("text exporter must share the application font backend");
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
                    window.set_window_title("Flow Creative");
                    gpui_kit::component::Theme::change(
                        gpui_kit::component::ThemeMode::Light,
                        Some(window),
                        cx,
                    );
                    let content: AnyView = if demo || file.is_some() {
                        let registry = Arc::new(standard_registry());
                        let document = if let Some(path) = &file {
                            let value = std::fs::read_to_string(path).unwrap_or_else(|e| {
                                eprintln!("Open failed: {e}");
                                std::process::exit(2)
                            });
                            registry.parse_document(&value).unwrap_or_else(|e| {
                                eprintln!("Invalid document: {e}");
                                std::process::exit(2)
                            })
                        } else {
                            registry
                                .map_records(sample_records(), LayoutPolicy::RequirePositions)
                                .expect("valid demo")
                        };
                        cx.new(|cx| {
                            let mut view = CreativeCanvas::new(
                                registry,
                                document,
                                demo_renderers(CreativeAssets::demo()),
                                window,
                                cx,
                            )
                            .expect("validated document");
                            view.set_assets(CreativeAssets::demo(), cx);
                            view.set_document_path(file, cx);
                            view
                        })
                        .into()
                    } else {
                        cx.new(|cx| CreativeWorkbench::new(root, window, cx)).into()
                    };
                    cx.new(|_| CreativeRoot { content })
                },
            )
            .expect("open creative workbench");
            cx.activate(true);
        });
}
