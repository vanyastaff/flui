//! Unified geometry and constraint types for rendering.
//!
//! Re-exports from `flui_types` plus unified enums for protocol flexibility.

pub use flui_types::{BoxConstraints, Size, SliverConstraints, SliverGeometry};

/// Unified constraint type for different layout protocols.
#[derive(Debug, Clone, PartialEq)]
pub enum Constraints {
    Box(BoxConstraints),
    Sliver(SliverConstraints),
}

impl Constraints {
    #[inline]
    pub fn as_box(&self) -> BoxConstraints {
        match self {
            Self::Box(c) => *c,
            Self::Sliver(_) => panic!("Expected box constraints"),
        }
    }

    #[inline]
    pub fn as_sliver(&self) -> SliverConstraints {
        match self {
            Self::Sliver(c) => *c,
            Self::Box(_) => panic!("Expected sliver constraints"),
        }
    }

    #[inline]
    pub const fn is_box(&self) -> bool {
        matches!(self, Self::Box(_))
    }

    #[inline]
    pub const fn is_sliver(&self) -> bool {
        matches!(self, Self::Sliver(_))
    }
}

impl From<BoxConstraints> for Constraints {
    fn from(c: BoxConstraints) -> Self {
        Self::Box(c)
    }
}

impl From<SliverConstraints> for Constraints {
    fn from(c: SliverConstraints) -> Self {
        Self::Sliver(c)
    }
}

/// Unified geometry type for different layout protocols.
#[derive(Debug, Clone, PartialEq)]
pub enum Geometry {
    Box(Size),
    Sliver(SliverGeometry),
}

impl Geometry {
    #[inline]
    pub fn as_box(&self) -> Size {
        match self {
            Self::Box(s) => *s,
            Self::Sliver(_) => panic!("Expected box geometry"),
        }
    }

    #[inline]
    pub fn as_sliver(&self) -> SliverGeometry {
        match self {
            Self::Sliver(g) => *g,
            Self::Box(_) => panic!("Expected sliver geometry"),
        }
    }

    #[inline]
    pub const fn is_box(&self) -> bool {
        matches!(self, Self::Box(_))
    }

    #[inline]
    pub const fn is_sliver(&self) -> bool {
        matches!(self, Self::Sliver(_))
    }
}

impl From<Size> for Geometry {
    fn from(s: Size) -> Self {
        Self::Box(s)
    }
}

impl From<SliverGeometry> for Geometry {
    fn from(g: SliverGeometry) -> Self {
        Self::Sliver(g)
    }
}

impl Default for Geometry {
    fn default() -> Self {
        Self::Box(Size::ZERO)
    }
}
