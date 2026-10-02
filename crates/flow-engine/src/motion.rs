//! Explicit one-shot path placement. No persistent follower or frame scheduler.
use crate::{ChangeOrigin, ChangeSet, ElementHandle, EngineError, InstanceState, SceneEngine};
use gpui_flow_scene::{Affine, DisplayId, Point, Scene, SceneError, Shape};

/// One placement request. Distance is measured in the source path's local
/// arc-length units; the anchor is a point in the target's local coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PathPlacement {
    pub distance: f64,
    pub anchor: Point,
    /// None preserves the target's world linear transform. Some(offset) rotates
    /// its world +x direction to the sampled tangent plus this degree offset,
    /// preserving world scale, shear and handedness. No scale decomposition.
    pub rotation: Option<f32>,
}
impl PathPlacement {
    pub fn new(distance: f64) -> Self {
        Self {
            distance,
            anchor: Point::ZERO,
            rotation: None,
        }
    }
}
struct Placement {
    target: DisplayId,
    path: DisplayId,
    source_shape: Shape,
    source_world: Affine,
    target_world: Affine,
    anchor: Point,
    point: Point,
    local: Affine,
}
impl SceneEngine {
    /// Place a target's local anchor at a Path/Pen distance as one atomic edit.
    /// Target and path must be attached to this engine. The target cannot be the
    /// path or its ancestor. Singular parents and collapsed rotation axes reject.
    /// Reactive layout cannot silently move the source or override the pose.
    /// This is an explicit data edit; host interaction/locked policy is separate.
    pub fn place_on_path(
        &mut self,
        target: ElementHandle,
        path: ElementHandle,
        options: PathPlacement,
    ) -> Result<ChangeSet, EngineError> {
        self.place_on_path_with_origin(target, path, options, ChangeOrigin::Edit)
    }
    /// The same atomic placement contract with explicit frame provenance.
    pub fn place_on_path_with_origin(
        &mut self,
        target: ElementHandle,
        path: ElementHandle,
        options: PathPlacement,
        origin: ChangeOrigin,
    ) -> Result<ChangeSet, EngineError> {
        let plan = Placement::prepare(self, target, path, options)?;
        self.transaction_validated_with_origin(
            origin,
            |scene| scene.update_object(&plan.target, |object| object.transform = plan.local),
            |scene| plan.validate(scene),
        )
    }
}
impl Placement {
    fn prepare(
        engine: &SceneEngine,
        target: ElementHandle,
        path: ElementHandle,
        options: PathPlacement,
    ) -> Result<Self, EngineError> {
        if !options.distance.is_finite()
            || !options.anchor.is_finite()
            || options.rotation.is_some_and(|r| !r.is_finite())
        {
            return Err(EngineError::Property(
                "path placement inputs must be finite".into(),
            ));
        }
        let target_id = attached_id(engine, target)?;
        let path_id = attached_id(engine, path)?;
        let mut ancestor = Some(&path_id);
        while let Some(id) = ancestor {
            if id == &target_id {
                return Err(EngineError::Property(
                    "target cannot be its path or an ancestor of its path".into(),
                ));
            }
            ancestor = engine.scene().get(id).and_then(|node| node.parent.as_ref());
        }
        let sample = engine
            .motion_point(path, options.distance)?
            .ok_or_else(|| EngineError::Property("path has no measurable geometry".into()))?;
        let current = engine.world_transform(&target_id)?;
        let mut linear = Affine {
            tx: 0.,
            ty: 0.,
            ..current
        };
        if let Some(offset) = options.rotation {
            if current.a == 0. && current.b == 0. {
                return Err(EngineError::Property(
                    "target world +x axis is collapsed".into(),
                ));
            }
            let angle = (current.b as f64).atan2(current.a as f64).to_degrees();
            // Reduce a huge finite offset first; adding the sampled angle to
            // it directly could lose that angle before the modulo operation.
            let delta = (sample.rotation_degrees as f64 + (offset as f64).rem_euclid(360.) - angle)
                .rem_euclid(360.);
            linear = Affine::rotation(delta as f32) * linear;
        }
        let target_world = Affine::translation(sample.point.x, sample.point.y)
            * linear
            * Affine::translation(-options.anchor.x, -options.anchor.y);
        let node = engine.scene().get(&target_id).expect("attached");
        let parent_world = match &node.parent {
            Some(parent) => engine.world_transform(parent)?,
            None => Affine::IDENTITY,
        };
        let inverse = parent_world
            .inverse()
            .ok_or_else(|| EngineError::Property("target parent transform is singular".into()))?;
        let mut local = inverse * target_world;
        if !local.is_finite()
            || !target_world.is_finite()
            || !affine_near(parent_world * local, target_world)
        {
            return Err(EngineError::Property(
                "path placement cannot be represented through target parent".into(),
            ));
        }
        // Stable repeated requests should not manufacture history out of f32
        // matrix roundoff. This tolerance is at most eight scaled f32 epsilons.
        if affine_near(local, node.object.transform)
            && affine_near(current, target_world)
            && point_near(current.transform_point(options.anchor), sample.point)
        {
            local = node.object.transform;
        }
        Ok(Self {
            target: target_id,
            path: path_id.clone(),
            source_shape: engine.object(path)?.shape.clone(),
            source_world: engine.world_transform(&path_id)?,
            target_world,
            anchor: options.anchor,
            point: sample.point,
            local,
        })
    }
    fn validate(&self, scene: &Scene) -> Result<(), SceneError> {
        let source = scene
            .get(&self.path)
            .ok_or_else(|| SceneError::MissingNode(self.path.clone()))?;
        if source.object.shape != self.source_shape
            || scene.world_transform(&self.path)? != self.source_world
        {
            return Err(SceneError::InvalidGeometry(
                "layout changed the source path during placement".into(),
            ));
        }
        let world = scene.world_transform(&self.target)?;
        if !affine_near(world, self.target_world)
            || !point_near(world.transform_point(self.anchor), self.point)
        {
            return Err(SceneError::InvalidGeometry(
                "layout overrode path placement".into(),
            ));
        }
        Ok(())
    }
}
fn attached_id(engine: &SceneEngine, handle: ElementHandle) -> Result<DisplayId, EngineError> {
    let object = engine.object(handle)?;
    if engine.instance(handle)?.state != InstanceState::Attached {
        return Err(EngineError::DetachedHandle);
    }
    Ok(object.id.clone())
}
fn affine_near(a: Affine, b: Affine) -> bool {
    // Linear columns have separate relative scales; an absolute epsilon floor
    // would silently discard rotations of very small but nonzero elements.
    let x = (a.a as f64)
        .abs()
        .max((a.b as f64).abs())
        .max((b.a as f64).abs())
        .max((b.b as f64).abs());
    let y = (a.c as f64)
        .abs()
        .max((a.d as f64).abs())
        .max((b.c as f64).abs())
        .max((b.d as f64).abs());
    near(a.a, b.a, x)
        && near(a.b, b.b, x)
        && near(a.c, b.c, y)
        && near(a.d, b.d, y)
        && point_near(Point::new(a.tx, a.ty), Point::new(b.tx, b.ty))
}
fn point_near(a: Point, b: Point) -> bool {
    near(a.x, b.x, (a.x as f64).abs().max((b.x as f64).abs()).max(1.))
        && near(a.y, b.y, (a.y as f64).abs().max((b.y as f64).abs()).max(1.))
}
fn near(a: f32, b: f32, scale: f64) -> bool {
    a.is_finite()
        && b.is_finite()
        && (a as f64 - b as f64).abs() <= 8. * f32::EPSILON as f64 * scale
}
