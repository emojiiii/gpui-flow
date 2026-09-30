use crate::{CompositeError, CompositeIssue, CompositeLimits};
use gpui_flow_scene::{
    Affine, BlendMode, ClipRegion, DisplayId, DisplayObject, MaskMode, Scene, SceneError, Shape,
};

/// Ordered immutable domain input. Equality includes the complete presented
/// values, local opacity, sibling order, transforms and inherited clips.
#[derive(Clone, Debug, PartialEq)]
pub struct DomainSource {
    pub(crate) root: DomainNode,
    pub(crate) clips: Vec<ClipRegion>,
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DomainNode {
    pub object: DisplayObject,
    pub world_transform: Affine,
    pub children: Vec<DomainNode>,
}
/// The native adapter substitutes one entire subtree at each Domain item.
#[derive(Clone, Debug, PartialEq)]
pub enum SceneUnit {
    Native(DisplayId),
    Domain(Box<DomainSource>),
    Rejected(CompositeIssue),
}

/// Find compositing boundaries before flattening. Opacity on a Box/Frame is a
/// boundary too, so a nested Group cannot accidentally bypass its parent.
/// Alpha masks use stable z-sorted direct Group siblings, never ID references.
pub fn scene_units(scene: &Scene, limits: CompositeLimits) -> Result<Vec<SceneUnit>, SceneError> {
    scene.validate()?;
    let limits = limits.bounded();
    let mut units = Vec::new();
    for id in ordered(scene, scene.roots()) {
        visit(scene, id, Affine::IDENTITY, &[], &limits, &mut units);
    }
    Ok(units)
}
impl DomainSource {
    pub fn root_id(&self) -> &DisplayId {
        &self.root.object.id
    }
    pub fn ids(&self) -> Vec<&DisplayId> {
        fn visit<'a>(node: &'a DomainNode, out: &mut Vec<&'a DisplayId>) {
            out.push(&node.object.id);
            for child in &node.children {
                visit(child, out);
            }
        }
        let mut out = Vec::new();
        visit(&self.root, &mut out);
        out
    }
}
fn ordered<'a>(scene: &Scene, ids: &'a [DisplayId]) -> Vec<&'a DisplayId> {
    let mut ids: Vec<_> = ids.iter().collect();
    ids.sort_by_key(|id| scene.get(id).map_or(0, |n| n.object.z_index));
    ids
}
fn boundary(scene: &Scene, id: &DisplayId) -> bool {
    let Some(node) = scene.get(id) else {
        return false;
    };
    let o = &node.object;
    o.style.blend_mode != BlendMode::PassThrough
        || (o.style.opacity < 1.0
            && (o.shape.allows_children()
                || o.style.fills().count() + o.style.strokes().count() > 1))
        || node.children.iter().any(|id| {
            scene
                .get(id)
                .is_some_and(|n| n.object.mask != MaskMode::None)
        })
}
fn visit(
    scene: &Scene,
    id: &DisplayId,
    parent: Affine,
    clips: &[ClipRegion],
    limits: &CompositeLimits,
    out: &mut Vec<SceneUnit>,
) {
    let Some(node) = scene.get(id) else { return };
    let o = &node.object;
    if !o.visible || o.style.opacity == 0.0 {
        return;
    }
    let world = parent * o.transform;
    if boundary(scene, id) {
        let result = if matches!(o.style.blend_mode, BlendMode::Multiply | BlendMode::Screen) {
            Err(CompositeError::UnsupportedExternalBackdrop)
        } else {
            capture(scene, id, parent, limits, &mut Counts::default(), 0)
        };
        out.push(match result {
            Ok(root) => SceneUnit::Domain(Box::new(DomainSource {
                root,
                clips: clips.to_vec(),
            })),
            Err(error) => SceneUnit::Rejected(CompositeIssue {
                object: id.clone(),
                error,
            }),
        });
        return;
    }
    out.push(SceneUnit::Native(id.clone()));
    let mut clips = clips.to_vec();
    if let Some((local_bounds, corner_radius)) = o.shape.child_clip() {
        clips.push(ClipRegion {
            local_bounds,
            corner_radius,
            world_transform: world,
        });
    }
    for child in ordered(scene, &node.children) {
        visit(scene, child, world, &clips, limits, out);
    }
}
#[derive(Default)]
struct Counts {
    nodes: usize,
    points: usize,
    masks: usize,
}
fn capture(
    scene: &Scene,
    id: &DisplayId,
    parent: Affine,
    limits: &CompositeLimits,
    counts: &mut Counts,
    depth: usize,
) -> Result<DomainNode, CompositeError> {
    let node = scene
        .get(id)
        .ok_or_else(|| CompositeError::InvalidGeometry("missing child".into()))?;
    let o = &node.object;
    counts.nodes += 1;
    if counts.nodes > limits.max_nodes {
        return Err(CompositeError::InputLimit);
    }
    if depth > limits.max_depth {
        return Err(CompositeError::NestingLimit);
    }
    let points = match &o.shape {
        Shape::Path(p) => p.commands.len(),
        Shape::Pen(p) => p.path.commands.len(),
        Shape::Polygon(p) => p.points.len(),
        Shape::Star(s) => s.points.saturating_mul(2),
        Shape::Rect(_) | Shape::Ellipse(_) | Shape::Line(_) | Shape::Group(_) => 0,
        other => return Err(CompositeError::UnsupportedShape(other.tag().into())),
    };
    counts.points = counts.points.saturating_add(points);
    if points > limits.max_points || counts.points > limits.max_total_points {
        return Err(CompositeError::InputLimit);
    }
    if o.mask != MaskMode::None {
        counts.masks += 1;
        if counts.masks > limits.max_mask_runs {
            return Err(CompositeError::InputLimit);
        }
    }
    for paint in o.style.fills().chain(o.style.strokes().map(|s| &s.paint)) {
        if matches!(paint, gpui_flow_scene::Paint::LinearGradient {stops,..} | gpui_flow_scene::Paint::RadialGradient {stops,..} if stops.len() > 256)
        {
            return Err(CompositeError::InputLimit);
        }
    }
    o.validate().map_err(CompositeError::InvalidGeometry)?;
    let world_transform = parent * o.transform;
    if world_transform.inverse().is_none() {
        return Err(CompositeError::UnsupportedTransform);
    }
    let mut children = Vec::new();
    for child in ordered(scene, &node.children) {
        children.push(capture(
            scene,
            child,
            world_transform,
            limits,
            counts,
            depth + 1,
        )?);
    }
    Ok(DomainNode {
        object: o.clone(),
        world_transform,
        children,
    })
}
