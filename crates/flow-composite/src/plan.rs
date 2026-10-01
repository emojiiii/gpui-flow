use crate::{CompositeError, CompositeIssue, CompositeLimits};
use gpui_flow_scene::{
    Affine, BlendMode, ClipRegion, Color, DisplayId, DisplayObject, MaskMode, Point, Scene,
    SceneError, Shape, display::Group,
};

/// An opaque canvas that the caller owns completely. This does not capture a
/// toolkit window or any native elements painted behind this domain.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasBackdrop {
    pub color: Color,
    pub grid: Option<CanvasGrid>,
}
/// One-logical-pixel dots, anchored relative to the canvas origin in screen
/// coordinates. Pan/zoom are resolved by the host, independently of scene roots.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasGrid {
    pub color: Color,
    pub spacing: f32,
    pub offset: Point,
}

/// Ordered immutable domain input. Equality includes the complete presented
/// values, local opacity, sibling order, transforms and inherited clips.
#[derive(Clone, Debug, PartialEq)]
pub struct DomainSource {
    pub(crate) root: DomainNode,
    pub(crate) clips: Vec<ClipRegion>,
    pub(crate) canvas: Option<CanvasBackdrop>,
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
    let mut domains = 0;
    for id in ordered(scene, scene.roots()) {
        visit(
            scene,
            id,
            Affine::IDENTITY,
            &[],
            &limits,
            &mut domains,
            &mut units,
        );
    }
    Ok(units)
}

/// Capture the complete presented scene in stable tree paint order. The opaque
/// canvas is the first contributor, so roots may blend against earlier roots.
/// No canonical nodes, identities, hit testing or document history are changed.
pub fn owned_canvas(
    scene: &Scene,
    backdrop: CanvasBackdrop,
    limits: CompositeLimits,
) -> Result<DomainSource, CompositeError> {
    scene
        .validate()
        .map_err(|e| CompositeError::InvalidGeometry(e.to_string()))?;
    if backdrop.color.a != 255 {
        return Err(CompositeError::UnsupportedExternalBackdrop);
    }
    if backdrop
        .grid
        .is_some_and(|g| !g.spacing.is_finite() || g.spacing < 1. || !g.offset.is_finite())
    {
        return Err(CompositeError::InvalidGeometry("canvas grid".into()));
    }
    let limits = limits.bounded();
    if limits.max_domains == 0 {
        return Err(CompositeError::InputLimit);
    }
    let mut counts = Counts {
        nodes: 1,
        ..Default::default()
    };
    let children = ordered(scene, scene.roots())
        .into_iter()
        .map(|id| capture(scene, id, Affine::IDENTITY, &limits, &mut counts, 1))
        .collect::<Result<_, _>>()?;
    // Empty IDs are forbidden in a validated Scene. This runtime-only cache
    // identity therefore cannot collide with any canonical display object.
    Ok(DomainSource {
        root: DomainNode {
            object: DisplayObject::new("", Group::new()),
            world_transform: Affine::IDENTITY,
            children,
        },
        clips: vec![],
        canvas: Some(backdrop),
    })
}
impl DomainSource {
    /// The empty ID denotes the runtime owned canvas, never a canonical node.
    pub fn root_id(&self) -> &DisplayId {
        &self.root.object.id
    }
    pub fn is_owned_canvas(&self) -> bool {
        self.canvas.is_some()
    }
    /// Conservative stale-frame eligibility: paint/opacity-only changes may
    /// reuse a previous image; geometry, transform, clip, mask and order may not.
    pub fn placement_eq(&self, other: &Self) -> bool {
        fn same(a: &DomainNode, b: &DomainNode) -> bool {
            a.object.id == b.object.id
                && a.object.shape == b.object.shape
                && a.world_transform == b.world_transform
                && a.object.visible == b.object.visible
                && a.object.mask == b.object.mask
                && a.object.style.stroke == b.object.style.stroke
                && a.object.style.stroke_layers == b.object.style.stroke_layers
                && a.object.style.fill_rule == b.object.style.fill_rule
                && a.object.style.shadow == b.object.style.shadow
                && a.object.style.fills().map(|p| p.is_visible()).eq(b
                    .object
                    .style
                    .fills()
                    .map(|p| p.is_visible()))
                && a.children.len() == b.children.len()
                && a.children.iter().zip(&b.children).all(|(a, b)| same(a, b))
        }
        self.canvas == other.canvas && self.clips == other.clips && same(&self.root, &other.root)
    }
    pub fn images(&self) -> Vec<(&DisplayId, &gpui_flow_scene::display::Image)> {
        fn visit<'a>(
            node: &'a DomainNode,
            out: &mut Vec<(&'a DisplayId, &'a gpui_flow_scene::display::Image)>,
        ) {
            if !node.object.visible || node.object.style.opacity == 0. {
                return;
            }
            if let Shape::Image(image) = &node.object.shape {
                out.push((&node.object.id, image));
            }
            for child in &node.children {
                visit(child, out);
            }
        }
        let mut out = Vec::new();
        if self.is_owned_canvas() {
            for child in &self.root.children {
                visit(child, &mut out);
            }
        } else {
            visit(&self.root, &mut out);
        }
        out
    }
    /// Visible text producers with their exact presented style and world matrix.
    pub fn texts(&self) -> Vec<(&DisplayObject, Affine)> {
        fn visit<'a>(node: &'a DomainNode, out: &mut Vec<(&'a DisplayObject, Affine)>) {
            if !node.object.visible || node.object.style.opacity == 0. {
                return;
            }
            if matches!(node.object.shape, Shape::Text(_)) {
                out.push((&node.object, node.world_transform));
            }
            for child in &node.children {
                visit(child, out);
            }
        }
        let mut out = Vec::new();
        visit(&self.root, &mut out);
        out
    }
    pub fn ids(&self) -> Vec<&DisplayId> {
        fn visit<'a>(node: &'a DomainNode, out: &mut Vec<&'a DisplayId>) {
            out.push(&node.object.id);
            for child in &node.children {
                visit(child, out);
            }
        }
        let mut out = Vec::new();
        if self.is_owned_canvas() {
            for child in &self.root.children {
                visit(child, &mut out);
            }
        } else {
            visit(&self.root, &mut out);
        }
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
    o.style.shadow.is_some()
        || o.style.blend_mode != BlendMode::PassThrough
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
    domains: &mut usize,
    out: &mut Vec<SceneUnit>,
) {
    let Some(node) = scene.get(id) else { return };
    let o = &node.object;
    if !o.visible || o.style.opacity == 0.0 {
        return;
    }
    let world = parent * o.transform;
    if boundary(scene, id) {
        *domains += 1;
        if *domains > limits.max_domains {
            out.push(SceneUnit::Rejected(CompositeIssue {
                object: id.clone(),
                error: CompositeError::InputLimit,
            }));
            return;
        }
        let result = if o.style.blend_mode.requires_backdrop() {
            Err(CompositeError::UnsupportedExternalBackdrop)
        } else {
            capture(scene, id, parent, limits, &mut Counts::default(), 0)
        };
        out.push(match result {
            Ok(root) if requires_parent_backdrop(&root) => SceneUnit::Rejected(CompositeIssue {
                object: id.clone(),
                error: CompositeError::UnsupportedExternalBackdrop,
            }),
            Ok(root) => SceneUnit::Domain(Box::new(DomainSource {
                root,
                clips: clips.to_vec(),
                canvas: None,
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
        visit(scene, child, world, &clips, limits, domains, out);
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
    // Keep the marker (a hidden alpha mask still starts a zero-coverage run),
    // without demanding a producer for its invisible subtree.
    if !o.visible || o.style.opacity == 0. {
        return Ok(DomainNode {
            object: o.clone(),
            world_transform: parent * o.transform,
            children: vec![],
        });
    }
    if o.style.shadow.is_some()
        && (o.shape.allows_children() || matches!(o.shape, Shape::Custom(_)))
    {
        return Err(CompositeError::UnsupportedShadow(o.id.clone()));
    }
    let points = match &o.shape {
        Shape::Path(p) => p.commands.len(),
        Shape::Pen(p) => p.path.commands.len(),
        Shape::Polygon(p) => p.points.len(),
        Shape::Star(s) => s.points.saturating_mul(2),
        Shape::Rect(_)
        | Shape::Ellipse(_)
        | Shape::Line(_)
        | Shape::Group(_)
        | Shape::Image(_)
        | Shape::Text(_)
        | Shape::Box(_)
        | Shape::Frame(_) => 0,
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

// A mask alone does not isolate a pass-through Group in Leafer. Only blends
// already protected by a real isolation or a translucent mask run can ignore
// a native backdrop outside this CPU domain.
fn requires_parent_backdrop(node: &DomainNode) -> bool {
    if !node.object.visible || node.object.style.opacity == 0. {
        return false;
    }
    if node.object.style.blend_mode.requires_backdrop() {
        return true;
    }
    if node.object.style.blend_mode == BlendMode::Normal || node.object.style.opacity < 1. {
        return false;
    }
    let mut translucent_run = false;
    for child in &node.children {
        if child.object.mask != MaskMode::None {
            translucent_run = !child.object.visible || child.object.style.opacity < 1.;
            continue;
        }
        if !translucent_run && requires_parent_backdrop(child) {
            return true;
        }
    }
    false
}
