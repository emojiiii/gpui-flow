//! GPUI adapter for retained display objects, separate from graph-node UI components.
//! Vector tessellation supports affine transforms and nested rounded frame clipping.
mod clip;
pub(crate) mod composite;
pub(crate) mod composite_text;
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
    /// Compatibility constructor. Use the fallible variant for unadmitted views.
    pub fn from_paint_item(item: &scene::PaintItem<'_>, viewport: Viewport) -> Self {
        Self::try_from_paint_item(item, viewport)
            .expect("stroke view must be admitted before native rendering")
    }
    pub fn try_from_paint_item(
        item: &scene::PaintItem<'_>,
        viewport: Viewport,
    ) -> Result<Self, String> {
        if item
            .stroke_camera
            .is_some_and(|camera| camera != viewport.zoom)
        {
            return Err("resolved native stroke camera mismatch".into());
        }
        let view = scene::Affine::translation(viewport.pan.x, viewport.pan.y)
            * scene::Affine::scale(viewport.zoom, viewport.zoom);
        let object = if item.object.style.has_scale_fixed_strokes() {
            let object = item
                .object
                .resolved_strokes(item.stroke_scale, viewport.zoom)?;
            object.validate()?;
            object
        } else {
            item.object.clone()
        };
        Ok(Self {
            object,
            screen_transform: view * item.world_transform,
            viewport_transform: view,
            opacity: item.opacity,
            clips: item.clips.clone(),
        })
    }
    pub(crate) fn native_renderable(&self) -> bool {
        !self
            .diagnostics()
            .iter()
            .any(|issue| rejects_native(&issue.feature))
    }
    pub fn diagnostics(&self) -> Vec<RenderIssue> {
        let mut issues = vec![];
        let mut report = |feature| {
            issues.push(RenderIssue {
                object: self.object.id.to_string(),
                feature,
            })
        };
        if matches!(self.object.shape, Shape::Canvas(_))
            || ((self.opacity < 1.
                || self
                    .object
                    .style
                    .fills()
                    .any(|p| matches!(p,Paint::Solid{color} if color.a<255)))
                && matches!(&self.object.shape,Shape::Text(t) if t.decoration != scene::display::TextDecoration::None))
            || matches!(&self.object.shape, Shape::Image(image)
                if image.source_crop.is_some()
                    || (image.is_rounded() && image.fit == scene::display::ImageFit::Contain))
            || self.object.style.blend_mode != scene::BlendMode::PassThrough
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
        if self.object.style.has_scale_fixed_strokes() {
            report(UnsupportedFeature::UnresolvedStrokeScale);
        }
        if self.object.style.strokes().any(|stroke| {
            stroke.uses_advanced_geometry()
                && self
                    .object
                    .shape
                    .stroke_mesh_with_rule(stroke, self.object.style.fill_rule)
                    .is_err()
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
fn rejects_native(feature: &UnsupportedFeature) -> bool {
    matches!(
        feature,
        UnsupportedFeature::LayeredPaint
            | UnsupportedFeature::CompositingDomain
            | UnsupportedFeature::StrokeGeometry
            | UnsupportedFeature::UnresolvedStrokeScale
            | UnsupportedFeature::TransformedText
            | UnsupportedFeature::TransformedImage
            | UnsupportedFeature::ClippedText
            | UnsupportedFeature::ClippedImage
    )
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
    /// Internal render operation. Keep the public standalone element and
    /// RenderObject construction shapes unchanged; do not strip style layers.
    pub(crate) fn with_paint_phase(self, phase: scene::PaintPhase) -> PhasedDisplayElement {
        PhasedDisplayElement {
            object: self.object,
            phase,
        }
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
            frame_index: None,
        }
    }
}
#[derive(IntoElement)]
pub(crate) struct PhasedDisplayElement {
    object: RenderObject,
    phase: scene::PaintPhase,
}
impl RenderOnce for PhasedDisplayElement {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        render_display(self.object, None, None, self.phase)
    }
}
/// Runtime-only resource presentation, separate from the legacy DisplayElement.
#[derive(IntoElement)]
pub struct PreparedDisplayElement {
    object: RenderObject,
    image: crate::resources::SceneImagePresentation,
    frame_index: Option<usize>,
}
impl PreparedDisplayElement {
    pub(crate) fn selected_frame(mut self, index: usize) -> Self {
        self.frame_index = Some(index);
        self
    }
}
impl RenderOnce for DisplayElement {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        render_display(self.object, None, None, scene::PaintPhase::All)
    }
}
impl RenderOnce for PreparedDisplayElement {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        render_display(
            self.object,
            Some(self.image),
            self.frame_index,
            scene::PaintPhase::All,
        )
    }
}
fn render_display(
    object: RenderObject,
    image: Option<crate::resources::SceneImagePresentation>,
    frame_index: Option<usize>,
    phase: scene::PaintPhase,
) -> AnyElement {
    if !object.native_renderable() {
        return div().into_any_element();
    }
    match &object.object.shape {
        Shape::Text(text) => text::text_element(text, &object),
        Shape::Image(source) => image::image_element(source, &object, image, frame_index),
        _ => canvas(
            |_, _, _| (),
            move |bounds, _, window, cx| {
                vector::paint_vector(&object, bounds.origin, phase, window, cx)
            },
        )
        .absolute()
        .size_full()
        .into_any_element(),
    }
}

#[cfg(test)]
mod resource_compatibility_tests {
    #[::core::prelude::v1::test]
    fn animation_visibility_uses_rejections_not_warning_only_diagnostics() {
        assert!(!super::rejects_native(
            &super::UnsupportedFeature::GradientPaint
        ));
        assert!(super::rejects_native(
            &super::UnsupportedFeature::TransformedImage
        ));
        assert!(super::rejects_native(
            &super::UnsupportedFeature::ClippedImage
        ));
    }

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

#[cfg(test)]
mod overflow_tests;

#[cfg(test)]
mod decoration_tests;

#[cfg(test)]
mod paragraph_style_tests;
