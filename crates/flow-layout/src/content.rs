//! Content bounds are a layout view of padding, never a resize of children.
use crate::{ContentBoundsPolicy, FlowError, FlowLayout, FlowStyle, invalid};
use gpui_flow_scene::{Bounds, DisplayId, Point, Scene, Size};

pub(crate) struct ContentBox {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
impl ContentBox {
    fn bounds(&self) -> Bounds {
        Bounds::new(
            Point::new(self.x as f32, self.y as f32),
            Size::new(self.width as f32, self.height as f32),
        )
    }
}

/// Resolve in f64 so an overflowing intermediate cannot silently become an
/// infinite f32 box. `LeaferShrink` follows copyAndSpread(..., true) + unsign.
pub(crate) fn resolve(
    id: &DisplayId,
    size: Size,
    style: &FlowStyle,
) -> Result<ContentBox, FlowError> {
    let mut x = f64::from(style.padding.left);
    let mut y = f64::from(style.padding.top);
    let mut width = f64::from(size.width) - x - f64::from(style.padding.right);
    let mut height = f64::from(size.height) - y - f64::from(style.padding.bottom);
    match style.content_bounds_policy {
        ContentBoundsPolicy::Clamp => {
            width = width.max(0.0);
            height = height.max(0.0);
        }
        ContentBoundsPolicy::LeaferShrink => {
            if width < 0.0 {
                x += width;
                width = -width;
            }
            if height < 0.0 {
                y += height;
                height = -height;
            }
        }
    }
    let content = ContentBox {
        x,
        y,
        width,
        height,
    };
    if !content.bounds().is_valid() {
        return Err(invalid(id, "padded content bounds overflow"));
    }
    Ok(content)
}

impl FlowLayout {
    /// The same parent-local content box consumed by percentage resolution,
    /// wrapping and placement. Zero padding returns the intrinsic box exactly;
    /// clearing padding therefore has shrinkContentCancel semantics without a
    /// retained alias or stale content-box cache. Does not change Scene bounds.
    pub fn content_bounds(&self, scene: &Scene, id: &DisplayId) -> Result<Bounds, FlowError> {
        self.validate(scene)?;
        let style = self
            .containers
            .get(id)
            .ok_or_else(|| invalid(id, "not a flow container"))?;
        let size = scene
            .get(id)
            .expect("validated container")
            .object
            .shape
            .local_bounds()
            .expect("Box/Frame")
            .size;
        resolve(id, size, style).map(|content| content.bounds())
    }
}
