//! The [`LayerBounds`] trait and its 11 production impls.
//!
//! Extracted from `layer/mod.rs`. The trait is a thin
//! capability sigil that lets generic tree-walk code ask "does this layer
//! type expose a `bounds()` accessor?" without matching every variant of the
//! [`Layer`] enum. Each impl forwards to the inherent `bounds()` method on
//! the concrete layer struct.
//!
//! Layers without intrinsic bounds (`OffsetLayer`, `OpacityLayer`,
//! `TransformLayer`, `ColorFilterLayer`, `ImageFilterLayer`, `FollowerLayer`)
//! deliberately do NOT implement this trait -- their visual extent depends on
//! their children.
//!
//! [`Layer`]: crate::layer::Layer

use flui_types::geometry::{Pixels, Rect};

use super::{
    AnnotatedRegionLayer, BackdropFilterLayer, CanvasLayer, ClipPathLayer, ClipRRectLayer,
    ClipRectLayer, ClipSuperellipseLayer, LeaderLayer, PlatformViewLayer, ShaderMaskLayer,
    TextureLayer,
};

/// Trait for layers that have intrinsic bounds.
///
/// Implemented by leaf layers (Canvas, Picture, Texture, PlatformView,
/// PerformanceOverlay), clip layers (ClipRect, ClipRRect, ClipPath,
/// ClipSuperellipse), and effect layers with explicit bounds (ShaderMask,
/// BackdropFilter, Leader, AnnotatedRegion).
///
/// Container layers without intrinsic bounds (Offset, Transform, Opacity,
/// ColorFilter, ImageFilter, Follower) intentionally do not implement this
/// trait.
pub trait LayerBounds {
    /// Returns the bounding rectangle of this layer.
    fn bounds(&self) -> Rect<Pixels>;
}

impl LayerBounds for CanvasLayer {
    fn bounds(&self) -> Rect<Pixels> {
        self.bounds()
    }
}

impl LayerBounds for ClipRectLayer {
    fn bounds(&self) -> Rect<Pixels> {
        self.bounds()
    }
}

impl LayerBounds for ClipRRectLayer {
    fn bounds(&self) -> Rect<Pixels> {
        self.bounds()
    }
}

impl LayerBounds for ClipPathLayer {
    fn bounds(&self) -> Rect<Pixels> {
        self.bounds()
    }
}

impl LayerBounds for ClipSuperellipseLayer {
    fn bounds(&self) -> Rect<Pixels> {
        self.bounds()
    }
}

impl LayerBounds for ShaderMaskLayer {
    fn bounds(&self) -> Rect<Pixels> {
        self.bounds()
    }
}

impl LayerBounds for BackdropFilterLayer {
    fn bounds(&self) -> Rect<Pixels> {
        self.bounds()
    }
}

impl LayerBounds for TextureLayer {
    fn bounds(&self) -> Rect<Pixels> {
        self.bounds()
    }
}

impl LayerBounds for PlatformViewLayer {
    fn bounds(&self) -> Rect<Pixels> {
        self.bounds()
    }
}

impl LayerBounds for LeaderLayer {
    fn bounds(&self) -> Rect<Pixels> {
        self.bounds()
    }
}

impl LayerBounds for AnnotatedRegionLayer {
    fn bounds(&self) -> Rect<Pixels> {
        self.bounds()
    }
}
