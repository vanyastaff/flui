//! RenderAnimatedOpacityMixin trait - animated opacity support.

use crate::traits::RenderObject;

/// Trait for render objects that support animated opacity.
///
/// # Flutter Equivalence
///
/// This corresponds to `RenderAnimatedOpacityMixin<T extends RenderObject>` in Flutter.
///
/// ```dart
/// mixin RenderAnimatedOpacityMixin<T extends RenderObject>
///     on RenderObjectWithChildMixin<T> {
///   Animation<double>? get opacity;
///   set opacity(Animation<double>? value);
///   // ...
/// }
/// ```
///
/// # Usage
///
/// This mixin provides shared logic for animated opacity that can be used
/// by both `RenderAnimatedOpacity` (box) and `RenderSliverAnimatedOpacity` (sliver).
///
/// # Opacity Behavior
///
/// - Opacity of 0.0 is fully transparent
/// - Opacity of 1.0 is fully opaque
/// - Values between 0.0 and 1.0 are semi-transparent
///
/// # Performance
///
/// When opacity is exactly 0.0, the child is not painted at all.
/// When opacity is exactly 1.0, no opacity layer is used.
/// For values in between, an opacity layer is created for compositing.
pub trait RenderAnimatedOpacityMixin: RenderObject {
    // ========================================================================
    // Opacity Animation
    // ========================================================================

    /// Returns the current opacity value.
    ///
    /// This should return the current value from the opacity animation,
    /// or a static value if no animation is active.
    ///
    /// # Range
    ///
    /// The value should be in the range [0.0, 1.0] where:
    /// - 0.0 = fully transparent
    /// - 1.0 = fully opaque
    fn opacity_value(&self) -> f32;

    /// Sets the opacity value directly (for non-animated usage).
    ///
    /// For animated usage, use `set_opacity_animation` instead.
    fn set_opacity_value(&mut self, opacity: f32);

    /// Returns whether an opacity animation is currently active.
    fn has_opacity_animation(&self) -> bool;

    /// Returns whether the child should always be included in the semantics tree.
    ///
    /// When `false`, the child's semantics are excluded when fully transparent.
    /// When `true`, semantics are always included regardless of opacity.
    ///
    /// # Flutter Equivalence
    ///
    /// This corresponds to `alwaysIncludeSemantics` in Flutter's
    /// `RenderAnimatedOpacity`.
    fn always_include_semantics(&self) -> bool {
        false
    }

    /// Sets whether the child should always be included in semantics.
    fn set_always_include_semantics(&mut self, value: bool);

    // ========================================================================
    // Computed Properties
    // ========================================================================

    /// Returns whether the child is completely invisible.
    ///
    /// When true, the child should not be painted at all.
    #[inline]
    fn is_fully_transparent(&self) -> bool {
        self.opacity_value() <= 0.0
    }

    /// Returns whether the child is completely visible.
    ///
    /// When true, no opacity layer is needed.
    #[inline]
    fn is_fully_opaque(&self) -> bool {
        self.opacity_value() >= 1.0
    }

    /// Returns whether an opacity layer is needed.
    ///
    /// An opacity layer is needed when opacity is between 0 and 1 (exclusive).
    #[inline]
    fn needs_opacity_layer(&self) -> bool {
        !self.is_fully_transparent() && !self.is_fully_opaque()
    }

    /// Returns the opacity as an alpha value (0-255).
    ///
    /// This is useful for painting APIs that use integer alpha values.
    #[inline]
    fn opacity_alpha(&self) -> u8 {
        (self.opacity_value().clamp(0.0, 1.0) * 255.0).round() as u8
    }

    // ========================================================================
    // Lifecycle
    // ========================================================================

    /// Called when the opacity value changes.
    ///
    /// Implementations should mark the render object for repaint.
    fn on_opacity_changed(&mut self) {
        self.mark_needs_paint();
    }
}

/// Default opacity values for common use cases.
pub mod opacity_constants {
    /// Fully transparent (invisible).
    pub const TRANSPARENT: f32 = 0.0;

    /// Fully opaque (completely visible).
    pub const OPAQUE: f32 = 1.0;

    /// Half transparent.
    pub const HALF: f32 = 0.5;

    /// Threshold below which the object is considered invisible for hit testing.
    /// In Flutter, objects with opacity below this may still be hit-testable
    /// depending on the `alwaysIncludeSemantics` setting.
    pub const HIT_TEST_THRESHOLD: f32 = 0.001;
}

#[cfg(test)]
mod tests {
    use super::opacity_constants::*;

    #[test]
    fn test_opacity_constants() {
        assert_eq!(TRANSPARENT, 0.0);
        assert_eq!(OPAQUE, 1.0);
        assert_eq!(HALF, 0.5);
    }
}
