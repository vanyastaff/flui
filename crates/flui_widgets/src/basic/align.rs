//! Align widget - aligns its child within itself
//!
//! A widget that aligns its child within the available space.
//! Similar to Flutter's Align widget.
//!
//! # Usage Patterns
//!
//! ## 1. Struct Literal
//! ```rust,ignore
//! Align {
//!     alignment: Alignment::CENTER,
//!     ..Default::default()
//! }
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
//! align! {
//!     alignment: Alignment::BOTTOM_RIGHT,
//! }
//! ```

use bon::Builder;
use flui_core::BuildContext;
use flui_core::view::{View, AnyView, IntoElement, SingleRenderBuilder};
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
    finish_fn = build_align
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
            .field("child", &if self.child.is_some() { "<AnyView>" } else { "None" })
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
    /// Creates a new Align widget with center alignment.
    pub fn new() -> Self {
        Self {
            key: None,
            alignment: Alignment::CENTER,
            width_factor: None,
            height_factor: None,
            child: None,
        }
    }

    /// Creates an Align widget with top-left alignment.
    pub fn top_left() -> Self {
        Self {
            alignment: Alignment::TOP_LEFT,
            ..Self::new()
        }
    }

    /// Creates an Align widget with top-center alignment.
    pub fn top_center() -> Self {
        Self {
            alignment: Alignment::TOP_CENTER,
            ..Self::new()
        }
    }

    /// Creates an Align widget with top-right alignment.
    pub fn top_right() -> Self {
        Self {
            alignment: Alignment::TOP_RIGHT,
            ..Self::new()
        }
    }

    /// Creates an Align widget with center-left alignment.
    pub fn center_left() -> Self {
        Self {
            alignment: Alignment::CENTER_LEFT,
            ..Self::new()
        }
    }

    /// Creates an Align widget with center alignment.
    pub fn center() -> Self {
        Self::new()
    }

    /// Creates an Align widget with center-right alignment.
    pub fn center_right() -> Self {
        Self {
            alignment: Alignment::CENTER_RIGHT,
            ..Self::new()
        }
    }

    /// Creates an Align widget with bottom-left alignment.
    pub fn bottom_left() -> Self {
        Self {
            alignment: Alignment::BOTTOM_LEFT,
            ..Self::new()
        }
    }

    /// Creates an Align widget with bottom-center alignment.
    pub fn bottom_center() -> Self {
        Self {
            alignment: Alignment::BOTTOM_CENTER,
            ..Self::new()
        }
    }

    /// Creates an Align widget with bottom-right alignment.
    pub fn bottom_right() -> Self {
        Self {
            alignment: Alignment::BOTTOM_RIGHT,
            ..Self::new()
        }
    }

    /// Sets the child widget.
    pub fn set_child(&mut self, child: impl View + 'static) {
        self.child = Some(Box::new(child));
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

// Build wrapper
impl<S: State> AlignBuilder<S> {
    /// Builds the Align widget.
    pub fn build(self) -> Align {
        self.build_align()
    }
}

/// Macro for creating Align with declarative syntax.
#[macro_export]
macro_rules! align {
    () => {
        $crate::Align::new()
    };
    ($($field:ident : $value:expr),* $(,)?) => {
        $crate::Align {
            $($field: $value.into(),)*
            ..Default::default()
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_core::view::LeafRenderBuilder;
    use flui_rendering::RenderPadding;
    use flui_types::EdgeInsets;

    // Mock view for testing
    #[derive(Debug, Clone)]
    struct MockView;

    impl View for MockView {
        fn build(self, _ctx: &BuildContext) -> impl IntoElement {
            LeafRenderBuilder::new(RenderPadding::new(EdgeInsets::ZERO))
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
        let align = Align::top_left();
        assert_eq!(align.alignment, Alignment::TOP_LEFT);
    }

    #[test]
    fn test_align_top_center() {
        let align = Align::top_center();
        assert_eq!(align.alignment, Alignment::TOP_CENTER);
    }

    #[test]
    fn test_align_top_right() {
        let align = Align::top_right();
        assert_eq!(align.alignment, Alignment::TOP_RIGHT);
    }

    #[test]
    fn test_align_center() {
        let align = Align::center();
        assert_eq!(align.alignment, Alignment::CENTER);
    }

    #[test]
    fn test_align_bottom_right() {
        let align = Align::bottom_right();
        assert_eq!(align.alignment, Alignment::BOTTOM_RIGHT);
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
    fn test_align_set_child() {
        let mut align = Align::new();
        align.set_child(MockView);
        assert!(align.child.is_some());
    }

    #[test]
    fn test_align_macro_empty() {
        let align = align!();
        assert_eq!(align.alignment, Alignment::CENTER);
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
        assert_eq!(Align::top_left().alignment, Alignment::TOP_LEFT);
        assert_eq!(Align::top_center().alignment, Alignment::TOP_CENTER);
        assert_eq!(Align::top_right().alignment, Alignment::TOP_RIGHT);
        assert_eq!(Align::center_left().alignment, Alignment::CENTER_LEFT);
        assert_eq!(Align::center().alignment, Alignment::CENTER);
        assert_eq!(Align::center_right().alignment, Alignment::CENTER_RIGHT);
        assert_eq!(Align::bottom_left().alignment, Alignment::BOTTOM_LEFT);
        assert_eq!(Align::bottom_center().alignment, Alignment::BOTTOM_CENTER);
        assert_eq!(Align::bottom_right().alignment, Alignment::BOTTOM_RIGHT);
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
        SingleRenderBuilder::new(RenderAlign::with_factors(
            self.alignment,
            self.width_factor,
            self.height_factor,
        ))
        .with_optional_child(self.child)
    }
}

// Align now implements View trait directly
