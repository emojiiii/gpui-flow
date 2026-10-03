//! Owned-prefix validation. Sites belong to the exact immutable DomainSource
//! snapshot; they are never reused across requests or detached from its inputs.
use crate::{BackgroundBlurContext as Context, CompositeError, DomainNode, DomainSource};
use gpui_flow_scene::{BlendMode, DisplayId, EraserMode, MaskMode, Shape};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Owner {
    Canvas,
    Transparent(DisplayId),
}
#[derive(Clone, Debug)]
pub(crate) struct Site {
    pub owner: Owner,
    /// Ordered node-entry position in this immutable source, before own paint.
    pub entry: usize,
}
pub(crate) fn active(node: &DomainNode) -> bool {
    node.object.visible
        && node.object.style.opacity > 0.
        && node.object.eraser != EraserMode::Path
        && node.object.mask != MaskMode::Path
        && (node.object.style.has_background_blur() || node.children.iter().any(active))
}
fn refusal(node: &DomainNode, reason: Context) -> CompositeError {
    CompositeError::UnsupportedBackgroundBlur {
        object: node.object.id.clone(),
        reason,
    }
}
pub(crate) fn target(node: &gpui_flow_scene::DisplayObject) -> Result<(), CompositeError> {
    if !node.visible
        || node.style.opacity == 0.
        || node.eraser == EraserMode::Path
        || node.mask == MaskMode::Path
        || !node.style.has_background_blur()
    {
        return Ok(());
    }
    let reason = if !matches!(
        node.shape,
        Shape::Rect(_)
            | Shape::Ellipse(_)
            | Shape::Polygon(_)
            | Shape::Star(_)
            | Shape::Path(_)
            | Shape::Pen(_)
            | Shape::Box(_)
            | Shape::Frame(_)
            | Shape::Line(_)
    ) {
        Some(Context::TargetShape)
    } else if !matches!(
        node.style.blend_mode,
        BlendMode::Normal | BlendMode::PassThrough
    ) {
        Some(Context::ArtisticBlend)
    } else {
        None
    };
    if let Some(reason) = reason {
        Err(CompositeError::UnsupportedBackgroundBlur {
            object: node.id.clone(),
            reason,
        })
    } else {
        Ok(())
    }
}
#[derive(Clone, Default)]
struct ContextState {
    owner: Option<Owner>,
    eraser_owner: bool,
    pixel_source: bool,
    mask_run: bool,
    effects: bool,
    background: bool,
    root_eraser: bool,
}
pub(crate) fn validate(source: &DomainSource) -> Result<Vec<(DisplayId, Site)>, CompositeError> {
    validate_inner(source, None)
}
pub(crate) fn validate_cancelled(
    source: &DomainSource,
    cancelled: &AtomicBool,
) -> Result<Vec<(DisplayId, Site)>, CompositeError> {
    validate_inner(source, Some(cancelled))
}
fn validate_inner(
    source: &DomainSource,
    cancelled: Option<&AtomicBool>,
) -> Result<Vec<(DisplayId, Site)>, CompositeError> {
    fn visit(
        node: &DomainNode,
        mut context: ContextState,
        index: &mut usize,
        sites: &mut Vec<(DisplayId, Site)>,
        cancelled: Option<&AtomicBool>,
    ) -> Result<(), CompositeError> {
        if cancelled.is_some_and(|c| c.load(Ordering::Relaxed)) {
            return Err(CompositeError::Superseded);
        }
        let o = &node.object;
        // OwnedCanvas's private runtime document has the deliberately empty
        // ID and constructor-owned default style. Canonical nodes (including
        // hidden/Path values) validate fully; Scene forbids authored empty IDs.
        if !o.id.as_str().is_empty() {
            o.validate().map_err(CompositeError::InvalidGeometry)?;
        }
        let entry = *index;
        *index += 1;
        if !o.visible
            || o.style.opacity == 0.
            || o.eraser == EraserMode::Path
            || o.mask == MaskMode::Path
        {
            return Ok(());
        }
        target(o)?;
        let erasers = node
            .children
            .iter()
            .any(|c| c.object.eraser != EraserMode::None);
        if o.style.has_background_blur() {
            let reason = if context.root_eraser {
                Some(Context::RootEraser)
            } else if context.mask_run || o.mask != MaskMode::None {
                Some(Context::MaskRun)
            } else if context.eraser_owner
                || context.pixel_source
                || erasers
                || o.eraser == EraserMode::Pixel
            {
                Some(Context::EraserOwner)
            } else if context.background {
                Some(Context::NestedBackground)
            } else if context.effects {
                Some(Context::AncestorEffect)
            } else if o.style.blend_mode == BlendMode::PassThrough
                && o.style.opacity == 1.
                && !node.children.iter().any(active)
                && crate::plan::requires_children_backdrop(node)
            {
                Some(Context::ForegroundBackdrop)
            } else {
                None
            };
            if let Some(reason) = reason {
                return Err(refusal(node, reason));
            }
            let owner = context
                .owner
                .clone()
                .ok_or(CompositeError::UnsupportedExternalBackdrop)?;
            sites.push((o.id.clone(), Site { owner, entry }));
        }
        let effects = o.style.has_visible_shadows()
            || o.style.has_visible_inner_shadows()
            || o.style.has_blur()
            || o.style.has_grayscale();
        context.effects |= effects;
        context.background |= o.style.has_background_blur();
        context.pixel_source |= o.eraser == EraserMode::Pixel;
        // Match PreparedNode::isolated, excluding the background operation's
        // own read. Normal/opacity creates a distinct transparent owner.
        if erasers
            || effects
            || o.style.opacity < 1.
            || o.style.blend_mode != BlendMode::PassThrough
            || o.eraser == EraserMode::Pixel
        {
            context.owner = Some(Owner::Transparent(o.id.clone()));
            context.eraser_owner = erasers;
        }
        let active_masks = crate::plan::has_active_masks(node);
        let mut run = false;
        for child in &node.children {
            run |= active_masks && child.object.mask != MaskMode::None;
            let mut child_context = context.clone();
            child_context.mask_run |= run;
            visit(child, child_context, index, sites, cancelled)?;
        }
        Ok(())
    }
    let mut sites = Vec::new();
    let owned = source.canvas.is_some();
    visit(
        &source.root,
        ContextState {
            owner: owned.then_some(Owner::Canvas),
            root_eraser: owned
                && source
                    .root
                    .children
                    .iter()
                    .any(|c| c.object.eraser != EraserMode::None),
            ..Default::default()
        },
        &mut 0,
        &mut sites,
        cancelled,
    )?;
    Ok(sites)
}
