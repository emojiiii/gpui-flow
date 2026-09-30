//! Private, headless Taffy 0.13.0 adapter for one already-formed Flow line.
//!
//! Taffy owns ordinary grow/max redistribution, item spacing, safe alignment,
//! and cross-axis stretch/clamping. It never receives Scene IDs or text backends.
//! Flow retains f64 line formation/content measurement, axis ownership, traversal
//! reversal, completed-line reversal, transformed intrinsic AABBs, and commit.
//! Text is measured by the existing bounded outer measure/arrange fixed point;
//! these leaves consume the measured boxes, not a second shaping callback.
//!
//! CSS grow weights differ from Flow: every surviving positive weight must use
//! all positive free space. Dividing by the smallest positive weight makes EACH
//! weight >= 1, even after other items freeze at their maxima. Scaling by the
//! initial sum would be incorrect. If the weight ratio or geometry cannot be
//! represented safely by Taffy's f32 kernel, retain the previous small f64 path.
//! This is deliberate bounded reuse, not a complete CSS/Leafer equivalence claim.
//!
//! Source checked: taffy 0.13.0 Cargo.toml (MIT; std/taffy_tree/flexbox features),
//! src/tree/taffy_tree.rs (new_leaf/new_with_children/disable_rounding/layout),
//! src/compute/flexbox.rs::resolve_flexible_lengths, and
//! src/style/alignment.rs (SAFE_CENTER/SAFE_END). The package archive SHA-256 is
//! c034e05f6ee85a12daa63863c2245797715075c70649947aa0da54f3f2ab1d0f
//! (matches the existing lockfile); the inspected source files match that archive.
//! License: https://github.com/DioxusLabs/taffy/blob/v0.13.0/LICENSE
//! Runtime regressions are UNRUN. Taffy computes in f32 instead of the previous
//! f64 accumulator; ordinary fractional allocations may differ by float rounding.
//! Compilation, fixed-point convergence, and performance still need verification.
use super::{Line, invalid};
use crate::{AlignItems, FlowError, FlowStyle, JustifyContent};
use gpui_flow_scene::DisplayId;
use taffy::prelude::{
    AlignItems as TaffyAlign, AvailableSpace, Dimension, Display, FlexDirection, FlexWrap,
    FromLength, JustifyContent as TaffyJustify, LengthPercentage, Size, Style, TaffyAuto,
    TaffyTree,
};

pub(super) struct Position {
    pub main: f64,
    pub cross: f64,
}

pub(super) fn solve(
    id: &DisplayId,
    line: &mut Line,
    available: f64,
    style: &FlowStyle,
    auto_main: bool,
) -> Result<Vec<Position>, FlowError> {
    // Keep huge/subnormal geometry in the previous f64 path. This also avoids
    // f32 intermediate sums overflowing despite a finite final Scene box.
    if !representable(line, available, style.gap) {
        return Ok(wide_line(line, available, style, auto_main));
    }
    let minimum_weight = line
        .items
        .iter()
        .filter_map(|item| (item.input.grow > 0.0).then_some(f64::from(item.input.grow)))
        .fold(f64::INFINITY, f64::min);
    let weights: Vec<f64> = line
        .items
        .iter()
        .map(|item| {
            if auto_main {
                0.0
            } else {
                f64::from(item.input.grow) / minimum_weight
            }
        })
        .collect();
    let wide_weights = weights.iter().sum::<f64>() > f64::from(f32::MAX) / 16.0;
    if wide_weights {
        // A common f32 scale cannot represent ratios spanning the entire finite
        // f32 range while keeping every surviving weight >= 1. Do not clamp or
        // drop tiny weights: after a large sibling caps they may own all remainder.
        grow_wide(line, available);
    }

    let error = |error: taffy::TaffyError| invalid(id, format!("Taffy line layout: {error}"));
    let mut tree: TaffyTree<()> = TaffyTree::new();
    tree.disable_rounding();
    let mut children = Vec::with_capacity(line.items.len());
    for (item, weight) in line.items.iter().zip(weights) {
        let align = item.input.align_self.unwrap_or(style.align_items);
        children.push(
            tree.new_leaf(Style {
                display: Display::Flex,
                // Every line is represented as a row in logical main/cross units.
                // arrange.rs alone maps these units back to physical x/y axes.
                size: Size {
                    width: Dimension::from_length(item.main as f32),
                    height: if align == AlignItems::Stretch {
                        Dimension::AUTO
                    } else {
                        Dimension::from_length(item.cross as f32)
                    },
                },
                // Explicit minima suppress CSS's automatic min-content minimum.
                min_size: Size {
                    width: Dimension::from_length(item.main as f32),
                    height: Dimension::from_length(item.cross_min as f32),
                },
                max_size: Size {
                    width: limit(item.main_max),
                    height: limit(item.cross_max),
                },
                flex_basis: Dimension::from_length(item.main as f32),
                flex_grow: if wide_weights { 0.0 } else { weight as f32 },
                flex_shrink: 0.0,
                align_self: Some(alignment(align)),
                ..Default::default()
            })
            .map_err(error)?,
        );
    }
    let root = tree
        .new_with_children(
            Style {
                display: Display::Flex,
                size: Size {
                    width: Dimension::from_length(available as f32),
                    height: Dimension::from_length(line.cross as f32),
                },
                min_size: Size {
                    width: Dimension::from_length(0.0),
                    height: Dimension::from_length(0.0),
                },
                flex_direction: FlexDirection::Row,
                flex_wrap: FlexWrap::NoWrap,
                flex_shrink: 0.0,
                gap: Size {
                    width: LengthPercentage::from_length(style.gap),
                    height: LengthPercentage::from_length(0.0),
                },
                justify_content: Some(match style.justify {
                    JustifyContent::Start => TaffyJustify::START,
                    JustifyContent::Center => TaffyJustify::SAFE_CENTER,
                    JustifyContent::End => TaffyJustify::SAFE_END,
                    JustifyContent::SpaceBetween => TaffyJustify::SPACE_BETWEEN,
                }),
                align_items: Some(TaffyAlign::START),
                ..Default::default()
            },
            &children,
        )
        .map_err(error)?;
    tree.compute_layout(
        root,
        Size {
            width: AvailableSpace::Definite(available as f32),
            height: AvailableSpace::Definite(line.cross as f32),
        },
    )
    .map_err(error)?;
    let mut positions = Vec::with_capacity(children.len());
    for (item, child) in line.items.iter_mut().zip(children) {
        let layout = tree.layout(child).map_err(error)?;
        if [
            layout.size.width,
            layout.size.height,
            layout.location.x,
            layout.location.y,
        ]
        .into_iter()
        .any(|value| !value.is_finite())
        {
            return Err(invalid(&item.object.id, "non-finite Taffy line result"));
        }
        item.main = f64::from(layout.size.width);
        item.cross = f64::from(layout.size.height);
        positions.push(Position {
            main: f64::from(layout.location.x),
            cross: f64::from(layout.location.y),
        });
    }
    Ok(positions)
}

fn alignment(align: AlignItems) -> TaffyAlign {
    match align {
        AlignItems::Start => TaffyAlign::START,
        AlignItems::Center => TaffyAlign::SAFE_CENTER,
        AlignItems::End => TaffyAlign::SAFE_END,
        AlignItems::Stretch => TaffyAlign::STRETCH,
    }
}
fn limit(value: f64) -> Dimension {
    if value.is_finite() {
        Dimension::from_length(value as f32)
    } else {
        Dimension::AUTO
    }
}
fn representable(line: &Line, available: f64, gap: f32) -> bool {
    let maximum = f64::from(f32::MAX) / 16.0;
    let finite =
        |value: f64| value == 0.0 || (f64::from(f32::MIN_POSITIVE)..=maximum).contains(&value);
    [available, line.main, line.cross, f64::from(gap)]
        .into_iter()
        .all(finite)
        // Taffy's flexible-length pass intentionally ignores subnormal free
        // space (`is_normal`); preserve Flow's previous arithmetic in that case.
        && (available <= line.main || finite(available - line.main))
        && line.items.iter().all(|item| {
            [item.main, item.cross, item.cross_min]
                .into_iter()
                .all(finite)
                && [item.main_max, item.cross_max]
                    .into_iter()
                    .all(|value| value == f64::INFINITY || finite(value))
        })
}

/// Compatibility fallback for geometry outside the bounded f32 adapter domain.
/// This is the existing f64 alignment policy, not a new alternative flex engine.
fn wide_line(line: &mut Line, available: f64, style: &FlowStyle, auto_main: bool) -> Vec<Position> {
    if !auto_main {
        grow_wide(line, available);
    }
    let free = (available - line.main).max(0.0);
    let mut main = match style.justify {
        JustifyContent::Center => free / 2.0,
        JustifyContent::End => free,
        _ => 0.0,
    };
    let gap = f64::from(style.gap)
        + if style.justify == JustifyContent::SpaceBetween && line.items.len() > 1 {
            free / (line.items.len() - 1) as f64
        } else {
            0.0
        };
    line.items
        .iter_mut()
        .map(|item| {
            let align = item.input.align_self.unwrap_or(style.align_items);
            if align == AlignItems::Stretch {
                item.cross = line.cross.clamp(item.cross_min, item.cross_max);
            }
            let free = (line.cross - item.cross).max(0.0);
            let cross = match align {
                AlignItems::Center => free / 2.0,
                AlignItems::End => free,
                _ => 0.0,
            };
            let position = Position { main, cross };
            main += item.main + gap;
            position
        })
        .collect()
}

/// Existing f64 weighted allocation, retained only for numerical-domain fallback.
/// Taffy's f32 grow factors cannot represent every finite f32 weight ratio while
/// also avoiding CSS's partial-free-space behavior after max-constrained freezes.
fn grow_wide(line: &mut Line, available: f64) {
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
            .map(|&i| f64::from(line.items[i].input.grow))
            .sum();
        let capped: Vec<_> = active
            .iter()
            .copied()
            .filter(|&i| {
                let item = &line.items[i];
                free * f64::from(item.input.grow) / weight >= item.main_max - item.main
            })
            .collect();
        if capped.is_empty() {
            for index in active {
                let item = &mut line.items[index];
                item.main += free * f64::from(item.input.grow) / weight;
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
