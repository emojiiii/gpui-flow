use super::RenderObject;
use gpui::{prelude::*, *};
use gpui_flow_scene::display::{Image, ImageFit};
pub(super) fn image_element(image: &Image, object: &RenderObject) -> AnyElement {
    let m = object.screen_transform;
    let fit = match image.fit {
        ImageFit::Contain => ObjectFit::Contain,
        ImageFit::Cover => ObjectFit::Cover,
        ImageFit::Fill => ObjectFit::Fill,
    };
    // Local resource references only: loading a scene never initiates network requests.
    if image.source.starts_with("http:") || image.source.starts_with("https:") {
        return div()
            .absolute()
            .left(px(m.tx))
            .top(px(m.ty))
            .text_color(rgb(0xf5b971))
            .child("Remote image: provide a host asset loader")
            .into_any_element();
    }
    div()
        .absolute()
        .left(px(m.tx))
        .top(px(m.ty))
        .w(px(image.size.width * m.a))
        .h(px(image.size.height * m.d))
        .overflow_hidden()
        .opacity(object.opacity)
        .child(
            img(std::path::PathBuf::from(&image.source))
                .size_full()
                .object_fit(fit),
        )
        .into_any_element()
}
