//! WidgetName - brief description
//!
//! Detailed explanation of what the widget does and how it works.
//!
//! # Usage Patterns
//!
//! Widget supports three creation styles:
//!
//! ## 1. Struct Literal (Flutter-like)
//! ```rust,ignore
//! WidgetName {
//!     property: Some(value),
//!     ..Default::default()
//! }
//! ```
//!
//! ## 2. Builder Pattern (Type-safe with bon)
//! ```rust,ignore
//! WidgetName::builder()
//!     .property(value)
//!     .child(some_widget)
//!     .build()
//! ```
//!
//! ## 3. Macro (Declarative)
//! ```rust,ignore
//! widget_name! {
//!     property: value,
//! }
//! ```

use bon::Builder;
use flui_core::{RenderObject, RenderObjectWidget, Widget};
use flui_rendering::RenderYourObject; // Replace with actual RenderObject
use flui_types::*; // Import needed types

/// A widget that does X.
///
/// Detailed description of what this widget does, its layout behavior,
/// and how it interacts with its child widget.
///
/// This widget creates a [RenderYourObject] to handle layout and painting.
#[derive(Debug, Clone, Builder)]
#[builder(
    // Type conversions - enable .into() for common types
    on(String, into),
    on(EdgeInsets, into),
    on(Color, into),

    // Custom finish function (private internal build)
    finish_fn = build_widget_name
)]
pub struct WidgetName {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// Main property description
    ///
    /// Explain what this property does and its default value.
    pub property1: Option<f32>,

    /// Property with default value
    ///
    /// Describe the effect of this property on layout/rendering.
    #[builder(default = DefaultValue)]
    pub property2: SomeType,

    /// Child widget
    ///
    /// The child widget that will be affected by this widget's properties.
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<Box<dyn Widget>>,
}

impl WidgetName {
    /// Creates a new widget with default values.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let widget = WidgetName::new();
    /// ```
    pub fn new() -> Self {
        Self {
            key: None,
            property1: None,
            property2: DefaultValue,
            child: None,
        }
    }

    /// Sets the child widget (for struct literal usage).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut widget = WidgetName::new();
    /// widget.set_child(some_widget);
    /// ```
    pub fn set_child(&mut self, child: impl Widget + 'static) {
        self.child = Some(Box::new(child));
    }

    /// Validates widget configuration.
    ///
    /// Returns Ok(()) if valid, or an error message describing the issue.
    pub fn validate(&self) -> Result<(), String> {
        // Add validation logic here
        if let Some(prop) = self.property1 {
            if prop < 0.0 || prop.is_nan() || prop.is_infinite() {
                return Err(format!(
                    "Invalid property1: {}. Must be finite and non-negative.",
                    prop
                ));
            }
        }
        Ok(())
    }
}

impl Default for WidgetName {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for WidgetName {
    fn create_element(&self) -> Box<dyn flui_core::Element> {
        // For RenderObjectWidget, create RenderObjectElement
        // Note: Same Element type is used for both single-child and multi-child widgets
        // The difference is determined by implementing MultiChildRenderObjectWidget trait
        Box::new(flui_core::RenderObjectElement::new(self.clone()))
    }
}

impl RenderObjectWidget for WidgetName {
    fn create_render_object(&self) -> Box<dyn RenderObject> {
        // Create the appropriate RenderObject for this widget
        // Pass widget configuration to RenderObject constructor
        Box::new(RenderYourObject::new(
            self.property1.unwrap_or(default_value),
            self.property2,
        ))
    }

    fn update_render_object(&self, render_object: &mut dyn RenderObject) {
        // Update RenderObject when widget configuration changes
        // Use downcast_mut to get concrete RenderObject type
        if let Some(render) = render_object.downcast_mut::<RenderYourObject>() {
            render.set_property1(self.property1.unwrap_or(default_value));
            render.set_property2(self.property2);
        }
    }
}

// For multi-child widgets, also implement MultiChildRenderObjectWidget:
// use flui_core::MultiChildRenderObjectWidget;
//
// impl MultiChildRenderObjectWidget for WidgetName {
//     fn children(&self) -> &[Box<dyn Widget>] {
//         &self.children
//     }
// }

// bon Builder Extensions
use widget_name_builder::{IsUnset, SetChild, State};

// Custom setter for child (if widget has children)
impl<S: State> WidgetNameBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// WidgetName::builder()
    ///     .property(value)
    ///     .child(some_widget)
    ///     .build()
    /// ```
    pub fn child(self, child: impl Widget + 'static) -> WidgetNameBuilder<SetChild<S>> {
        // bon wraps Box in Option internally
        self.child_internal(Box::new(child) as Box<dyn Widget>)
    }
}

// Public build() wrapper for convenience
impl<S: State> WidgetNameBuilder<S> {
    /// Convenience method to build the widget.
    ///
    /// Equivalent to calling the generated `build_widget_name()` finishing function.
    pub fn build(self) -> WidgetName {
        self.build_widget_name()
    }
}

/// Macro for creating WidgetName with declarative syntax.
///
/// # Examples
///
/// ```rust,ignore
/// widget_name! {
///     property1: value1,
///     property2: value2,
/// }
/// ```
#[macro_export]
macro_rules! widget_name {
    // Empty widget
    () => {
        $crate::WidgetName::new()
    };

    // Widget with fields
    ($($field:ident : $value:expr),* $(,)?) => {
        $crate::WidgetName {
            $($field: Some($value.into()),)*
            ..Default::default()
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_widget_new() {
        let widget = WidgetName::new();
        assert!(widget.key.is_none());
        assert!(widget.property1.is_none());
        assert_eq!(widget.property2, DefaultValue);
    }

    #[test]
    fn test_widget_default() {
        let widget = WidgetName::default();
        assert!(widget.property1.is_none());
        assert_eq!(widget.property2, DefaultValue);
    }

    #[test]
    fn test_widget_struct_literal() {
        let widget = WidgetName {
            property1: Some(100.0),
            ..Default::default()
        };
        assert_eq!(widget.property1, Some(100.0));
        assert_eq!(widget.property2, DefaultValue);
    }

    #[test]
    fn test_widget_builder() {
        let widget = WidgetName::builder().property1(100.0).build();
        assert_eq!(widget.property1, Some(100.0));
    }

    #[test]
    fn test_widget_builder_chaining() {
        let widget = WidgetName::builder()
            .property1(100.0)
            .property2(some_value)
            .build();

        assert_eq!(widget.property1, Some(100.0));
        assert_eq!(widget.property2, some_value);
    }

    #[test]
    fn test_widget_macro_empty() {
        let widget = widget_name!();
        assert!(widget.property1.is_none());
    }

    #[test]
    fn test_widget_macro_with_fields() {
        let widget = widget_name! {
            property1: 100.0,
        };
        assert_eq!(widget.property1, Some(100.0));
    }

    #[test]
    fn test_widget_validate_ok() {
        let widget = WidgetName::builder().property1(100.0).build();
        assert!(widget.validate().is_ok());
    }

    #[test]
    fn test_widget_validate_invalid() {
        let widget = WidgetName {
            property1: Some(-1.0),
            ..Default::default()
        };
        assert!(widget.validate().is_err());
    }

    #[test]
    fn test_widget_validate_nan() {
        let widget = WidgetName {
            property1: Some(f32::NAN),
            ..Default::default()
        };
        assert!(widget.validate().is_err());
    }

    #[test]
    fn test_widget_set_child() {
        let mut widget = WidgetName::new();
        assert!(widget.child.is_none());

        // Note: Requires a concrete Widget implementation to test fully
        // widget.set_child(some_concrete_widget);
        // assert!(widget.child.is_some());
    }

    #[test]
    fn test_render_object_creation() {
        let widget = WidgetName::builder()
            .property1(100.0)
            .build();

        let render_object = widget.create_render_object();
        assert!(render_object.downcast_ref::<RenderYourObject>().is_some());
    }

    #[test]
    fn test_render_object_update() {
        let widget1 = WidgetName::builder()
            .property1(100.0)
            .build();

        let mut render_object = widget1.create_render_object();

        let widget2 = WidgetName::builder()
            .property1(200.0)
            .build();

        widget2.update_render_object(&mut *render_object);

        // Verify the update was applied
        if let Some(render) = render_object.downcast_ref::<RenderYourObject>() {
            // Add assertions based on your RenderObject's state
        }
    }
}
