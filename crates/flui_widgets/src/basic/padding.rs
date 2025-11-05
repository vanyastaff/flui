//! Padding widget - adds empty space around a child
//!
//! A widget that insets its child by the given padding.
//! Similar to Flutter's Padding widget.
//!
//! # Usage Patterns
//!
//! ## 1. Struct Literal
//! ```rust,ignore
//! Padding {
//!     padding: EdgeInsets::all(16.0),
//!     ..Default::default()
//! }
//! ```
//!
//! ## 2. Builder Pattern
//! ```rust,ignore
//! Padding::builder()
//!     .padding(EdgeInsets::all(16.0))
//!     .child(some_widget)
//!     .build()
//! ```
//!
//! ## 3. Macro
//! ```rust,ignore
//! padding! {
//!     padding: EdgeInsets::all(16.0),
//! }
//! ```

use bon::Builder;
use flui_core::{BuildContext, Element, RenderElement};
use flui_core::render::RenderNode;
use flui_core::view::{View, ChangeFlags, AnyView};
use flui_rendering::RenderPadding;
use flui_types::EdgeInsets;

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
/// Padding::builder()
///     .padding(EdgeInsets::all(20.0))
///     .child(Text::new("Hello"))
///     .build()
///
/// // Asymmetric padding
/// Padding::builder()
///     .padding(EdgeInsets::only(left: 10.0, right: 10.0, top: 5.0, bottom: 5.0))
///     .child(some_widget)
///     .build()
/// ```
#[derive(Builder)]
#[builder(
    on(String, into),
    on(EdgeInsets, into),
    finish_fn = build_padding
)]
pub struct Padding {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// The amount of space by which to inset the child.
    #[builder(default = EdgeInsets::ZERO)]
    pub padding: EdgeInsets,

    /// The child widget to pad.
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<Box<dyn AnyView>>,
}

impl std::fmt::Debug for Padding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Padding")
            .field("key", &self.key)
            .field("padding", &self.padding)
            .field("child", &if self.child.is_some() { "<AnyView>" } else { "None" })
            .finish()
    }
}

impl Clone for Padding {
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            padding: self.padding,
            child: self.child.clone(),
        }
    }
}

impl Padding {
    /// Creates a new Padding with zero padding.
    pub fn new() -> Self {
        Self {
            key: None,
            padding: EdgeInsets::ZERO,
            child: None,
        }
    }

    /// Creates a Padding with uniform padding on all sides.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let padding = Padding::all(16.0);
    /// ```
    pub fn all(value: f32) -> Self {
        Self {
            key: None,
            padding: EdgeInsets::all(value),
            child: None,
        }
    }

    /// Creates a Padding with symmetric horizontal and vertical padding.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let padding = Padding::symmetric(horizontal: 20.0, vertical: 10.0);
    /// ```
    pub fn symmetric(horizontal: f32, vertical: f32) -> Self {
        Self {
            key: None,
            padding: EdgeInsets::symmetric(horizontal, vertical),
            child: None,
        }
    }

    /// Creates a Padding with the given padding and child.
    pub fn with_child(padding: EdgeInsets, child: impl View + 'static) -> Self {
        Self {
            key: None,
            padding,
            child: Some(Box::new(child)),
        }
    }

    /// Sets the child widget.
    pub fn set_child(&mut self, child: impl View + 'static) {
        self.child = Some(Box::new(child));
    }

    /// Validates padding configuration.
    pub fn validate(&self) -> Result<(), String> {
        // Padding values should be non-negative
        if self.padding.left < 0.0
            || self.padding.right < 0.0
            || self.padding.top < 0.0
            || self.padding.bottom < 0.0
        {
            return Err("Padding values must be non-negative".to_string());
        }

        Ok(())
    }
}

impl Default for Padding {
    fn default() -> Self {
        Self::new()
    }
}

// Implement View for Padding - New architecture
impl View for Padding {
    type Element = Element;
    type State = Option<Box<dyn std::any::Any>>;

    fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        // Build child if present
        let (child_id, child_state) = if let Some(child) = self.child {
            let (elem, state) = child.build_any(ctx);
            let id = ctx.tree().write().insert(elem.into_element());
            (Some(id), Some(state))
        } else {
            (None, None)
        };

        // Create RenderNode (always Single for SingleRender widgets)
        let render_node = RenderNode::Single {
            render: Box::new(RenderPadding::new(self.padding)),
            child: child_id,
        };

        // Create RenderElement using constructor
        let render_element = RenderElement::new(render_node);

        (Element::Render(render_element), child_state)
    }

    fn rebuild(
        self,
        prev: &Self,
        state: &mut Self::State,
        element: &mut Self::Element,
    ) -> ChangeFlags {
        // TODO: Implement proper rebuild logic if needed
        // For now, return NONE as View architecture handles rebuilding
        ChangeFlags::NONE
    }
}

// bon Builder Extensions
use padding_builder::{IsUnset, SetChild, State};

// Custom child setter
impl<S: State> PaddingBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    pub fn child(self, child: impl View + 'static) -> PaddingBuilder<SetChild<S>> {
        self.child_internal(Box::new(child))
    }
}

// Build wrapper
impl<S: State> PaddingBuilder<S> {
    /// Builds the Padding widget.
    pub fn build(self) -> Padding {
        self.build_padding()
    }
}

/// Macro for creating Padding with declarative syntax.
#[macro_export]
macro_rules! padding {
    () => {
        $crate::Padding::new()
    };
    ($($field:ident : $value:expr),* $(,)?) => {
        $crate::Padding {
            $($field: $value.into(),)*
            ..Default::default()
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_core::element::ElementBase;
    use flui_types::{Size, Offset};

    // Mock view for testing
    #[derive(Debug, Clone)]
    struct MockView;

    impl View for MockView {
        type Element = Element;
        type State = ();

        fn build(self, _ctx: &mut BuildContext) -> (Self::Element, Self::State) {
            let render_node = RenderNode::Leaf(Box::new(RenderPadding::new(EdgeInsets::ZERO)));
            let render_element = RenderElement::new(render_node);
            (Element::Render(render_element), ())
        }

        fn rebuild(self, _prev: &Self, _state: &mut Self::State, _element: &mut Self::Element) -> ChangeFlags {
            ChangeFlags::NONE
        }
    }

    #[test]
    fn test_padding_new() {
        let padding = Padding::new();
        assert!(padding.key.is_none());
        assert_eq!(padding.padding, EdgeInsets::ZERO);
        assert!(padding.child.is_none());
    }

    #[test]
    fn test_padding_default() {
        let padding = Padding::default();
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
        assert_eq!(padding.padding.left, 20.0);
        assert_eq!(padding.padding.right, 20.0);
        assert_eq!(padding.padding.top, 10.0);
        assert_eq!(padding.padding.bottom, 10.0);
    }

    #[test]
    fn test_padding_builder() {
        let padding = Padding::builder().padding(EdgeInsets::all(10.0)).build();
        assert_eq!(padding.padding, EdgeInsets::all(10.0));
    }

    #[test]
    fn test_padding_builder_with_child() {
        let padding = Padding::builder()
            .padding(EdgeInsets::all(10.0))
            .child(MockView)
            .build();
        assert!(padding.child.is_some());
    }

    #[test]
    fn test_padding_set_child() {
        let mut padding = Padding::new();
        padding.set_child(MockView);
        assert!(padding.child.is_some());
    }

    #[test]
    fn test_padding_macro_empty() {
        let padding = padding!();
        assert_eq!(padding.padding, EdgeInsets::ZERO);
    }

    #[test]
    fn test_padding_macro_with_padding() {
        let padding = padding! {
            padding: EdgeInsets::all(20.0),
        };
        assert_eq!(padding.padding, EdgeInsets::all(20.0));
    }

    #[test]
    fn test_padding_validate_ok() {
        let padding = Padding::all(10.0);
        assert!(padding.validate().is_ok());
    }

    #[test]
    fn test_padding_validate_negative() {
        let padding = Padding {
            padding: EdgeInsets::new(10.0, -5.0, 0.0, 0.0),
            ..Default::default()
        };
        assert!(padding.validate().is_err());
    }

    #[test]
    fn test_padding_view_trait() {
        let padding = Padding::builder()
            .padding(EdgeInsets::all(10.0))
            .child(MockView)
            .build();

        // Test child field
        assert!(padding.child.is_some());
    }
}

// Padding now implements View trait directly
