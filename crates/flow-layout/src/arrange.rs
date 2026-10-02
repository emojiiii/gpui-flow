use crate::{AlignContent, FlowError, FlowItem, FlowStyle, ItemBox, JustifyContent, invalid};
use gpui_flow_scene::{Affine, Bounds, DisplayId, DisplayObject, Scene, Shape};
use std::collections::BTreeMap;

#[path = "taffy_adapter.rs"]
mod solver;

struct Item {
    object: DisplayObject,
    bounds: Bounds,
    input: FlowItem,
    main: f64,
    cross: f64,
    main_max: f64,
    cross_min: f64,
    cross_max: f64,
    baseline: Option<f64>,
}
#[derive(Default)]
struct Line {
    items: Vec<Item>,
    main: f64,
    cross: f64,
    ascent: f64,
    descent: f64,
}

fn has_visible_stroke(object: &DisplayObject) -> bool {
    object
        .style
        .strokes()
        .any(|stroke| stroke.width > 0.0 && stroke.paint.is_visible())
}

pub(crate) fn validate_item_box(object: &DisplayObject, mode: ItemBox) -> Result<(), FlowError> {
    if mode == ItemBox::Stroke
        && has_visible_stroke(object)
        && matches!(object.shape, Shape::Text(_) | Shape::Image(_))
    {
        return Err(invalid(
            &object.id,
            "stroke item boxes with visible Text/Image strokes are unsupported by the native renderer",
        ));
    }
    Ok(())
}

fn linear_bounds(object: &DisplayObject, mode: ItemBox) -> Result<Bounds, FlowError> {
    let local = object.shape.local_bounds().ok_or_else(|| {
        invalid(
            &object.id,
            "participating child has no intrinsic box; exclude Groups with in_flow=false",
        )
    })?;
    // Leafer getItemBounds selects the child's own local stroke bounds. Box's
    // stroke calculation is Rect-like, not the subtree/render bound. Reuse our
    // existing conservative geometry (including mesh cap/join tips and dash
    // gaps), never Scene::local_bounds or a second stroke expansion algorithm.
    let local = match mode {
        ItemBox::Box => local,
        ItemBox::Stroke => object.paint_bounds().expect("intrinsic box"),
    };
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
    baselines: &BTreeMap<DisplayId, f64>,
) -> Result<usize, FlowError> {
    let parent = scene.get(id).expect("validated container");
    let size = parent.object.shape.local_bounds().expect("Box/Frame").size;
    let mut children = parent.children.clone();
    if style.direction.is_reverse() {
        children.reverse();
    }
    let row = style.direction.is_row();
    let auto_main = if row {
        style.width_sizing
    } else {
        style.height_sizing
    }
    .is_content();
    let auto_cross = if row {
        style.height_sizing
    } else {
        style.width_sizing
    }
    .is_content();
    // Like upstream Flow, content-sized main axes have no finite wrapping line.
    let wrap = style.wrap && !auto_main;
    let content = crate::content::resolve(id, size, style)?;
    let width = content.width;
    let height = content.height;
    let (mut available_main, mut available_cross) = axis(row, width, height);
    let (mut origin_main, mut origin_cross) = axis(row, content.x, content.y);
    // Leafer forms auto/fit lines with zero gap, then distributes their space.
    // Keep fixed-gap line formation and the existing Taffy adapter unchanged.
    let gap = style.gap_mode.initial(style.gap);
    let initial_line_gap = style.line_gap_mode.initial(style.line_gap);
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
        let bounds = linear_bounds(&object, style.item_box)?;
        let (current_main, current_cross) = axis(
            row,
            input
                .width_percent
                .map_or(f64::from(bounds.size.width), |fraction| {
                    width * f64::from(fraction)
                }),
            input
                .height_percent
                .map_or(f64::from(bounds.size.height), |fraction| {
                    height * f64::from(fraction)
                }),
        );
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
        if wrap && !line.items.is_empty() && line.main + addition > available_main {
            lines.push(line);
            line = Line::default();
        }
        line.main += main + if line.items.is_empty() { 0.0 } else { gap };
        line.cross = line.cross.max(cross);
        let baseline = baselines.get(&child).copied();
        line.items.push(Item {
            object,
            bounds,
            input,
            main,
            cross,
            main_max,
            cross_min,
            cross_max,
            baseline,
        });
        // First-baseline extents are an O(1) accumulation per item. Ordinary
        // non-baseline rows retain linear line-formation work.
        if let Some(baseline) = baseline {
            line.ascent = line.ascent.max(baseline);
            line.descent = line.descent.max(cross - baseline);
            line.cross = line.cross.max(line.ascent + line.descent);
        }
        count += 1;
    }
    if !line.items.is_empty() {
        lines.push(line);
    }
    let natural_main = lines.iter().map(|line| line.main).fold(0.0, f64::max);
    let natural_cross = lines.iter().map(|line| line.cross).sum::<f64>()
        + initial_line_gap * lines.len().saturating_sub(1) as f64;
    let (natural_width, natural_height) = axis(row, natural_main, natural_cross);
    let next_width = scalar(
        id,
        style.width_sizing.resolve(
            natural_width + style.padding.left as f64 + style.padding.right as f64,
            size.width as f64,
        ),
    )?;
    let next_height = scalar(
        id,
        style.height_sizing.resolve(
            natural_height + style.padding.top as f64 + style.padding.bottom as f64,
            size.height as f64,
        ),
    )?;
    if next_width != size.width || next_height != size.height {
        scene.update_object(id, |object| {
            let size = match &mut object.shape {
                Shape::Box(shape) => &mut shape.size,
                Shape::Frame(shape) => &mut shape.size,
                _ => unreachable!("validated container"),
            };
            size.width = next_width;
            size.height = next_height;
        })?;
    }
    if auto_main || auto_cross {
        let content = crate::content::resolve(
            id,
            gpui_flow_scene::Size::new(next_width, next_height),
            style,
        )?;
        (available_main, available_cross) = axis(row, content.width, content.height);
        (origin_main, origin_cross) = axis(row, content.x, content.y);
    }
    for line in &mut lines {
        if !wrap {
            // One non-wrapped line fills the content cross axis. Oversized
            // fixed children overflow rather than changing the parent's size.
            line.cross = available_cross;
        }
    }
    let occupied_cross: f64 = lines.iter().map(|l| l.cross).sum();
    let automatic_line_gap =
        style
            .line_gap_mode
            .distribute(available_cross, occupied_cross, lines.len());
    let line_gap = automatic_line_gap.unwrap_or(initial_line_gap);
    // Upstream sets the resolved extent directly. Recomputing it from signed
    // gaps could cancel huge values and invent free alignment space.
    let total_cross = if automatic_line_gap.is_some() {
        available_cross
    } else {
        occupied_cross + line_gap * lines.len().saturating_sub(1) as f64
    };
    let cross_free = (available_cross - total_cross).max(0.0);
    let mut cross_at = origin_cross
        + match style.align_content {
            AlignContent::Start => 0.0,
            AlignContent::Center => cross_free / 2.0,
            AlignContent::End => cross_free,
        };
    if wrap && style.wrap_reverse {
        lines.reverse();
    }
    let mut numeric_style = style.clone();
    numeric_style.gap = gap as f32;
    for mut line in lines {
        // Leafer flowX grow wins over automatic gaps, even when all growing
        // items cap. flowY's Auto after grow is also zero. Its Fit-after-grow
        // ordering is outside this slice; column grow + Fit is rejected.
        let automatic_gap = if line.items.iter().any(|item| item.input.grow > 0.0) {
            None
        } else {
            style
                .gap_mode
                .distribute(available_main, line.main, line.items.len())
        };
        numeric_style.justify = if automatic_gap.is_some() {
            JustifyContent::Start
        } else {
            style.justify
        };
        let mut positions =
            solver::solve(id, &mut line, available_main, &numeric_style, auto_main)?;
        for (item, position) in line.items.iter().zip(&mut positions) {
            if let Some(baseline) = item.baseline {
                position.cross = line.ascent - baseline;
            }
        }
        // Leafer lockRatio scales both intrinsic dimensions from the allocated
        // main axis. CSS aspect_ratio is not this operation: definite dimensions
        // suppress it, while cross min/max and stretch transfer different rules.
        // Keep Taffy's tested allocation, then couple only explicitly locked
        // items. Validation excludes ambiguous wrap/content-size combinations.
        for (item, position) in line.items.iter_mut().zip(&mut positions) {
            if item.input.lock_ratio {
                crate::ratio::resize(
                    &mut item.object,
                    item.bounds,
                    row,
                    item.main,
                    style.item_box,
                )?;
                let bounds = linear_bounds(&item.object, style.item_box)?;
                item.cross = axis(
                    row,
                    f64::from(bounds.size.width),
                    f64::from(bounds.size.height),
                )
                .1;
                let free = (line.cross - item.cross).max(0.0);
                position.cross = match item.input.align_self.unwrap_or(style.align_items) {
                    crate::AlignItems::Center => free / 2.0,
                    crate::AlignItems::End => free,
                    _ => 0.0,
                };
            }
        }
        if let Some(gap) = automatic_gap {
            // CSS gaps cannot express Leafer's signed Fit spacing. Taffy still
            // owns item sizes/cross alignment; this small f64 policy owns only
            // the automatic main positions, including overflowing/negative ones.
            let mut main = 0.0;
            for (item, position) in line.items.iter().zip(&mut positions) {
                position.main = main;
                main += item.main + gap;
            }
        }
        for (mut item, position) in line.items.into_iter().zip(positions) {
            let (width, height) = axis(row, item.main, item.cross);
            let (x, y) = axis(row, origin_main + position.main, cross_at + position.cross);
            if !item.input.lock_ratio {
                resize(&mut item.object, item.bounds, width, height, style.item_box)?;
            }
            let bounds = linear_bounds(&item.object, style.item_box)?;
            item.object.transform.tx = scalar(&item.object.id, x - bounds.origin.x as f64)?;
            item.object.transform.ty = scalar(&item.object.id, y - bounds.origin.y as f64)?;
            let child = item.object.id.clone();
            if scene.get(&child).expect("child").object != item.object {
                scene.update_object(&child, |object| *object = item.object)?;
            }
        }
        cross_at += line.cross + line_gap;
    }
    Ok(count)
}

fn resize(
    object: &mut DisplayObject,
    old: Bounds,
    width: f64,
    height: f64,
    mode: ItemBox,
) -> Result<(), FlowError> {
    let width = scalar(&object.id, width)?;
    let height = scalar(&object.id, height)?;
    if old.size.width == width && old.size.height == height {
        return Ok(());
    }
    // Allocation is in slot units; assigning a stroke slot directly to shape
    // size would grow content by the stroke extent on every subsequent pass.
    // Upstream scaleResize is a different contract. Keep this first slice
    // position-only instead of inventing stroke subtraction or mesh inversion.
    if mode == ItemBox::Stroke && has_visible_stroke(object) {
        return Err(invalid(
            &object.id,
            "resizing visible stroke item boxes is unsupported; use box mode or remove the size allocation",
        ));
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
        Shape::Canvas(shape) => &mut shape.size,
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
