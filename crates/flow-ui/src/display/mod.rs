//! GPUI adapter for retained display objects, separate from graph-node UI components.
//! Vector tessellation supports affine transforms and nested rounded frame clipping.
mod clip;
mod diagnostics;
mod gradient;
mod image;
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
        if self.object.style.has_layers()
            && matches!(
                self.object.shape,
                Shape::Text(_) | Shape::Image(_) | Shape::Group(_)
            )
        {
            report(UnsupportedFeature::LayeredPaint);
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
        let m = self.screen_transform;
        let transformed = m.b.abs() > 0.0001
            || m.c.abs() > 0.0001
            || m.a <= 0.
            || m.d <= 0.
            || (m.a - m.d).abs() > 0.0001;
        match self.object.shape {
            Shape::Text(_) => {
                if transformed {
                    report(UnsupportedFeature::TransformedText);
                }
                if !self.clips.is_empty() {
                    report(UnsupportedFeature::ClippedText);
                }
            }
            Shape::Image(_) => {
                if transformed {
                    report(UnsupportedFeature::TransformedImage);
                }
                if !self.clips.is_empty() {
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
}
impl RenderOnce for DisplayElement {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let issues = self.object.diagnostics();
        if issues.iter().any(|i| {
            matches!(
                i.feature,
                UnsupportedFeature::LayeredPaint
                    | UnsupportedFeature::TransformedText
                    | UnsupportedFeature::TransformedImage
                    | UnsupportedFeature::ClippedText
                    | UnsupportedFeature::ClippedImage
            )
        }) {
            return div().into_any_element();
        }
        match &self.object.object.shape {
            Shape::Text(text) => text::text_element(text, &self.object),
            Shape::Image(image) => image::image_element(image, &self.object),
            _ => {
                let object = self.object;
                canvas(
                    |_, _, _| (),
                    move |bounds, _, window, cx| {
                        vector::paint_vector(&object, bounds.origin, window, cx)
                    },
                )
                .absolute()
                .size_full()
                .into_any_element()
            }
        }
    }
}

mod registry;
pub use registry::*;
