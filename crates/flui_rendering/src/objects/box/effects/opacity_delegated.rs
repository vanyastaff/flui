//! RenderOpacityDelegated - example of ambassador delegation pattern.
//!
//! This demonstrates how to use `#[derive(Delegate)]` to automatically
//! forward trait methods to a container field.

use ambassador::Delegate;
use flui_types::{Offset, Size};

use crate::constraints::BoxConstraints;
use crate::containers::ProxyBox;
use crate::pipeline::PaintingContext;
// Import both the trait AND the generated macro
use crate::traits::r#box::{ambassador_impl_ProxyBoxBehavior, ProxyBoxBehavior};
// These imports are needed for the generated delegation code
use crate::traits::r#box::{BoxHitTestResult, RenderBox};

/// A render object that applies opacity to its child.
///
/// This version uses ambassador's `#[derive(Delegate)]` macro to
/// automatically forward `ProxyBoxBehavior` methods to the `proxy` field.
///
/// # Delegation Pattern
///
/// ```text
/// RenderOpacityDelegated
///     │
///     ├── proxy: ProxyBox  ←── ProxyBoxBehavior methods delegated here
///     │       ├── proxy_child()
///     │       ├── proxy_perform_layout()
///     │       ├── proxy_paint()
///     │       └── proxy_hit_test_children()
///     │
///     └── opacity: f32     ←── Custom state
/// ```
///
/// # Usage
///
/// ```rust,ignore
/// let mut opacity = RenderOpacityDelegated::new(0.5);
///
/// // These methods are automatically delegated to proxy:
/// let size = opacity.proxy_perform_layout(constraints);
/// let has_child = opacity.proxy_has_child();
///
/// // Custom behavior can override delegated methods
/// opacity.paint_with_opacity(context, offset);
/// ```
#[derive(Debug, Delegate)]
#[delegate(ProxyBoxBehavior, target = "proxy")]
pub struct RenderOpacityDelegated {
    /// Container holding the child and geometry.
    proxy: ProxyBox,

    /// The opacity value (0.0 to 1.0).
    opacity: f32,

    /// Whether the child should be included in hit testing when invisible.
    always_include_semantics: bool,
}

impl RenderOpacityDelegated {
    /// Creates a new opacity render object.
    ///
    /// The opacity is clamped to [0.0, 1.0].
    pub fn new(opacity: f32) -> Self {
        Self {
            proxy: ProxyBox::new(),
            opacity: opacity.clamp(0.0, 1.0),
            always_include_semantics: false,
        }
    }

    /// Creates a fully opaque render object.
    pub fn opaque() -> Self {
        Self::new(1.0)
    }

    /// Creates a fully transparent render object.
    pub fn transparent() -> Self {
        Self::new(0.0)
    }

    /// Returns the current opacity.
    pub fn opacity(&self) -> f32 {
        self.opacity
    }

    /// Sets the opacity value.
    ///
    /// The value is clamped to [0.0, 1.0].
    pub fn set_opacity(&mut self, opacity: f32) {
        let clamped = opacity.clamp(0.0, 1.0);
        if (self.opacity - clamped).abs() > f32::EPSILON {
            self.opacity = clamped;
            // In real implementation: self.mark_needs_paint();
        }
    }

    /// Returns whether semantics are always included.
    pub fn always_include_semantics(&self) -> bool {
        self.always_include_semantics
    }

    /// Sets whether semantics should always be included.
    pub fn set_always_include_semantics(&mut self, value: bool) {
        if self.always_include_semantics != value {
            self.always_include_semantics = value;
        }
    }

    /// Returns whether the child is effectively invisible.
    pub fn is_invisible(&self) -> bool {
        self.opacity < 0.001
    }

    /// Returns whether the opacity creates any effect.
    pub fn is_opaque(&self) -> bool {
        self.opacity > 0.999
    }

    // ========================================================================
    // Custom behavior (overrides delegation)
    // ========================================================================

    /// Custom paint implementation with opacity handling.
    ///
    /// This method demonstrates how to add custom behavior while
    /// still using delegated methods for common operations.
    pub fn paint_with_opacity(&self, context: &mut PaintingContext, offset: Offset) {
        if self.is_invisible() {
            // Don't paint anything when fully transparent
            return;
        }

        if self.is_opaque() {
            // Delegate directly to proxy when fully opaque
            self.proxy_paint(context, offset);
        } else {
            // Apply opacity layer
            // In real implementation:
            // context.push_opacity(self.opacity, |ctx| {
            //     self.proxy_paint(ctx, offset);
            // });
            let _ = (context, offset);
        }
    }

    /// Layout with size stored in proxy.
    pub fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // Delegation handles the actual layout
        self.proxy_perform_layout(constraints)
    }

    /// Returns the current size (delegated).
    pub fn size(&self) -> Size {
        self.proxy_size()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_delegated_opacity_new() {
        let opacity = RenderOpacityDelegated::new(0.5);
        assert!((opacity.opacity() - 0.5).abs() < f32::EPSILON);
        assert!(!opacity.proxy_has_child()); // Delegated method
    }

    #[test]
    fn test_delegated_opacity_clamping() {
        let under = RenderOpacityDelegated::new(-0.5);
        assert!((under.opacity() - 0.0).abs() < f32::EPSILON);

        let over = RenderOpacityDelegated::new(1.5);
        assert!((over.opacity() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_delegated_proxy_methods() {
        let opacity = RenderOpacityDelegated::new(0.5);

        // These methods are delegated to proxy
        assert!(!opacity.proxy_has_child());
        assert_eq!(opacity.proxy_size(), Size::ZERO);

        // Intrinsic dimensions (delegated)
        assert_eq!(opacity.proxy_compute_min_intrinsic_width(100.0), 0.0);
        assert_eq!(opacity.proxy_compute_max_intrinsic_width(100.0), 0.0);
    }

    #[test]
    fn test_delegated_layout() {
        let mut opacity = RenderOpacityDelegated::new(0.5);
        let constraints = BoxConstraints::new(0.0, 200.0, 0.0, 150.0);

        // No child - returns smallest
        let size = opacity.layout(constraints);
        assert_eq!(size, Size::ZERO);
        assert_eq!(opacity.size(), Size::ZERO);
    }

    #[test]
    fn test_delegated_dry_layout() {
        let opacity = RenderOpacityDelegated::new(0.5);
        let constraints = BoxConstraints::new(0.0, 200.0, 0.0, 150.0);

        // Delegated dry layout
        let size = opacity.proxy_compute_dry_layout(constraints);
        assert_eq!(size, Size::ZERO); // No child
    }

    #[test]
    fn test_invisible_and_opaque() {
        let transparent = RenderOpacityDelegated::transparent();
        assert!(transparent.is_invisible());
        assert!(!transparent.is_opaque());

        let opaque = RenderOpacityDelegated::opaque();
        assert!(!opaque.is_invisible());
        assert!(opaque.is_opaque());
    }
}
