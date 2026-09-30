use crate::layout::intersection;
use crate::subtree::paint_ids;
use crate::{EngineError, LayoutEntry, SceneEngine};
use gpui_flow_scene::{
    Affine, Bounds, DisplayId, HitPolicy, HitTestOptions, HitTestResult, PaintItem, Point,
    SceneError, Shape,
};
impl SceneEngine {
    pub fn world_transform(&self, id: &DisplayId) -> Result<Affine, EngineError> {
        Ok(self.entry(id)?.world_transform)
    }
    pub fn local_bounds(&self, id: &DisplayId) -> Result<Option<Bounds>, EngineError> {
        Ok(self.entry(id)?.local_bounds)
    }
    pub fn world_bounds(&self, id: &DisplayId) -> Result<Option<Bounds>, EngineError> {
        Ok(self.entry(id)?.world_bounds)
    }
    fn entry(&self, id: &DisplayId) -> Result<&LayoutEntry, EngineError> {
        self.layout()
            .get(id)
            .ok_or_else(|| SceneError::MissingNode(id.clone()).into())
    }
    pub fn local_to_world(&self, id: &DisplayId, point: Point) -> Result<Point, EngineError> {
        checked_point(self.entry(id)?.world_transform.transform_point(point))
    }
    pub fn world_to_local(&self, id: &DisplayId, point: Point) -> Result<Point, EngineError> {
        let inverse = self
            .entry(id)?
            .inverse
            .ok_or_else(|| SceneError::NonInvertibleTransform(id.clone()))?;
        checked_point(inverse.transform_point(point))
    }
    /// Uses committed cached matrices/opacity/clips; no layout or Scene::validate
    /// is repeated per frame. Flattening/tessellation remains a backend concern.
    pub fn paint_list(&self) -> Result<Vec<PaintItem<'_>>, EngineError> {
        let mut result = vec![];
        for id in paint_ids(self.scene()) {
            let object = &self.scene().get(&id).expect("node").object;
            let e = self.entry(&id)?;
            if !e.visible || e.opacity <= 0. {
                continue;
            }
            result.push(PaintItem {
                object,
                world_transform: e.world_transform,
                opacity: e.opacity,
                locked: e.locked,
                clips: e.clips.clone(),
                hit_enabled: e.hit_enabled,
            });
        }
        Ok(result)
    }
    pub fn visible_bounds(&self) -> Result<Option<Bounds>, EngineError> {
        let mut bounds = None;
        for item in self.paint_list()? {
            let mut next = item
                .object
                .paint_bounds()
                .map(|b| item.world_transform.transform_bounds(b));
            for clip in item.clips {
                next = next.and_then(|b| intersection(b, clip.world_bounds()));
            }
            if let Some(b) = next {
                bounds = Some(bounds.map_or(b, |a: Bounds| a.union(b)));
            }
        }
        Ok(bounds)
    }
    pub fn hit_test(
        &self,
        world: Point,
        options: HitTestOptions,
    ) -> Result<Option<HitTestResult>, EngineError> {
        Ok(self.hit_test_all(world, options)?.into_iter().next())
    }
    pub fn hit_test_all(
        &self,
        world: Point,
        options: HitTestOptions,
    ) -> Result<Vec<HitTestResult>, EngineError> {
        if !world.is_finite() || !options.tolerance.is_finite() || options.tolerance < 0. {
            return Err(
                SceneError::InvalidGeometry("invalid hit point or tolerance".into()).into(),
            );
        }
        let mut hits = vec![];
        for id in paint_ids(self.scene()).into_iter().rev() {
            let object = &self.scene().get(&id).expect("node").object;
            let e = self.entry(&id)?;
            if !e.hit_enabled
                || (!options.include_locked && e.locked)
                || (!options.include_invisible && (!e.visible || e.opacity <= 0.))
                || object.hit_policy == HitPolicy::None
                || e.clips.iter().any(|c| !c.contains(world))
            {
                continue;
            }
            let Some(inverse) = e.inverse else {
                continue;
            };
            let local = inverse.transform_point(world);
            if !local.is_finite() {
                continue;
            }
            let tolerance =
                (options.tolerance as f64 * inverse.max_scale() as f64).min(f32::MAX as f64) as f32;
            let custom = match &object.shape {
                Shape::Custom(c)
                    if object.hit_policy != HitPolicy::Bounds
                        && c.bounds.expanded(tolerance).contains(local) =>
                {
                    self.registry().get(&c.kind).and_then(|d| {
                        d.schema()
                            .resolve(&c.data)
                            .ok()
                            .and_then(|p| d.hit_test(&p, local, tolerance))
                    })
                }
                _ => None,
            };
            let hit = custom.unwrap_or_else(|| match object.hit_policy {
                HitPolicy::None => false,
                HitPolicy::Bounds => e
                    .local_bounds
                    .is_some_and(|b| b.expanded(tolerance).contains(local)),
                HitPolicy::Geometry => {
                    object
                        .shape
                        .contains_geometry(local, object.style.fill_rule)
                        || near_contours(&object.shape, local, tolerance)
                }
                HitPolicy::Painted => object.shape.hit_test(local, &object.style, tolerance),
            });
            if hit {
                hits.push(HitTestResult {
                    id: id.clone(),
                    world_point: world,
                    local_point: local,
                    path: self.scene().ancestor_path(&id)?,
                });
            }
        }
        Ok(hits)
    }
}
fn checked_point(point: Point) -> Result<Point, EngineError> {
    if point.is_finite() {
        Ok(point)
    } else {
        Err(SceneError::InvalidGeometry("coordinate overflow".into()).into())
    }
}
fn near_contours(shape: &Shape, point: Point, tolerance: f32) -> bool {
    if tolerance <= 0. {
        return false;
    }
    shape.flatten(0.1).iter().any(|c| {
        let hit = c
            .points
            .windows(2)
            .any(|p| segment_distance(point, p[0], p[1]) <= tolerance);
        hit || (c.closed
            && c.points.len() > 1
            && segment_distance(point, *c.points.last().unwrap(), c.points[0]) <= tolerance)
    })
}
fn segment_distance(p: Point, a: Point, b: Point) -> f32 {
    let d = b - a;
    let len = d.x * d.x + d.y * d.y;
    if len <= f32::EPSILON {
        return p.distance(a);
    }
    let t = (((p.x - a.x) * d.x + (p.y - a.y) * d.y) / len).clamp(0., 1.);
    p.distance(a + d * t)
}
