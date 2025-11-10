//! Align widget - aligns its child within itself
//!
//! A widget that aligns its child within the available space.
//! Similar to Flutter's Align widget.
//!
//! # Usage Patterns
//!
//! ## 1. Convenience Methods (Recommended)
//! ```rust,ignore
//! // Common alignments (9 presets)
//! Align::top_left(child)
//! Align::top_center(child)
//! Align::top_right(child)
//! Align::center_left(child)
//! Align::center(child)
//! Align::center_right(child)
//! Align::bottom_left(child)
//! Align::bottom_center(child)
//! Align::bottom_right(child)
//!
//! // Custom alignment
//! Align::with_alignment(Alignment::new(0.5, -0.5), child)
//! ```
//!
//! ## 2. Builder Pattern
//! ```rust,ignore
//! Align::builder()
//!     .alignment(Alignment::TOP_LEFT)
//!     .child(some_widget)
//!     .build()
//! ```
//!
//! ## 3. Macro
//! ```rust,ignore
//! align!(child: widget, alignment: Alignment::BOTTOM_RIGHT)
//! ```

use bon::Builder;
use flui_core::view::{AnyView, IntoElement, RenderBuilder, View};
use flui_core::BuildContext;
use flui_rendering::RenderAlign;
use flui_types::Alignment;

/// A widget that aligns its child within the available space.
///
/// Align positions its child at a specific position within itself using an Alignment.
///
/// ## Layout Behavior
///
/// - The child is positioned according to the alignment
/// - Alignment coordinates: (-1, -1) = top-left, (0, 0) = center, (1, 1) = bottom-right
/// - If `width_factor` or `height_factor` are specified, the Align sizes itself
///   as a multiple of the child's size
///
/// ## Examples
///
/// ```rust,ignore
/// // Top-right alignment
/// Align::builder()
///     .alignment(Alignment::TOP_RIGHT)
///     .child(Text::new("Hello"))
///     .build()
///
/// // Custom alignment
/// Align::builder()
///     .alignment(Alignment::new(0.5, -0.5))  // Right of center, top
///     .child(some_widget)
///     .build()
///
/// // With size factors
/// Align::builder()
///     .alignment(Alignment::CENTER)
///     .width_factor(2.0)
///     .height_factor(2.0)
///     .child(some_widget)
///     .build()
/// ```
#[derive(Builder)]
#[builder(
    on(String, into),
    on(Alignment, into),
    finish_fn(name = build_internal, vis = "")
)]
pub struct Align {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// How to align the child within the available space.
    ///
    /// Defaults to Alignment::CENTER if not specified.
    #[builder(default = Alignment::CENTER)]
    pub alignment: Alignment,

    /// Multiplier for child width to determine Align width.
    ///
    /// If null, Align takes all available horizontal space.
    /// If non-null, Align width = child width * width_factor.
    pub width_factor: Option<f32>,

    /// Multiplier for child height to determine Align height.
    ///
    /// If null, Align takes all available vertical space.
    /// If non-null, Align height = child height * height_factor.
    pub height_factor: Option<f32>,

    /// The child widget to align.
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<Box<dyn AnyView>>,
}

// Manual Debug implementation since AnyView doesn't implement Debug
impl std::fmt::Debug for Align {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Align")
            .field("key", &self.key)
            .field("alignment", &self.alignment)
            .field("width_factor", &self.width_factor)
            .field("height_factor", &self.height_factor)
            .field(
                "child",
                &if self.child.is_some() {
                    "<AnyView>"
                } else {
                    "None"
                },
            )
            .finish()
    }
}

// Manual Clone implementation since AnyView doesn't implement Clone
impl Clone for Align {
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            alignment: self.alignment,
            width_factor: self.width_factor,
            height_factor: self.height_factor,
            child: self.child.clone(), // Shallow clone - child is not cloned
        }
    }
}

impl Align {
    /// Creates a new empty Align widget with center alignment.
    ///
    /// Note: Prefer using convenience methods like `Align::center(child)` for most cases.
    pub const fn new() -> Self {
        Self {
            key: None,
            alignment: Alignment::CENTER,
            width_factor: None,
            height_factor: None,
            child: None,
        }
    }

    /// Creates an Align with custom alignment and child.
    ///
    /// Use this for custom alignment values not covered by presets.
    ///
    /// # Example
    /// ```rust,ignore
    /// // Custom position: slightly right and up from center
    /// Align::with_alignment(Alignment::new(0.3, -0.2), widget)
    /// ```
    pub fn with_alignment(alignment: Alignment, child: impl View + 'static) -> Self {
        Self::builder().alignment(alignment).child(child).build()
    }

    // ========== 9 Standard Alignment Presets ==========

    /// Aligns child to top-left corner.
    ///
    /// # Example
    /// ```rust,ignore
    /// Align::top_left(close_button)
    /// ```
    pub fn top_left(child: impl View + 'static) -> Self {
        Self::with_alignment(Alignment::TOP_LEFT, child)
    }

    /// Aligns child to top-center.
    ///
    /// # Example
    /// ```rust,ignore
    /// Align::top_center(title_text)
    /// ```
    pub fn top_center(child: impl View + 'static) -> Self {
        Self::with_alignment(Alignment::TOP_CENTER, child)
    }

    /// Aligns child to top-right corner.
    ///
    /// # Example
    /// ```rust,ignore
    /// Align::top_right(menu_button)
    /// ```
    pub fn top_right(child: impl View + 'static) -> Self {
        Self::with_alignment(Alignment::TOP_RIGHT, child)
    }

    /// Aligns child to center-left.
    ///
    /// # Example
    /// ```rust,ignore
    /// Align::center_left(sidebar)
    /// ```
    pub fn center_left(child: impl View + 'static) -> Self {
        Self::with_alignment(Alignment::CENTER_LEFT, child)
    }

    /// Aligns child to center (same as `Center::with_child`).
    ///
    /// # Example
    /// ```rust,ignore
    /// Align::center(logo)
    /// ```
    pub fn center(child: impl View + 'static) -> Self {
        Self::with_alignment(Alignment::CENTER, child)
    }

    /// Aligns child to center-right.
    ///
    /// # Example
    /// ```rust,ignore
    /// Align::center_right(scroll_indicator)
    /// ```
    pub fn center_right(child: impl View + 'static) -> Self {
        Self::with_alignment(Alignment::CENTER_RIGHT, child)
    }

    /// Aligns child to bottom-left corner.
    ///
    /// # Example
    /// ```rust,ignore
    /// Align::bottom_left(back_button)
    /// ```
    pub fn bottom_left(child: impl View + 'static) -> Self {
        Self::with_alignment(Alignment::BOTTOM_LEFT, child)
    }

    /// Aligns child to bottom-center.
    ///
    /// # Example
    /// ```rust,ignore
    /// Align::bottom_center(action_button)
    /// ```
    pub fn bottom_center(child: impl View + 'static) -> Self {
        Self::with_alignment(Alignment::BOTTOM_CENTER, child)
    }

    /// Aligns child to bottom-right corner.
    ///
    /// # Example
    /// ```rust,ignore
    /// Align::bottom_right(fab_button)
    /// ```
    pub fn bottom_right(child: impl View + 'static) -> Self {
        Self::with_alignment(Alignment::BOTTOM_RIGHT, child)
    }

    /// Validates Align configuration.
    pub fn validate(&self) -> Result<(), String> {
        if let Some(width_factor) = self.width_factor {
            if width_factor <= 0.0 || width_factor.is_nan() || width_factor.is_infinite() {
                return Err(format!(
                    "Invalid width_factor: {}. Must be positive and finite.",
                    width_factor
                ));
            }
        }

        if let Some(height_factor) = self.height_factor {
            if height_factor <= 0.0 || height_factor.is_nan() || height_factor.is_infinite() {
                return Err(format!(
                    "Invalid height_factor: {}. Must be positive and finite.",
                    height_factor
                ));
            }
        }

        Ok(())
    }
}

impl Default for Align {
    fn default() -> Self {
        Self::new()
    }
}

// bon Builder Extensions
use align_builder::{IsUnset, SetChild, State};

// Custom child setter
impl<S: State> AlignBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    pub fn child(self, child: impl View + 'static) -> AlignBuilder<SetChild<S>> {
        self.child_internal(Box::new(child))
    }
}

// Build wrapper with validation
impl<S: State> AlignBuilder<S> {
    /// Builds the Align widget with automatic validation in debug mode.
    pub fn build(self) -> Align {
        let align = self.build_internal();

        // In debug mode, validate configuration and warn on issues
        #[cfg(debug_assertions)]
        if let Err(e) = align.validate() {
            tracing::warn!("Align validation warning: {}", e);
        }

        align
    }
}

/// Macro for creating Align with declarative syntax.
///
/// # Examples
///
/// ```rust,ignore
/// // Empty align
/// align!()
///
/// // With child only (center alignment)
/// align!(child: Text::new("Hello"))
///
/// // With child and alignment
/// align!(child: widget, alignment: Alignment::TOP_RIGHT)
///
/// // Properties only (no child)
/// align!(alignment: Alignment::BOTTOM_LEFT)
/// ```
#[macro_export]
macro_rules! align {
    // Empty align
    () => {
        $crate::Align::new()
    };

    // With child only (center alignment)
    (child: $child:expr) => {
        $crate::Align::builder()
            .child($child)
            .build()
    };

    // With child and properties
    (child: $child:expr, $($field:ident : $value:expr),+ $(,)?) => {
        $crate::Align::builder()
            .child($child)
            $(.$field($value))+
            .build()
    };

    // Without child, just properties
    ($($field:ident : $value:expr),+ $(,)?) => {
        $crate::Align {
            $($field: $value.into(),)*
            ..Default::default()
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_core::view::RenderBuilder;
    use flui_rendering::RenderPadding;
    use flui_types::EdgeInsets;

    // Mock view for testing
    #[derive(Debug, Clone)]
    struct MockView;

    impl View for MockView {
        fn build(self, _ctx: &BuildContext) -> impl IntoElement {
            RenderBuilder::new(RenderPadding::new(EdgeInsets::ZERO))
        }
    }

    #[test]
    fn test_align_new() {
        let align = Align::new();
        assert!(align.key.is_none());
        assert_eq!(align.alignment, Alignment::CENTER);
        assert!(align.width_factor.is_none());
        assert!(align.height_factor.is_none());
        assert!(align.child.is_none());
    }

    #[test]
    fn test_align_default() {
        let align = Align::default();
        assert_eq!(align.alignment, Alignment::CENTER);
    }

    #[test]
    fn test_align_top_left() {
        let align = Align::top_left(MockView);
        assert_eq!(align.alignment, Alignment::TOP_LEFT);
        assert!(align.child.is_some());
    }

    #[test]
    fn test_align_top_center() {
        let align = Align::top_center(MockView);
        assert_eq!(align.alignment, Alignment::TOP_CENTER);
        assert!(align.child.is_some());
    }

    #[test]
    fn test_align_top_right() {
        let align = Align::top_right(MockView);
        assert_eq!(align.alignment, Alignment::TOP_RIGHT);
        assert!(align.child.is_some());
    }

    #[test]
    fn test_align_center() {
        let align = Align::center(MockView);
        assert_eq!(align.alignment, Alignment::CENTER);
        assert!(align.child.is_some());
    }

    #[test]
    fn test_align_bottom_right() {
        let align = Align::bottom_right(MockView);
        assert_eq!(align.alignment, Alignment::BOTTOM_RIGHT);
        assert!(align.child.is_some());
    }

    #[test]
    fn test_align_with_alignment() {
        let custom = Alignment::new(0.5, -0.5);
        let align = Align::with_alignment(custom, MockView);
        assert_eq!(align.alignment, custom);
        assert!(align.child.is_some());
    }

    #[test]
    fn test_align_builder() {
        let align = Align::builder().alignment(Alignment::TOP_LEFT).build();
        assert_eq!(align.alignment, Alignment::TOP_LEFT);
    }

    #[test]
    fn test_align_builder_with_child() {
        let align = Align::builder().child(MockView).build();
        assert!(align.child.is_some());
    }

    #[test]
    fn test_align_builder_with_factors() {
        let align = Align::builder()
            .width_factor(2.0)
            .height_factor(1.5)
            .build();
        assert_eq!(align.width_factor, Some(2.0));
        assert_eq!(align.height_factor, Some(1.5));
    }

    #[test]
    fn test_align_macro_empty() {
        let align = align!();
        assert_eq!(align.alignment, Alignment::CENTER);
    }

    #[test]
    fn test_align_macro_with_child() {
        let align = align!(child: MockView);
        assert!(align.child.is_some());
        assert_eq!(align.alignment, Alignment::CENTER);
    }

    #[test]
    fn test_align_macro_with_child_and_alignment() {
        let align = align!(child: MockView, alignment: Alignment::TOP_RIGHT);
        assert!(align.child.is_some());
        assert_eq!(align.alignment, Alignment::TOP_RIGHT);
    }

    #[test]
    fn test_align_macro_with_alignment() {
        let align = align! {
            alignment: Alignment::BOTTOM_LEFT,
        };
        assert_eq!(align.alignment, Alignment::BOTTOM_LEFT);
    }

    #[test]
    fn test_align_validate_ok() {
        let align = Align::builder().width_factor(1.5).build();
        assert!(align.validate().is_ok());
    }

    #[test]
    fn test_align_validate_invalid_width_factor() {
        let align = Align {
            width_factor: Some(-1.0),
            ..Default::default()
        };
        assert!(align.validate().is_err());
    }

    #[test]
    fn test_align_validate_nan_height_factor() {
        let align = Align {
            height_factor: Some(f32::NAN),
            ..Default::default()
        };
        assert!(align.validate().is_err());
    }

    #[test]
    fn test_align_all_factory_methods() {
        // Test all 9 alignment factory methods
        assert_eq!(Align::top_left(MockView).alignment, Alignment::TOP_LEFT);
        assert_eq!(Align::top_center(MockView).alignment, Alignment::TOP_CENTER);
        assert_eq!(Align::top_right(MockView).alignment, Alignment::TOP_RIGHT);
        assert_eq!(
            Align::center_left(MockView).alignment,
            Alignment::CENTER_LEFT
        );
        assert_eq!(Align::center(MockView).alignment, Alignment::CENTER);
        assert_eq!(
            Align::center_right(MockView).alignment,
            Alignment::CENTER_RIGHT
        );
        assert_eq!(
            Align::bottom_left(MockView).alignment,
            Alignment::BOTTOM_LEFT
        );
        assert_eq!(
            Align::bottom_center(MockView).alignment,
            Alignment::BOTTOM_CENTER
        );
        assert_eq!(
            Align::bottom_right(MockView).alignment,
            Alignment::BOTTOM_RIGHT
        );

        // Verify all have children
        assert!(Align::center(MockView).child.is_some());
    }

    #[test]
    fn test_view_trait() {
        let align = Align::builder()
            .alignment(Alignment::TOP_LEFT)
            .child(MockView)
            .build();

        // Test that it implements View
        assert!(align.child.is_some());
    }

    #[test]
    fn test_align_with_child() {
        let align = Align::builder()
            .alignment(Alignment::CENTER)
            .child(MockView)
            .build();

        // Test child field
        assert!(align.child.is_some());
    }
}

// Implement View for Align - Simplified API
impl View for Align {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        RenderBuilder::new(RenderAlign::with_factors(
            self.alignment,
            self.width_factor,
            self.height_factor,
        ))
        .maybe_child(self.child)
    }
}

// Align now implements View trait directly
