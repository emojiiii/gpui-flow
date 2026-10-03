//! No pixel allocation: replay the bounded paint schedule against a private
//! ledger before the first domain output surface. This intentionally repeats
//! descendant work when a parent shadow samples it and when it is painted.
//! A rejected exponential shadow chain never reaches Gaussian or RGBA work.
use super::*;

pub(super) fn admit(
    raster: &Raster<'_>,
    root: &PreparedNode,
    source: &DomainSource,
) -> Result<(), CompositeError> {
    let mut ledger = raster.budget.clone();
    let mut plan = Admission {
        width: raster.width,
        height: raster.height,
        pixels: raster.pixels,
        world_to_layer: raster.world_to_layer,
        origin: raster.origin,
        budget: &mut ledger,
        cancelled: raster.cancelled,
    };
    plan.budget.reserve(plan.pixels * 4)?; // retained output
    if let Some(backdrop) = source.canvas {
        plan.budget.write(plan.pixels)?;
        if backdrop.grid.is_some_and(|g| g.color.a != 0) {
            plan.budget.write(plan.pixels)?; // conservative even if no grid dots
        }
    }
    plan.node(root, 0, false)?;
    if !source.clips.is_empty() {
        plan.budget.reserve(plan.pixels)?;
        for index in 0..source.clips.len() {
            plan.budget.write(plan.pixels)?;
            if index != 0 {
                plan.budget.write(plan.pixels)?;
                plan.budget.reserve(plan.pixels)?;
                plan.budget.in_use -= plan.pixels;
            }
        }
        plan.budget.write(plan.pixels)?;
        plan.budget.in_use -= plan.pixels;
    }
    plan.check()
}
struct Admission<'a> {
    width: u32,
    height: u32,
    pixels: u64,
    world_to_layer: Affine,
    origin: [f64; 2],
    budget: &'a mut PixelBudget,
    cancelled: &'a AtomicBool,
}
impl Admission<'_> {
    fn check(&self) -> Result<(), CompositeError> {
        if self.cancelled.load(Ordering::Relaxed) {
            Err(CompositeError::Superseded)
        } else {
            Ok(())
        }
    }
    fn node(
        &mut self,
        node: &PreparedNode,
        depth: usize,
        clip: bool,
    ) -> Result<(), CompositeError> {
        self.check()?;
        if !node.visible
            || node.opacity == 0.
            || node.outside(self.world_to_layer, self.origin, self.width, self.height)
        {
            return Ok(());
        }
        if node.eraser == EraserMode::Path {
            if node.erase_path.is_some() {
                self.budget.reserve(self.pixels)?;
                self.budget.write(self.pixels * 2)?;
                self.budget.in_use -= self.pixels;
            }
            return Ok(());
        }
        if let Some(plan) = node.device.as_ref().and_then(|d| d.blur.as_ref()) {
            if depth >= self.budget.limits.max_depth {
                return Err(CompositeError::NestingLimit);
            }
            self.budget.reserve(plan.bytes())?;
            self.budget.write(plan.work())?;
            let p = &plan.surface;
            Admission {
                width: p.width,
                height: p.height,
                pixels: p.pixels,
                world_to_layer: p.world_to_layer,
                origin: p.origin,
                budget: self.budget,
                cancelled: self.cancelled,
            }
            .contents(node, depth + 1, false, true)?;
            if node.grayscale > 0. {
                self.budget.write(p.pixels)?;
            }
            self.budget.write(self.pixels)?;
            self.budget.in_use -= plan.bytes();
        } else if node.isolated() {
            if depth >= self.budget.limits.max_depth {
                return Err(CompositeError::NestingLimit);
            }
            self.budget.reserve(self.pixels * 4)?;
            self.contents(node, depth + 1, false, true)?;
            if node.grayscale > 0. {
                self.budget.write(self.pixels)?;
            }
            self.budget.write(self.pixels)?;
            self.budget.in_use -= self.pixels * 4;
        } else {
            self.contents(node, depth, clip, true)?;
        }
        Ok(())
    }
    fn contents(
        &mut self,
        node: &PreparedNode,
        depth: usize,
        clip: bool,
        shadow: bool,
    ) -> Result<(), CompositeError> {
        self.check()?;
        // Effects may be inserted before or after children by the renderer.
        // Child clip/mask scratch is fully released before either insertion
        // point, so replaying effects first has identical peak/live-byte and
        // cumulative-work charges. Every source still replays all children.
        for (index, &effect, inner) in node
            .shadows
            .iter()
            .enumerate()
            .map(|(i, s)| (i, s, false))
            .chain(
                node.inner_shadows
                    .iter()
                    .enumerate()
                    .map(|(i, s)| (i, s, true)),
            )
            .filter(|_| shadow)
        {
            let plan = if let Some(device) = &node.device {
                if inner {
                    &device.inner_shadows[index]
                } else {
                    &device.shadows[index]
                }
                .clone()
            } else {
                crate::shadow::SourcePlan::new(effect, self.world_to_layer, self.budget.limits)?
            };
            let bytes = plan.filter.bytes(plan.pixels, plan.width);
            self.budget.reserve(bytes)?;
            self.budget
                .write(plan.filter.work(plan.pixels) + if inner { plan.pixels * 2 } else { 0 })?;
            Admission {
                width: plan.width,
                height: plan.height,
                pixels: plan.pixels,
                world_to_layer: plan.world_to_layer,
                origin: plan.origin,
                budget: self.budget,
                cancelled: self.cancelled,
            }
            .contents(node, depth, false, false)?;
            self.budget.write(self.pixels)?;
            self.budget.in_use -= bytes;
        }
        // Image-local rounded masks follow the same checked A8 path as child clips.
        if node
            .image
            .as_ref()
            .is_some_and(|image| image.own_clip.is_some())
        {
            self.budget.reserve(self.pixels)?;
            self.budget.write(self.pixels)?;
            if clip {
                self.budget.write(self.pixels * 2)?;
                self.budget.reserve(self.pixels)?;
                self.budget.in_use -= self.pixels;
            }
            self.budget.in_use -= self.pixels;
        }
        self.budget
            .write(self.pixels * (node.paints.len() as u64 + u64::from(node.image.is_some())))?;
        if node.child_clip.is_some() && !node.children.is_empty() {
            self.budget.reserve(self.pixels)?;
            self.budget.write(self.pixels)?;
            if clip {
                self.budget.write(self.pixels * 2)?;
                self.budget.reserve(self.pixels)?;
                self.budget.in_use -= self.pixels;
            }
            self.children(node, depth, true)?;
            self.budget.in_use -= self.pixels;
        } else {
            self.children(node, depth, clip)?;
        }
        Ok(())
    }
    fn children(
        &mut self,
        node: &PreparedNode,
        depth: usize,
        clip: bool,
    ) -> Result<(), CompositeError> {
        let mut i = 0;
        while i < node.children.len() {
            self.check()?;
            let child = &node.children[i];
            if child.mask == MaskMode::None {
                self.node(child, depth, clip)?;
                i += 1;
                continue;
            }
            self.budget.reserve(self.pixels * 4)?;
            self.node(child, depth, false)?;
            self.budget.reserve(self.pixels)?;
            self.budget.write(self.pixels)?; // alpha extraction
            self.budget.in_use -= self.pixels * 4;
            self.budget.reserve(self.pixels * 4)?;
            i += 1;
            while i < node.children.len() && node.children[i].mask == MaskMode::None {
                let content = &node.children[i];
                let flush = content.flushes_mask(child.opacity);
                if flush {
                    self.budget.write(self.pixels * 3)?;
                }
                self.node(content, depth, false)?;
                if flush {
                    self.budget.write(self.pixels * 3)?;
                }
                i += 1;
            }
            self.budget.write(self.pixels * 3)?;
            self.budget.in_use -= self.pixels * 5;
        }
        Ok(())
    }
}
