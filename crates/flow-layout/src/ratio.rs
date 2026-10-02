//! Leafer-style uniform resize coupling, deliberately separate from CSS sizing.
//!
//! Pinned leafer-in 3eb445bf, flow/layout/{x,y}/grow.ts, resizeWidth/Height:
//! allocate/clamp the requested axis, then scaleResize(scale, scale) if locked.
//! Taffy 0.13.0's generate_anonymous_flex_items resolves definite size before
//! flex growth; determine_used_cross_size can stretch without changing main.
//! Neither is a lock on two existing dimensions. Reuse Taffy for main allocation
//! and use this bounded coupling rather than adding a second flex solver.
use crate::{AlignItems, FlowError, FlowItem, FlowLayout, FlowStyle, ItemBox, invalid};
use gpui_flow_scene::{Bounds, DisplayId, DisplayObject, Scene, Shape, Size};

pub(super) fn validate(
    layout: &FlowLayout,
    scene: &Scene,
    id: &DisplayId,
    parent: &FlowStyle,
    item: &FlowItem,
) -> Result<(), FlowError> {
    if !item.lock_ratio {
        return Ok(());
    }
    let object = &scene.get(id).expect("validated child").object;
    let bounds = object
        .shape
        .local_bounds()
        .ok_or_else(|| invalid(id, "lock_ratio requires an intrinsic finite positive size"))?;
    if !bounds.size.is_positive() {
        return Err(invalid(
            id,
            "lock_ratio requires an intrinsic finite positive size",
        ));
    }
    if !matches!(
        object.shape,
        Shape::Rect(_)
            | Shape::Ellipse(_)
            | Shape::Image(_)
            | Shape::Canvas(_)
            | Shape::Box(_)
            | Shape::Frame(_)
    ) {
        return Err(invalid(
            id,
            "lock_ratio supports Rect/Ellipse/Image/Box/Frame only; text and custom resize policies are unsupported",
        ));
    }
    let row = parent.direction.is_row();
    let (main_min, main_max, cross_min, cross_max) = if row {
        (
            item.min_width,
            item.max_width,
            item.min_height,
            item.max_height,
        )
    } else {
        (
            item.min_height,
            item.max_height,
            item.min_width,
            item.max_width,
        )
    };
    if item.align_self.unwrap_or(parent.align_items) == AlignItems::Stretch
        || cross_min.is_some()
        || cross_max.is_some()
    {
        return Err(invalid(
            id,
            "lock_ratio requires a free cross axis; stretch and cross-axis constraints are unsupported",
        ));
    }
    let allocated =
        item.grow > 0.0 || item.basis.is_some() || main_min.is_some() || main_max.is_some();
    if allocated {
        if parent.wrap {
            return Err(invalid(
                id,
                "lock_ratio size allocation with wrapping is unsupported; upstream captures cross-line metrics before grow",
            ));
        }
        if parent.width_sizing.is_content() || parent.height_sizing.is_content() {
            return Err(invalid(
                id,
                "lock_ratio size allocation requires a fixed-size parent",
            ));
        }
        if layout.containers.get(id).is_some_and(|style| {
            style.width_sizing.is_content() || style.height_sizing.is_content()
        }) {
            return Err(invalid(
                id,
                "lock_ratio cannot resize a content-sized container",
            ));
        }
        let t = object.transform;
        if t.a <= 0.0 || t.d <= 0.0 || t.b != 0.0 || t.c != 0.0 {
            return Err(invalid(
                id,
                "lock_ratio resizing requires a positive axis-aligned transform",
            ));
        }
    }
    Ok(())
}

/// Allocate in parent-local units, then compare in intrinsic units before
/// coupling the other dimension. A transformed f32 size may round one ULP away
/// from the requested slot, even when division yields the exact same intrinsic
/// size. Scaling again from that rounded slot would drift forever on reflow.
pub(super) fn resize(
    object: &mut DisplayObject,
    bounds: Bounds,
    row: bool,
    main: f64,
    mode: ItemBox,
) -> Result<(), FlowError> {
    let id = &object.id;
    let (old_main, old_cross) = if row {
        (bounds.size.width, bounds.size.height)
    } else {
        (bounds.size.height, bounds.size.width)
    };
    if !old_main.is_finite() || old_main <= 0.0 || !old_cross.is_finite() || old_cross <= 0.0 {
        return Err(invalid(
            id,
            "lock_ratio requires a finite positive transformed size",
        ));
    }
    let main = main as f32;
    if !main.is_finite() || main <= 0.0 {
        return Err(invalid(
            id,
            "lock_ratio allocation must remain finite and positive on both axes",
        ));
    }
    if main == old_main {
        return Ok(());
    }
    if mode == ItemBox::Stroke
        && object
            .style
            .strokes()
            .any(|s| s.width > 0.0 && s.paint.is_visible())
    {
        return Err(invalid(
            id,
            "resizing visible stroke item boxes is unsupported; use box mode or remove the size allocation",
        ));
    }
    let t = object.transform;
    // Configuration validation ensures this whenever a size owner is present.
    if t.a <= 0.0 || t.d <= 0.0 || t.b != 0.0 || t.c != 0.0 {
        return Err(invalid(
            id,
            "lock_ratio resizing requires a positive axis-aligned transform",
        ));
    }
    let size = match &mut object.shape {
        Shape::Rect(shape) => &mut shape.size,
        Shape::Ellipse(shape) => &mut shape.size,
        Shape::Image(shape) => &mut shape.size,
        Shape::Canvas(shape) => &mut shape.size,
        Shape::Box(shape) => &mut shape.size,
        Shape::Frame(shape) => &mut shape.size,
        _ => unreachable!("validated lock_ratio shape"),
    };
    let (old_main, old_cross, scale_main, scale_cross) = if row {
        (size.width, size.height, t.a, t.d)
    } else {
        (size.height, size.width, t.d, t.a)
    };
    let main = main / scale_main;
    if main == old_main {
        return Ok(());
    }
    let cross = (f64::from(old_cross) * f64::from(main) / f64::from(old_main)) as f32;
    let next = if row {
        Size::new(main, cross)
    } else {
        Size::new(cross, main)
    };
    if !next.is_positive() || !(cross * scale_cross).is_finite() || cross * scale_cross <= 0.0 {
        return Err(invalid(
            id,
            "lock_ratio allocation must remain finite and positive on both axes",
        ));
    }
    *size = next;
    Ok(())
}
