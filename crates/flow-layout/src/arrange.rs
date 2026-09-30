use crate::{
    AlignContent, AlignItems, FlowDirection, FlowError, FlowItem, FlowStyle, JustifyContent,
    invalid,
};
use gpui_flow_scene::{Affine, Bounds, DisplayId, DisplayObject, Scene, Shape};
use std::collections::BTreeMap;

struct Item {
    object: DisplayObject,
    bounds: Bounds,
    input: FlowItem,
    main: f64,
    cross: f64,
    main_max: f64,
    cross_min: f64,
    cross_max: f64,
}
#[derive(Default)]
struct Line {
    items: Vec<Item>,
    main: f64,
    cross: f64,
}

fn linear_bounds(object: &DisplayObject) -> Result<Bounds, FlowError> {
    let local = object.shape.local_bounds().ok_or_else(|| {
        invalid(
            &object.id,
            "participating child has no intrinsic box; exclude Groups with in_flow=false",
        )
    })?;
    let bounds = Affine {
        tx: 0.0,
        ty: 0.0,
        ..object.transform
    }
    .transform_bounds(local);
    if !bounds.is_valid() {
        return Err(invalid(&object.id, "transformed box overflow"));
    }
    Ok(bounds)
}
fn axis(row: bool, width: f64, height: f64) -> (f64, f64) {
    if row {
        (width, height)
    } else {
        (height, width)
    }
}
fn constraints(min: Option<f32>, max: Option<f32>) -> (f64, f64) {
    (
        min.map_or(0.0, f64::from),
        max.map_or(f64::INFINITY, f64::from),
    )
}
fn scalar(id: &DisplayId, v: f64) -> Result<f32, FlowError> {
    let v = v as f32;
    if v.is_finite() {
        Ok(v)
    } else {
        Err(invalid(id, "layout arithmetic overflow"))
    }
}

pub(crate) fn container(
    scene: &mut Scene,
    id: &DisplayId,
    style: &FlowStyle,
    options: &BTreeMap<DisplayId, FlowItem>,
) -> Result<usize, FlowError> {
    let parent = scene.get(id).expect("validated container");
    let size = parent.object.shape.local_bounds().expect("Box/Frame").size;
    let children = parent.children.clone();
    let row = style.direction == FlowDirection::Row;
    // Saturate the content area at zero when padding exceeds the fixed box.
    let width =
        (size.width as f64 - style.padding.left as f64 - style.padding.right as f64).max(0.0);
    let height =
        (size.height as f64 - style.padding.top as f64 - style.padding.bottom as f64).max(0.0);
    let (available_main, available_cross) = axis(row, width, height);
    let (origin_main, origin_cross) =
        axis(row, style.padding.left as f64, style.padding.top as f64);
    let gap = style.gap as f64;
    let mut lines = Vec::new();
    let mut line = Line::default();
    let mut count = 0;
    for child in children {
        let input = options.get(&child).cloned().unwrap_or_default();
        if !input.in_flow {
            continue;
        }
        // Visibility is a painting policy, not the collapsed layout sentinel.
        let object = scene.get(&child).expect("validated child").object.clone();
        let bounds = linear_bounds(&object)?;
        let (current_main, current_cross) =
            axis(row, bounds.size.width as f64, bounds.size.height as f64);
        let (width_min, width_max) = constraints(input.min_width, input.max_width);
        let (height_min, height_max) = constraints(input.min_height, input.max_height);
        let ((main_min, main_max), (cross_min, cross_max)) = if row {
            ((width_min, width_max), (height_min, height_max))
        } else {
            ((height_min, height_max), (width_min, width_max))
        };
        let basis =
            input
                .basis
                .map(f64::from)
                .unwrap_or(if input.grow > 0.0 { 0.0 } else { current_main });
        let main = basis.clamp(main_min, main_max);
        let cross = current_cross.clamp(cross_min, cross_max);
        let addition = main + if line.items.is_empty() { 0.0 } else { gap };
        if style.wrap && !line.items.is_empty() && line.main + addition > available_main {
            lines.push(line);
            line = Line::default();
        }
        line.main += main + if line.items.is_empty() { 0.0 } else { gap };
        line.cross = line.cross.max(cross);
        line.items.push(Item {
            object,
            bounds,
            input,
            main,
            cross,
            main_max,
            cross_min,
            cross_max,
        });
        count += 1;
    }
    if !line.items.is_empty() {
        lines.push(line);
    }
    if lines.is_empty() {
        return Ok(0);
    }

    for line in &mut lines {
        grow(line, available_main);
        if !style.wrap {
            // One non-wrapped line fills the content cross axis. Oversized
            // fixed children overflow rather than changing the parent's size.
            line.cross = available_cross;
        }
    }
    let total_cross: f64 = lines.iter().map(|l| l.cross).sum::<f64>()
        + style.line_gap as f64 * lines.len().saturating_sub(1) as f64;
    let cross_free = (available_cross - total_cross).max(0.0);
    let mut cross_at = origin_cross
        + match style.align_content {
            AlignContent::Start => 0.0,
            AlignContent::Center => cross_free / 2.0,
            AlignContent::End => cross_free,
        };
    for line in lines {
        let free = (available_main - line.main).max(0.0);
        let mut main_at = origin_main
            + match style.justify {
                JustifyContent::Center => free / 2.0,
                JustifyContent::End => free,
                _ => 0.0,
            };
        let item_gap = gap
            + if style.justify == JustifyContent::SpaceBetween && line.items.len() > 1 {
                free / (line.items.len() - 1) as f64
            } else {
                0.0
            };
        for mut item in line.items {
            let align = item.input.align_self.unwrap_or(style.align_items);
            if align == AlignItems::Stretch {
                item.cross = line.cross.clamp(item.cross_min, item.cross_max);
            }
            let cross_free = (line.cross - item.cross).max(0.0);
            let cross_offset = match align {
                AlignItems::Center => cross_free / 2.0,
                AlignItems::End => cross_free,
                _ => 0.0,
            };
            let (width, height) = axis(row, item.main, item.cross);
            let (x, y) = axis(row, main_at, cross_at + cross_offset);
            resize(&mut item.object, item.bounds, width, height)?;
            let bounds = linear_bounds(&item.object)?;
            item.object.transform.tx = scalar(&item.object.id, x - bounds.origin.x as f64)?;
            item.object.transform.ty = scalar(&item.object.id, y - bounds.origin.y as f64)?;
            let child = item.object.id.clone();
            if scene.get(&child).expect("child").object != item.object {
                scene.update_object(&child, |object| *object = item.object)?;
            }
            main_at += item.main + item_gap;
        }
        cross_at += line.cross + style.line_gap as f64;
    }
    Ok(count)
}

/// Weighted positive free-space allocation with max constraints. Freeze all
/// clamped children together, then redistribute their remainder, avoiding
/// child-order bias and permitting any number of constrained siblings.
fn grow(line: &mut Line, available: f64) {
    let mut free = (available - line.main).max(0.0);
    let mut active: Vec<_> = line
        .items
        .iter()
        .enumerate()
        .filter(|(_, item)| item.input.grow > 0.0 && item.main < item.main_max)
        .map(|(index, _)| index)
        .collect();
    while free > 0.0 && !active.is_empty() {
        let weight: f64 = active
            .iter()
            .map(|&i| line.items[i].input.grow as f64)
            .sum();
        let capped: Vec<_> = active
            .iter()
            .copied()
            .filter(|&i| {
                let item = &line.items[i];
                free * item.input.grow as f64 / weight >= item.main_max - item.main
            })
            .collect();
        if capped.is_empty() {
            for index in active {
                let item = &mut line.items[index];
                item.main += free * item.input.grow as f64 / weight;
            }
            line.main += free;
            break;
        }
        for &index in &capped {
            let item = &mut line.items[index];
            let used = item.main_max - item.main;
            item.main = item.main_max;
            free = (free - used).max(0.0);
            line.main += used;
        }
        active.retain(|i| !capped.contains(i));
    }
}

fn resize(
    object: &mut DisplayObject,
    old: Bounds,
    width: f64,
    height: f64,
) -> Result<(), FlowError> {
    let width = scalar(&object.id, width)?;
    let height = scalar(&object.id, height)?;
    if old.size.width == width && old.size.height == height {
        return Ok(());
    }
    let transform = object.transform;
    if transform.a <= 0.0 || transform.d <= 0.0 || transform.b != 0.0 || transform.c != 0.0 {
        return Err(invalid(
            &object.id,
            "resizing requires a positive axis-aligned transform",
        ));
    }
    let size = match &mut object.shape {
        Shape::Rect(shape) => &mut shape.size,
        Shape::Ellipse(shape) => &mut shape.size,
        Shape::Text(shape) => &mut shape.size,
        Shape::Image(shape) => &mut shape.size,
        Shape::Box(shape) => &mut shape.size,
        Shape::Frame(shape) => &mut shape.size,
        _ => {
            return Err(invalid(
                &object.id,
                "resizing supports Rect/Ellipse/Text/Image/Box/Frame only",
            ));
        }
    };
    // Do not round-trip an unchanged axis through the affine scale. Repeated
    // main-axis grow must not gradually perturb an unrelated intrinsic height.
    if old.size.width != width {
        size.width = width / transform.a;
    }
    if old.size.height != height {
        size.height = height / transform.d;
    }
    if !size.is_positive() {
        return Err(invalid(
            &object.id,
            "allocated size must remain finite and positive; provide a positive minimum when space is exhausted",
        ));
    }
    Ok(())
}
