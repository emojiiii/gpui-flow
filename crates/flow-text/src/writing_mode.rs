//! Flow axis layout over upright native clusters, independent of script direction.
use std::fmt;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum WritingMode {
    #[default]
    X,
    XReverse,
    Y,
    YReverse,
}
impl WritingMode {
    pub fn is_vertical(self) -> bool {
        matches!(self, Self::Y | Self::YReverse)
    }
    pub fn is_reverse(self) -> bool {
        matches!(self, Self::XReverse | Self::YReverse)
    }
}

/// A consumer must reject unavailable capabilities instead of ignoring the axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnsupportedWritingMode {
    Backend,
    NativeClusterOrder,
    Ellipsis,
    Decoration,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WritingModeError {
    Unsupported(UnsupportedWritingMode),
    Invalid(String),
}
impl From<String> for WritingModeError {
    fn from(value: String) -> Self {
        Self::Invalid(value)
    }
}
impl From<&str> for WritingModeError {
    fn from(value: &str) -> Self {
        Self::Invalid(value.into())
    }
}
impl fmt::Display for WritingModeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(reason) => write!(f, "unsupported text writing mode: {reason:?}"),
            Self::Invalid(reason) => f.write_str(reason),
        }
    }
}
impl std::error::Error for WritingModeError {}
