//! Real scene semantics between two ordinary host controls. --trace logs routed
//! semantic events; the last button dumps the actual last GPUI accessibility tree.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, FlowTheme, SceneAccessibility, SceneView, SceneViewEvent,
    scene::{Affine, Color, DisplayObject, Scene, Size, Style, display},
};
struct Lab {
    scene: Entity<SceneView>,
    readonly: bool,
    _events: Subscription,
}
impl Lab {
    fn new(cx: &mut Context<Self>) -> Self {
        let mut document = Scene::new();
        let mut rectangle = DisplayObject::new("rectangle", display::Rect::new(160., 90.))
            .with_transform(Affine::translation(50., 60.))
            .with_style(Style::fill(Color::hex(0x3e829e)));
        rectangle.name = "Blue rectangle".into();
        document.insert(rectangle, None, None).unwrap();
        let mut locked = DisplayObject::new("locked", display::Rect::new(160., 90.))
            .with_transform(Affine::translation(260., 60.))
            .with_style(Style::fill(Color::hex(0x706078)));
        locked.name = "Locked rectangle".into();
        locked.locked = true;
        document.insert(locked, None, None).unwrap();
        let mut text = display::Text::new("Editable scene text", Size::new(360., 80.));
        text.font_size = 26.;
        document
            .insert(
                DisplayObject::new("text", text)
                    .with_transform(Affine::translation(50., 220.))
                    .with_style(Style::fill(Color::WHITE)),
                None,
                None,
            )
            .unwrap();
        let scene = cx.new(|cx| {
            let mut scene = SceneView::new(document, cx).unwrap();
            scene.set_accessibility(Some(SceneAccessibility::new("Example drawing")), cx);
            scene
        });
        let trace = std::env::args().any(|arg| arg == "--trace");
        let events = cx.subscribe(&scene, move |_, _, event, _| {
            if trace {
                match event {
                    SceneViewEvent::Semantic(event) => eprintln!(
                        "semantic: {:?} target={} input={:?} prevented={}",
                        event.kind,
                        event.target,
                        event.input,
                        event.default_prevented()
                    ),
                    SceneViewEvent::EditorTool(event) => eprintln!("tool: {event:?}"),
                    _ => {}
                }
            }
        });
        Self {
            scene,
            readonly: false,
            _events: events,
        }
    }
}
impl Render for Lab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = FlowTheme::default();
        div().size_full().flex().flex_col().bg(rgb(theme.background)).text_color(rgb(theme.foreground))
            .child(div().p_3().flex().gap_3().items_center()
                .child(ControlButton::new("before-scene", "Before scene · toggle read-only").on_click(cx.listener(|this, _, _, cx| {
                    this.readonly = !this.readonly;
                    let editable = !this.readonly;
                    this.scene.update(cx, |scene, cx| scene.set_editable(editable, cx));
                    cx.notify();
                })))
                .child(if self.readonly { "Read-only: focus works; Text activation disabled" } else { "Editable: Enter or AT Click opens Text" }))
            .child(div().flex_1().min_h_0().child(self.scene.clone()))
            .child(div().p_3().flex().gap_3().items_center()
                .child(ControlButton::new("after-scene", "After scene · dump a11y tree").on_click(|_, window, _| {
                    eprintln!("GPUI a11y active={} tree={}", window.is_a11y_active(),
                        window.debug_a11y_tree_json().unwrap_or_else(|| "unavailable; connect platform assistive technology".into()));
                }))
                .child("Tab / Shift-Tab: enter, targets, exit · locked node is skipped · Space pans · Done / Cancel returns"))
    }
}
fn main() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(|cx| {
            gpui_kit::init(cx);
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(1060.), px(700.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    window.set_window_title("gpui-flow · Scene accessibility");
                    cx.new(Lab::new)
                },
            )
            .unwrap();
            cx.activate(true);
        });
}
