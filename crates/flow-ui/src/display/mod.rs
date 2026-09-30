//! GPUI adapter for retained display objects, separate from graph-node UI components.
//! Vector tessellation supports affine transforms and nested rounded frame clipping.
mod clip;
pub(crate) mod composite;
mod diagnostics;
mod gradient;
mod image;
mod rect_clip;
pub use gradient::{
    GradientCacheStats, GradientRenderIssue, clear_gradient_cache, gradient_cache_stats,
    gradient_render_issues, take_gradient_render_issues,
};
mod text;
mod vector;
pub use diagnostics::*;
use gpui::{prelude::*, *};
use gpui_flow_core::Viewport;
use gpui_flow_scene::{self as scene, ClipRegion, DisplayObject, Paint, Shape};

#[derive(Clone, PartialEq)]
pub struct RenderObject {
    pub object: DisplayObject,
    pub screen_transform: scene::Affine,
    pub viewport_transform: scene::Affine,
    pub opacity: f32,
    pub clips: Vec<ClipRegion>,
}
impl RenderObject {
    pub fn from_paint_item(item: &scene::PaintItem<'_>, viewport: Viewport) -> Self {
        let view = scene::Affine::translation(viewport.pan.x, viewport.pan.y)
            * scene::Affine::scale(viewport.zoom, viewport.zoom);
        Self {
            object: item.object.clone(),
            screen_transform: view * item.world_transform,
            viewport_transform: view,
            opacity: item.opacity,
            clips: item.clips.clone(),
        }
    }
    pub fn diagnostics(&self) -> Vec<RenderIssue> {
        let mut issues = vec![];
        let mut report = |feature| {
            issues.push(RenderIssue {
                object: self.object.id.to_string(),
                feature,
            })
        };
        if self.object.style.blend_mode != scene::BlendMode::PassThrough
            || self.object.mask != scene::MaskMode::None
            || (self.object.style.opacity < 1.0
                && (self.object.shape.allows_children()
                    || self.object.style.fills().count() + self.object.style.strokes().count() > 1))
        {
            report(UnsupportedFeature::CompositingDomain);
        }
        if self.object.style.has_layers()
            && matches!(
                self.object.shape,
                Shape::Text(_) | Shape::Image(_) | Shape::Group(_)
            )
        {
            report(UnsupportedFeature::LayeredPaint);
        }
        if self.object.style.strokes().any(|stroke| {
            stroke.uses_advanced_geometry() && self.object.shape.stroke_mesh(stroke).is_err()
        }) {
            report(UnsupportedFeature::StrokeGeometry);
        }
        let unsupported = |p: &Paint| !matches!(p, Paint::Solid { .. });
        if matches!(self.object.shape, Shape::Text(_) | Shape::Image(_))
            && (self.object.style.fill.as_ref().is_some_and(unsupported)
                || self
                    .object
                    .style
                    .stroke
                    .as_ref()
                    .is_some_and(|s| unsupported(&s.paint)))
        {
            report(UnsupportedFeature::GradientPaint);
        }
        let transformed = !rect_clip::supports_transform(self.screen_transform);
        match self.object.shape {
            Shape::Text(_) => {
                if transformed {
                    report(UnsupportedFeature::TransformedText);
                }
                if rect_clip::plan(self).is_err() {
                    report(UnsupportedFeature::ClippedText);
                }
            }
            Shape::Image(_) => {
                if transformed {
                    report(UnsupportedFeature::TransformedImage);
                }
                if rect_clip::plan(self).is_err() {
                    report(UnsupportedFeature::ClippedImage);
                }
            }
            _ => {}
        }
        issues
    }
}
/// One independently renderable retained object. Use SceneView for picking, tree dispatch and editing.
#[derive(IntoElement)]
pub struct DisplayElement {
    pub object: RenderObject,
}
impl DisplayElement {
    pub fn new(object: RenderObject) -> Self {
        Self { object }
    }
    /// Supply a lifecycle-tracked image; Pending/Failed/Cancelled never fall
    /// back to another untracked load. The wrapper preserves this type's legacy
    /// public `DisplayElement { object }` construction shape.
    pub fn with_image(
        self,
        image: crate::resources::SceneImagePresentation,
    ) -> PreparedDisplayElement {
        PreparedDisplayElement {
            object: self.object,
            image,
        }
    }
}
/// Runtime-only resource presentation, separate from the legacy DisplayElement.
#[derive(IntoElement)]
pub struct PreparedDisplayElement {
    object: RenderObject,
    image: crate::resources::SceneImagePresentation,
}
impl RenderOnce for DisplayElement {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        render_display(self.object, None)
    }
}
impl RenderOnce for PreparedDisplayElement {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        render_display(self.object, Some(self.image))
    }
}
fn render_display(
    object: RenderObject,
    image: Option<crate::resources::SceneImagePresentation>,
) -> AnyElement {
    let issues = object.diagnostics();
    if issues.iter().any(|i| {
        matches!(
            i.feature,
            UnsupportedFeature::LayeredPaint
                | UnsupportedFeature::CompositingDomain
                | UnsupportedFeature::StrokeGeometry
                | UnsupportedFeature::TransformedText
                | UnsupportedFeature::TransformedImage
                | UnsupportedFeature::ClippedText
                | UnsupportedFeature::ClippedImage
        )
    }) {
        return div().into_any_element();
    }
    match &object.object.shape {
        Shape::Text(text) => text::text_element(text, &object),
        Shape::Image(source) => image::image_element(source, &object, image),
        _ => canvas(
            |_, _, _| (),
            move |bounds, _, window, cx| vector::paint_vector(&object, bounds.origin, window, cx),
        )
        .absolute()
        .size_full()
        .into_any_element(),
    }
}

#[cfg(test)]
mod resource_compatibility_tests {
    use super::*;
    #[::core::prelude::v1::test]
    fn legacy_display_element_literal_wraps_without_changing_the_render_object() {
        let object = RenderObject {
            object: DisplayObject::new(
                "image",
                scene::display::Image::new("local.png", scene::Size::new(20., 30.)),
            ),
            screen_transform: scene::Affine::IDENTITY,
            viewport_transform: scene::Affine::IDENTITY,
            opacity: 1.,
            clips: vec![],
        };
        let expected = object.clone();
        // Existing downstream literal construction remains source-compatible.
        let element = DisplayElement { object };
        let prepared = element.with_image(crate::resources::SceneImagePresentation::Pending);
        assert!(prepared.object == expected);
        assert!(matches!(
            prepared.image,
            crate::resources::SceneImagePresentation::Pending
        ));
    }
}

mod registry;
pub use registry::*;
