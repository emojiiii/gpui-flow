use crate::{Bounds, Contour};
use serde::{Deserialize, Serialize};
use serde_json::Value;
/// An application-defined element with serializable fallback vector geometry.
/// `kind` is the renderer/factory registry key; `data` holds type-specific
/// properties. Bounds must enclose the supplied contours. This crate never
/// executes application code during deserialization.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Custom {
    pub kind: String,
    pub bounds: Bounds,
    #[serde(default)]
    pub contours: Vec<Contour>,
    #[serde(default)]
    pub data: Value,
}
impl Custom {
    pub fn new(kind: impl Into<String>, bounds: Bounds, contours: Vec<Contour>) -> Self {
        Self {
            kind: kind.into(),
            bounds,
            contours,
            data: Value::Null,
        }
    }
    pub fn with_data(mut self, data: Value) -> Self {
        self.data = data;
        self
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.kind.trim().is_empty() {
            return Err("custom element kind cannot be empty".into());
        }
        if !self.bounds.is_valid() {
            return Err("custom element bounds must be finite and non-negative".into());
        }
        for contour in &self.contours {
            if contour.points.len() < (if contour.closed { 3 } else { 1 }) {
                return Err("custom contour has too few vertices".into());
            }
            if contour
                .points
                .iter()
                .any(|p| !p.is_finite() || !self.bounds.contains(*p))
            {
                return Err(
                    "custom fallback geometry must be finite and enclosed by declared bounds"
                        .into(),
                );
            }
        }
        Ok(())
    }
}
