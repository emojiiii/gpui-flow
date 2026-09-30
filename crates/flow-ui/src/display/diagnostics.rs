//! Backend support is explicit; serializable scene features are not silently approximated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnsupportedFeature {
    GradientPaint,
    LayeredPaint,
    /// Unsupported non-vector stroke, invalid geometry, or a stroke work limit.
    StrokeGeometry,
    TransformedText,
    TransformedImage,
    ClippedText,
    ClippedImage,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderIssue {
    pub object: String,
    pub feature: UnsupportedFeature,
}
