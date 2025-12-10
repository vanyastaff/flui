//! Padding widget - adds empty space around a child
//!
//! A widget that insets its child by the given padding.
//! Similar to Flutter's Padding widget.
//!
//! # Usage Patterns
//!
//! ## 1. Convenience Methods (Recommended)
//! ```rust,ignore
//! // Uniform padding on all sides
//! Padding::all(16.0).child(text_widget)
//!
//! // Symmetric padding (horizontal, vertical)
//! Padding::symmetric(20.0, 10.0).child(content)
//!
//! // Only specific sides
//! Padding::only(10.0, 0.0, 0.0, 0.0).child(content)
//! ```
//!
//! ## 2. Builder Pattern
//! ```rust,ignore
//! Padding::builder()
//!     .padding(EdgeInsets::all(16.0))
//!     .child(some_widget)
//!     .build()
//! ```

use bon::Builder;
use flui_core::{view::IntoElement, Element};
use flui_objects::RenderPadding;
use flui_rendering::BoxProtocol;
use flui_rendering::{Optional, ProtocolId, RenderElement, RuntimeArity};
use flui_types::EdgeInsets;
use flui_view::{
    wrappers::RenderViewWrapper, Child, IntoView, IntoViewConfig, RenderView, UpdateResult,
    ViewObject,
};

/// A widget that insets its child by the given padding.
///
/// ## Layout Behavior
///
/// - The padding is applied inside any decoration constraints
/// - Negative padding is not supported and will be clamped to zero
/// - The child size is reduced by the padding amount
///
/// ## Examples
///
/// ```rust,ignore
/// // Uniform padding
/// Padding::all(16.0).child(Text::new("Hello"))
///
/// // Asymmetric padding
/// Padding::only(10.0, 5.0, 10.0, 5.0).child(content)
/// ```
#[derive(Debug, Builder)]
#[builder(on(EdgeInsets, into), finish_fn(name = build_internal, vis = ""))]
pub struct Padding {
    /// The amount of space by which to inset the child.
    #[builder(default = EdgeInsets::ZERO)]
    pub padding: EdgeInsets,

    /// The child widget.
    #[builder(default = Child::none())]
    pub child: Child,
}

impl Padding {
    /// Creates a new Padding with zero padding.
    pub fn new() -> Self {
        Self {
            padding: EdgeInsets::ZERO,
            child: Child::none(),
        }
    }

    /// Creates a Padding with the given EdgeInsets.
    pub fn from_insets(padding: EdgeInsets) -> Self {
        Self {
            padding,
            child: Child::none(),
        }
    }

    /// Creates a Padding with uniform padding on all sides.
    ///
    /// Most common use case - adds equal spacing on all four sides.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// Padding::all(16.0).child(Text::new("Hello"))
    /// ```
    pub fn all(value: f32) -> Self {
        Self {
            padding: EdgeInsets::all(value),
            child: Child::none(),
        }
    }

    /// Creates a Padding with symmetric horizontal and vertical padding.
    ///
    /// Perfect for responsive layouts - different spacing on x and y axes.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // 20px left/right, 10px top/bottom
    /// Padding::symmetric(20.0, 10.0).child(content)
    /// ```
    pub fn symmetric(horizontal: f32, vertical: f32) -> Self {
        Self {
            padding: EdgeInsets::symmetric(horizontal, vertical),
            child: Child::none(),
        }
    }

    /// Creates a Padding with custom padding on specific sides.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// Padding::only(10.0, 5.0, 10.0, 5.0).child(content)  // left, top, right, bottom
    /// ```
    pub fn only(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self {
            padding: EdgeInsets::new(left, top, right, bottom),
            child: Child::none(),
        }
    }

    /// Creates a Padding with only left padding.
    pub fn left(value: f32) -> Self {
        Self {
            padding: EdgeInsets::new(value, 0.0, 0.0, 0.0),
            child: Child::none(),
        }
    }

    /// Creates a Padding with only top padding.
    pub fn top(value: f32) -> Self {
        Self {
            padding: EdgeInsets::new(0.0, value, 0.0, 0.0),
            child: Child::none(),
        }
    }

    /// Creates a Padding with only right padding.
    pub fn right(value: f32) -> Self {
        Self {
            padding: EdgeInsets::new(0.0, 0.0, value, 0.0),
            child: Child::none(),
        }
    }

    /// Creates a Padding with only bottom padding.
    pub fn bottom(value: f32) -> Self {
        Self {
            padding: EdgeInsets::new(0.0, 0.0, 0.0, value),
            child: Child::none(),
        }
    }

    /// Creates horizontal padding (left and right).
    pub fn horizontal(value: f32) -> Self {
        Self {
            padding: EdgeInsets::symmetric(value, 0.0),
            child: Child::none(),
        }
    }

    /// Creates vertical padding (top and bottom).
    pub fn vertical(value: f32) -> Self {
        Self {
            padding: EdgeInsets::symmetric(0.0, value),
            child: Child::none(),
        }
    }

    /// Sets the child widget.
    pub fn child<V: IntoViewConfig>(mut self, view: V) -> Self {
        self.child = Child::new(view);
        self
    }
}

impl Default for Padding {
    fn default() -> Self {
        Self::new()
    }
}

// bon Builder Extensions
use padding_builder::State;

impl<S: State> PaddingBuilder<S> {
    /// Builds the Padding widget.
    pub fn build(self) -> Padding {
        self.build_internal()
    }
}

impl RenderView<BoxProtocol, Optional> for Padding {
    type RenderObject = RenderPadding;

    fn create(&self) -> RenderPadding {
        RenderPadding::new(self.padding)
    }

    fn update(&self, render: &mut RenderPadding) -> UpdateResult {
        if render.padding != self.padding {
            render.set_padding(self.padding);
            UpdateResult::NeedsLayout
        } else {
            UpdateResult::Unchanged
        }
    }
}

/// IntoView implementation for Padding.
///
/// Enables Padding to be used in widget composition. Note: This only converts
/// the Padding widget itself, not its child. The child is stored in the `child` field
/// and will be mounted separately during the element tree construction.
impl IntoView for Padding {
    fn into_view(self) -> Box<dyn ViewObject> {
        Box::new(RenderViewWrapper::new(self))
    }
}

/// IntoElement implementation for Padding.
///
/// Converts Padding to Element with its child. The child is converted
/// using `Child::into_element()` and attached as a pending child.
impl IntoElement for Padding {
    fn into_element(self) -> Element {
        // Extract child before moving self into wrapper
        let child = self.child;

        // Create the Padding render object
        let render_object = RenderPadding::new(self.padding);

        // Check if child is present
        if child.is_none() {
            // No child - just create render element
            Element::Render(RenderElement::with_pending(
                Box::new(render_object),
                ProtocolId::Box,
                RuntimeArity::Exact(0),
            ))
        } else {
            // Has child - convert and create with pending child
            let child_element = child.into_element();
            Element::Render(RenderElement::with_pending_and_children(
                Box::new(render_object),
                ProtocolId::Box,
                RuntimeArity::Exact(1),
                vec![Box::new(child_element)],
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_padding_new() {
        let padding = Padding::new();
        assert_eq!(padding.padding, EdgeInsets::ZERO);
    }

    #[test]
    fn test_padding_all() {
        let padding = Padding::all(16.0);
        assert_eq!(padding.padding, EdgeInsets::all(16.0));
    }

    #[test]
    fn test_padding_symmetric() {
        let padding = Padding::symmetric(20.0, 10.0);
        assert_eq!(padding.padding, EdgeInsets::symmetric(20.0, 10.0));
    }

    #[test]
    fn test_padding_only() {
        let padding = Padding::only(1.0, 2.0, 3.0, 4.0);
        assert_eq!(padding.padding, EdgeInsets::new(1.0, 2.0, 3.0, 4.0));
    }

    #[test]
    fn test_padding_left() {
        let padding = Padding::left(10.0);
        assert_eq!(padding.padding.left, 10.0);
        assert_eq!(padding.padding.top, 0.0);
        assert_eq!(padding.padding.right, 0.0);
        assert_eq!(padding.padding.bottom, 0.0);
    }

    #[test]
    fn test_padding_builder() {
        let padding = Padding::builder().padding(EdgeInsets::all(16.0)).build();
        assert_eq!(padding.padding, EdgeInsets::all(16.0));
    }

    #[test]
    fn test_padding_default() {
        let padding = Padding::default();
        assert_eq!(padding.padding, EdgeInsets::ZERO);
    }

    #[test]
    fn test_render_view_create() {
        let padding = Padding::all(10.0);
        let render = padding.create();
        assert_eq!(render.padding, EdgeInsets::all(10.0));
    }

    #[test]
    fn test_render_view_update_changed() {
        let padding = Padding::all(20.0);
        let mut render = RenderPadding::new(EdgeInsets::all(10.0));

        let result = padding.update(&mut render);
        assert_eq!(result, UpdateResult::NeedsLayout);
        assert_eq!(render.padding, EdgeInsets::all(20.0));
    }

    #[test]
    fn test_render_view_update_unchanged() {
        let padding = Padding::all(10.0);
        let mut render = RenderPadding::new(EdgeInsets::all(10.0));

        let result = padding.update(&mut render);
        assert_eq!(result, UpdateResult::Unchanged);
    }
}
