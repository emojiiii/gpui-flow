//! Explicit state geometry. Unsupported shape sizes fail instead of approximating
//! their geometry by an affine scale or silently ignoring a property.
use super::{SceneStateStyles, StateStyleError, VisualState, named::validate_name};
use gpui_flow_engine::{ElementHandle, LayoutProviderId, SceneEngine};
use gpui_flow_scene::{DisplayObject, Scene, Shape, Size};
use std::collections::BTreeMap;

/// Absolute local translation and intrinsic box dimensions. `x`/`y` replace only
/// the affine translation; all rotation, scale and skew coefficients are kept.
/// Sizes require Rect, Ellipse, Text, Image, Box or Frame.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct StateGeometry {
    pub x: Option<f32>,
    pub y: Option<f32>,
    pub width: Option<f32>,
    pub height: Option<f32>,
}
impl StateGeometry {
    pub(super) fn merge(&mut self, patch: &Self) {
        self.x = patch.x.or(self.x);
        self.y = patch.y.or(self.y);
        self.width = patch.width.or(self.width);
        self.height = patch.height.or(self.height);
    }
    pub(super) fn apply(&self, object: &mut DisplayObject) -> Result<(), StateStyleError> {
        if [self.x, self.y, self.width, self.height]
            .into_iter()
            .flatten()
            .any(|v| !v.is_finite())
            || [self.width, self.height]
                .into_iter()
                .flatten()
                .any(|v| v <= 0.)
        {
            return Err(StateStyleError::InvalidStyle(
                "state geometry must be finite and dimensions strictly positive".into(),
            ));
        }
        if let Some(x) = self.x {
            object.transform.tx = x;
        }
        if let Some(y) = self.y {
            object.transform.ty = y;
        }
        if self.width.is_some() || self.height.is_some() {
            let size = shape_size_mut(&mut object.shape).ok_or_else(|| {
                StateStyleError::InvalidStyle(
                    "state width/height require Rect, Ellipse, Text, Image, Box or Frame".into(),
                )
            })?;
            if let Some(width) = self.width {
                size.width = width;
            }
            if let Some(height) = self.height {
                size.height = height;
            }
        }
        object.validate().map_err(StateStyleError::InvalidStyle)
    }
}
/// Same property-by-property precedence as paint, including disabled suppression.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct StateGeometryStyles {
    pub selected: StateGeometry,
    pub placeholder: StateGeometry,
    pub focus: StateGeometry,
    pub hover: StateGeometry,
    pub press: StateGeometry,
    pub disabled: StateGeometry,
}
impl StateGeometryStyles {
    pub(super) fn resolve(
        &self,
        state: VisualState,
        named: Option<&StateGeometry>,
        placeholder: bool,
    ) -> StateGeometry {
        let mut next = named.copied().unwrap_or_default();
        if state.selected {
            next.merge(&self.selected);
        }
        if placeholder {
            next.merge(&self.placeholder);
        }
        if state.disabled {
            next.merge(&self.disabled);
        } else {
            if state.focus {
                next.merge(&self.focus);
            }
            if state.hover {
                next.merge(&self.hover);
            }
            if state.press {
                next.merge(&self.press);
            }
        }
        next
    }
    fn patches(&self) -> [&StateGeometry; 6] {
        [
            &self.selected,
            &self.placeholder,
            &self.focus,
            &self.hover,
            &self.press,
            &self.disabled,
        ]
    }
}
pub(in crate::scene) fn shape_size(shape: &Shape) -> Option<Size> {
    match shape {
        Shape::Rect(s) => Some(s.size),
        Shape::Ellipse(s) => Some(s.size),
        Shape::Text(s) => Some(s.size),
        Shape::Image(s) => Some(s.size),
        Shape::Box(s) => Some(s.size),
        Shape::Frame(s) => Some(s.size),
        _ => None,
    }
}
pub(in crate::scene) fn shape_size_mut(shape: &mut Shape) -> Option<&mut Size> {
    match shape {
        Shape::Rect(s) => Some(&mut s.size),
        Shape::Ellipse(s) => Some(&mut s.size),
        Shape::Text(s) => Some(&mut s.size),
        Shape::Image(s) => Some(&mut s.size),
        Shape::Box(s) => Some(&mut s.size),
        Shape::Frame(s) => Some(&mut s.size),
        _ => None,
    }
}
pub(super) struct GeometryPresentation {
    pub revision: u64,
    pub document_epoch: u64,
    pub layout_provider: Option<LayoutProviderId>,
    pub engine: SceneEngine,
}
impl SceneStateStyles {
    /// Validate every geometry patch before replacing this binding's settings.
    /// Active combinations and reactive layout ownership are checked atomically
    /// when presenting. Inactive unsupported shape dimensions fail here too.
    pub fn set_geometry_styles(
        &mut self,
        engine: &SceneEngine,
        handle: ElementHandle,
        styles: StateGeometryStyles,
    ) -> Result<(), StateStyleError> {
        self.check(engine)?;
        let object = engine.object(handle)?;
        for patch in styles.patches() {
            patch.apply(&mut object.clone())?;
        }
        let binding = self
            .bindings
            .get_mut(&handle)
            .ok_or(StateStyleError::Unbound)?;
        binding.geometry = styles;
        Ok(())
    }
    /// Replace the named geometry palette; names may be defined by paint,
    /// geometry, or both. Inherited names always use the child's own patches.
    pub fn set_named_geometry(
        &mut self,
        engine: &SceneEngine,
        handle: ElementHandle,
        states: BTreeMap<String, StateGeometry>,
    ) -> Result<(), StateStyleError> {
        self.check(engine)?;
        let object = engine.object(handle)?;
        if !self.bindings.contains_key(&handle) {
            return Err(StateStyleError::Unbound);
        }
        if states.len() > super::MAX_NAMED_STATES_PER_BINDING {
            return Err(StateStyleError::Capacity);
        }
        for (name, patch) in &states {
            validate_name(name)?;
            patch.apply(&mut object.clone())?;
        }
        self.bindings
            .get_mut(&handle)
            .expect("checked binding")
            .named_geometry = states;
        Ok(())
    }
    /// Retire picking after a host fails to convert/draw the accepted frame and
    /// falls back to the canonical scene. Does not alter motion or the clock.
    pub fn invalidate_geometry_presentation(&mut self) {
        self.geometry_presentation = None;
    }
    /// Cached geometry of the last accepted presentation, with canonical paint
    /// and text content. Use its existing engine hit/clip/custom rules;
    /// use the canonical engine for resource tickets and live handles. There is
    /// no cache when geometry is canonical, after a failed frame, or after a
    /// canonical edit/reset. This never advances the presentation clock.
    pub fn geometry_engine(&self, canonical: &SceneEngine) -> Option<&SceneEngine> {
        if canonical.engine_id() != self.engine {
            return None;
        }
        self.geometry_presentation
            .as_ref()
            .filter(|snapshot| {
                snapshot.revision == canonical.revision()
                    && snapshot.document_epoch == canonical.document_epoch()
                    && snapshot.layout_provider == canonical.layout_provider()
            })
            .map(|snapshot| &snapshot.engine)
    }
    pub fn geometry_presentation(&self, canonical: &SceneEngine) -> Option<&Scene> {
        self.geometry_engine(canonical).map(SceneEngine::scene)
    }
}
