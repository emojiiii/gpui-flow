use crate::{CompositeError, CompositeIssue, CompositeLimits};
use gpui_flow_scene::{
    Affine, BlendMode, ClipRegion, Color, DisplayId, DisplayObject, EraserMode, MaskMode,
    PaintPhase, Point, Scene, SceneError, Shape, display::Group,
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
    pub(crate) lowered_geometry: Option<gpui_flow_scene::StrokeGeometryBudget>,
    pub(crate) resolved_camera: Option<f32>,
    /// Explicit logical scale when an export matrix already includes DPR.
    pub(crate) logical_camera: Option<f32>,
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DomainNode {
    pub object: DisplayObject,
    pub stroke_scale: gpui_flow_scene::StrokeScaleData,
    pub world_transform: Affine,
    pub children: Vec<DomainNode>,
}
/// The native adapter substitutes one entire subtree at each Domain item.
#[derive(Clone, Debug, PartialEq)]
pub enum SceneUnit {
    Native(DisplayId),
    /// A vector container's own paint at its tree entry or exit. Both phases
    /// retain one canonical object and its ancestor-only clip chain.
    NativePhase {
        id: DisplayId,
        phase: PaintPhase,
    },
    Domain(Box<DomainSource>),
    Rejected(CompositeIssue),
}

/// Find compositing boundaries before flattening. Opacity on a Box/Frame is a
/// boundary too, so a nested Group cannot accidentally bypass its parent.
/// Alpha masks use stable z-sorted direct Group siblings, never ID references.
pub fn scene_units(scene: &Scene, limits: CompositeLimits) -> Result<Vec<SceneUnit>, SceneError> {
    if scene.has_transfer_context() {
        return Err(SceneError::InvalidGeometry(
            "transient subtree context must be inserted before rendering".into(),
        ));
    }
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
    if scene.has_transfer_context() {
        return Err(CompositeError::UnsupportedScaleFixed(
            "transient subtree context must be inserted before rendering".into(),
        ));
    }
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
            stroke_scale: gpui_flow_scene::StrokeScaleData::IDENTITY,
            object: DisplayObject::new("", Group::new()),
            world_transform: Affine::IDENTITY,
            children,
        },
        clips: vec![],
        canvas: Some(backdrop),
        lowered_geometry: None,
        resolved_camera: scene.resolved_stroke_camera(),
        logical_camera: None,
    })
}
/// Capture a complete subtree for an imperative Canvas draw. Ancestors' clips,
/// opacity and background are intentionally not included. No scene is mutated.
pub fn capture_subtree(
    scene: &Scene,
    id: &DisplayId,
    limits: CompositeLimits,
) -> Result<DomainSource, CompositeError> {
    if scene.has_transfer_context() {
        return Err(CompositeError::UnsupportedScaleFixed(
            "transient subtree context must be inserted before rendering".into(),
        ));
    }
    scene
        .validate()
        .map_err(|e| CompositeError::InvalidGeometry(e.to_string()))?;
    let node = scene
        .get(id)
        .ok_or_else(|| CompositeError::InvalidGeometry("missing Canvas draw source".into()))?;
    let mut pending = vec![id];
    while let Some(next) = pending.pop() {
        let node = scene.get(next).expect("validated subtree");
        if node
            .object
            .style
            .strokes()
            .any(|s| !s.scale_fixed.is_disabled())
        {
            return Err(CompositeError::UnsupportedScaleFixed(
                "Canvas subtree capture".into(),
            ));
        }
        pending.extend(&node.children);
    }
    if node.object.eraser != EraserMode::None {
        return Err(CompositeError::UnsupportedExternalBackdrop);
    }
    let world = scene
        .world_transform(id)
        .map_err(|e| CompositeError::InvalidGeometry(e.to_string()))?;
    let parent = world
        * node
            .object
            .transform
            .inverse()
            .ok_or(CompositeError::UnsupportedTransform)?;
    let root = capture(
        scene,
        id,
        parent,
        &limits.bounded(),
        &mut Counts::default(),
        0,
    )?;
    Ok(DomainSource {
        root,
        clips: vec![],
        canvas: None,
        lowered_geometry: None,
        resolved_camera: scene.resolved_stroke_camera(),
        logical_camera: None,
    })
}
impl DomainSource {
    pub(crate) fn has_scale_fixed_strokes(&self) -> bool {
        fn visit(node: &DomainNode) -> bool {
            node.object.style.has_scale_fixed_strokes() || node.children.iter().any(visit)
        }
        visit(&self.root)
    }

    pub fn root_transform(&self) -> Affine {
        self.root.world_transform
    }
    pub fn canvases(&self) -> Vec<(&DisplayId, &gpui_flow_scene::display::Canvas)> {
        fn visit<'a>(
            node: &'a DomainNode,
            out: &mut Vec<(&'a DisplayId, &'a gpui_flow_scene::display::Canvas)>,
        ) {
            if !node.object.visible || node.object.style.opacity == 0. {
                return;
            }
            if let Shape::Canvas(canvas) = &node.object.shape {
                out.push((&node.object.id, canvas));
            }
            for child in &node.children {
                visit(child, out);
            }
        }
        let mut out = Vec::new();
        visit(&self.root, &mut out);
        out
    }

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
                && a.stroke_scale == b.stroke_scale
                && a.object.visible == b.object.visible
                && a.object.mask == b.object.mask
                && a.object.eraser == b.object.eraser
                && a.object.style.stroke == b.object.style.stroke
                && a.object.style.stroke_layers == b.object.style.stroke_layers
                && a.object.style.fill_rule == b.object.style.fill_rule
                && a.object.style.grayscale == b.object.style.grayscale
                && a.object.style.shadow == b.object.style.shadow
                && a.object.style.shadow_layers == b.object.style.shadow_layers
                && a.object.style.inner_shadow_layers == b.object.style.inner_shadow_layers
                && a.object.style.fills().map(|p| p.is_visible()).eq(b
                    .object
                    .style
                    .fills()
                    .map(|p| p.is_visible()))
                && a.children.len() == b.children.len()
                && a.children.iter().zip(&b.children).all(|(a, b)| same(a, b))
        }
        self.resolved_camera == other.resolved_camera
            && self.logical_camera == other.logical_camera
            && self.canvas == other.canvas
            && self.clips == other.clips
            && same(&self.root, &other.root)
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
        self.text_sources()
            .into_iter()
            .map(|(object, world, _)| (object, world))
            .collect()
    }
    /// Text producers plus whether an effect samples their complete alignment
    /// box. A host must not pre-crop those inputs to the destination viewport:
    /// offscreen glyph alpha can affect visible inner or outer shadow pixels.
    pub fn text_sources(&self) -> Vec<(&DisplayObject, Affine, bool)> {
        fn visit<'a>(
            node: &'a DomainNode,
            ancestor_effect: bool,
            out: &mut Vec<(&'a DisplayObject, Affine, bool)>,
        ) {
            if !node.object.visible || node.object.style.opacity == 0. {
                return;
            }
            let effect = ancestor_effect
                || node.object.style.has_visible_shadows()
                || node.object.style.has_visible_inner_shadows();
            if matches!(node.object.shape, Shape::Text(_)) {
                out.push((&node.object, node.world_transform, effect));
            }
            for child in &node.children {
                visit(child, effect, out);
            }
        }
        let mut out = Vec::new();
        visit(&self.root, false, &mut out);
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
    matches!(o.shape, Shape::Canvas(_))
        || o.eraser != EraserMode::None
        || (matches!(&o.shape,Shape::Text(t) if t.decoration != gpui_flow_scene::display::TextDecoration::None)
            && o.style
                .fills()
                .any(|p| matches!(p,gpui_flow_scene::Paint::Solid{color} if color.a<255)))
        || o.style.has_grayscale()
        || o.style.has_shadows()
        || o.style.has_inner_shadows()
        || o.style.blend_mode != BlendMode::PassThrough
        || (o.style.opacity < 1.0
            && (o.shape.allows_children()
                || matches!(&o.shape,Shape::Text(t) if t.decoration != gpui_flow_scene::display::TextDecoration::None)
                || o.style.fills().count() + o.style.strokes().count() > 1))
        || node.children.iter().any(|id| {
            scene.get(id).is_some_and(|n| {
                n.object.mask != MaskMode::None || n.object.eraser != EraserMode::None
            })
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
    let complex_ancestor_clip = clips
        .iter()
        .any(|c| !c.is_rectangular() || !c.world_transform.is_positive_uniform_scale_translation());
    let image_geometry_domain = matches!(&o.shape, Shape::Image(image)
        // GPUI's native atlas crop rounds source UVs to integer texels. A
        // decoded size is unavailable here, so every explicit normalized crop
        // uses the existing bounded CPU path, without silently snapping it.
        if image.source_crop.is_some()
            || !world.is_positive_uniform_scale_translation()
            || complex_ancestor_clip
            || (image.is_rounded() && image.fit == gpui_flow_scene::display::ImageFit::Contain));
    // Native Text content masks are rectangular. Preserve actual ancestor
    // geometry through the existing glyph bridge/CPU mask, never an AABB.
    // Affine Text reuses local glyph pixels; missing bridges/styles fail explicitly.
    let text_clip_domain = matches!(o.shape, Shape::Text(_))
        && (complex_ancestor_clip || !world.is_positive_uniform_scale_translation());
    if boundary(scene, id) || image_geometry_domain || text_clip_domain {
        *domains += 1;
        if *domains > limits.max_domains {
            out.push(SceneUnit::Rejected(CompositeIssue {
                object: id.clone(),
                error: CompositeError::InputLimit,
            }));
            return;
        }
        let result = if o.eraser != EraserMode::None || o.style.blend_mode.requires_backdrop() {
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
                lowered_geometry: None,
                resolved_camera: scene.resolved_stroke_camera(),
                logical_camera: None,
            })),
            Err(error) => SceneUnit::Rejected(CompositeIssue {
                object: id.clone(),
                error,
            }),
        });
        return;
    }
    let (before, after) = o.shape.paint_phases();
    out.push(match before {
        PaintPhase::All => SceneUnit::Native(id.clone()),
        phase => SceneUnit::NativePhase {
            id: id.clone(),
            phase,
        },
    });
    let mut clips = clips.to_vec();
    if let Some((local_bounds, corner_radius, corner_radii)) = o.shape.child_clip() {
        clips.push(ClipRegion {
            corner_radii,
            local_bounds,
            corner_radius,
            world_transform: world,
        });
    }
    for child in ordered(scene, &node.children) {
        visit(scene, child, world, &clips, limits, domains, out);
    }
    if let Some(phase) = after {
        out.push(SceneUnit::NativePhase {
            id: id.clone(),
            phase,
        });
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
    validate_eraser(o)?;
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
            stroke_scale: if o.style.has_scale_fixed_strokes() {
                scene
                    .stroke_scale(id)
                    .map_err(|e| CompositeError::InvalidGeometry(e.to_string()))?
            } else {
                gpui_flow_scene::StrokeScaleData::IDENTITY
            },
            object: o.clone(),
            world_transform: parent * o.transform,
            children: vec![],
        });
    }
    if o.style.has_shadows() && matches!(o.shape, Shape::Custom(_)) {
        return Err(CompositeError::UnsupportedShadow(o.id.clone()));
    }
    if o.style.has_inner_shadows() {
        crate::shadow::validate_inner_shape(&o.id, &o.shape)?;
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
        | Shape::Canvas(_)
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
    let captured = DomainNode {
        stroke_scale: if o.style.has_scale_fixed_strokes() {
            scene
                .stroke_scale(id)
                .map_err(|e| CompositeError::InvalidGeometry(e.to_string()))?
        } else {
            gpui_flow_scene::StrokeScaleData::IDENTITY
        },
        object: o.clone(),
        world_transform,
        children,
    };
    // A new effect surface must not silently sever a pass-through descendant
    // from its existing backdrop, even when the outer domain owns that backdrop.
    if o.shape.allows_children()
        && o.eraser == EraserMode::None
        && (o.style.has_visible_shadows() || o.style.has_visible_inner_shadows())
        && o.style.blend_mode == BlendMode::PassThrough
        && o.style.opacity == 1.
        && requires_parent_backdrop(&captured)
    {
        return Err(CompositeError::UnsupportedShadowBackdrop(o.id.clone()));
    }
    if o.shape.allows_children()
        && o.eraser == EraserMode::None
        && o.style.has_grayscale()
        && o.style.blend_mode == BlendMode::PassThrough
        && o.style.opacity == 1.
        && requires_parent_backdrop(&captured)
    {
        return Err(CompositeError::UnsupportedGrayscaleBackdrop(o.id.clone()));
    }
    Ok(captured)
}

// A mask alone does not isolate a pass-through Group in Leafer. Only blends
// already protected by a real isolation or a translucent mask run can ignore
// a native backdrop outside this CPU domain.
fn requires_parent_backdrop(node: &DomainNode) -> bool {
    if !node.object.visible || node.object.style.opacity == 0. {
        return false;
    }
    if node.object.eraser != EraserMode::None || node.object.style.blend_mode.requires_backdrop() {
        return true;
    }
    if node.object.style.blend_mode == BlendMode::Normal
        || node.object.style.opacity < 1.
        || node
            .children
            .iter()
            .any(|child| child.object.eraser != EraserMode::None)
    {
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

pub(crate) fn validate_eraser(object: &DisplayObject) -> Result<(), CompositeError> {
    if object.eraser == EraserMode::Path
        && !matches!(
            object.shape,
            Shape::Rect(_)
                | Shape::Ellipse(_)
                | Shape::Line(_)
                | Shape::Polygon(_)
                | Shape::Star(_)
                | Shape::Path(_)
                | Shape::Pen(_)
        )
    {
        return Err(CompositeError::UnsupportedPathEraser(object.id.clone()));
    }
    Ok(())
}
