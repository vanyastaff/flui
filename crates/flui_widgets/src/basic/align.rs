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
use flui_core::render::RenderNode;
use flui_core::widget::{RenderWidget, Widget};
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
#[derive(Debug, Clone, Builder)]
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
    pub child: Option<Widget>,
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
    pub fn set_child(&mut self, child: Widget) {
        self.child = Some(child);
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
    pub fn child(self, child: impl flui_core::IntoWidget) -> AlignBuilder<SetChild<S>> {
        self.child_internal(child.into_widget())
    }
}

// Build wrapper
impl<S: State> AlignBuilder<S> {
    /// Builds the Align widget and returns it as a Widget.
    pub fn build(self) -> flui_core::Widget {
        flui_core::Widget::render(self.build_align())
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
    use flui_core::LeafRenderObjectElement;
    use flui_rendering::RenderPadding;
    use flui_types::EdgeInsets;

    #[derive(Debug, Clone)]
    struct MockWidget;

    impl RenderWidget for MockWidget {
        fn create_render_object(&self, _context: &BuildContext) -> RenderNode {
            RenderNode::single(Box::new(RenderPadding::new(EdgeInsets::ZERO)))
        }

        fn update_render_object(&self, _context: &BuildContext, _render_object: &mut RenderNode) {}
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
        let align = Align::builder().child(Widget::from(MockWidget)).build();
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
        align.set_child(Widget::from(MockWidget));
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
    fn test_widget_trait() {
        let widget = Align::builder()
            .alignment(Alignment::TOP_LEFT)
            .child(Widget::from(MockWidget))
            .build();

        // Test child() method
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_single_child_render_object_widget_trait() {
        let widget = Align::builder()
            .alignment(Alignment::CENTER)
            .child(Widget::from(MockWidget))
            .build();

        // Test child() method - returns Option now
        assert!(widget.child.is_some());
    }
}

// Implement RenderWidget
impl RenderWidget for Align {
    fn create_render_object(&self, _context: &BuildContext) -> RenderNode {
        RenderNode::single(Box::new(RenderAlign::with_factors(
            self.alignment,
            self.width_factor,
            self.height_factor,
        )))
    }

    fn update_render_object(&self, _context: &BuildContext, render_object: &mut RenderNode) {
        if let RenderNode::Single { render, .. } = render_object {
            if let Some(align) = render.downcast_mut::<RenderAlign>() {
                align.set_alignment(self.alignment);
                align.set_width_factor(self.width_factor);
                align.set_height_factor(self.height_factor);
            }
        }
    }

    fn child(&self) -> Option<&Widget> {
        self.child.as_ref()
    }
}

// Implement IntoWidget for ergonomic API
flui_core::impl_into_widget!(Align, render);
