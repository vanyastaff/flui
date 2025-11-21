//! LayoutBuilder widget - builds widget tree based on parent constraints
//!
//! A widget that calls a builder function with the parent's constraints.
//! Similar to Flutter's LayoutBuilder widget.
//!
//! # Note
//!
//! This is currently a simplified implementation that builds the child with unconstrained
//! constraints at build time. A complete implementation would require special RenderObject
//! support to access actual runtime constraints during the layout phase.
//!
//! # Usage Patterns
//!
//! ```rust,ignore
//! LayoutBuilder::new(|ctx, constraints| {
//!     if constraints.max_width > 600.0 {
//!         Box::new(DesktopLayout::new())
//!     } else {
//!         Box::new(MobileLayout::new())
//!     }
//! })
//! ```

use flui_core::element::Element;
use flui_core::view::{IntoElement, View};
use flui_core::BuildContext;
use flui_types::BoxConstraints;
use std::sync::Arc;

/// Type for layout builder function that creates a widget from BuildContext and constraints
pub type LayoutWidgetBuilder = Arc<dyn Fn(&BuildContext, BoxConstraints) -> Element + Send + Sync>;

/// A widget that calls a builder function with BoxConstraints.
///
/// **Note**: This is a simplified implementation that provides unconstrained constraints
/// at build time. A complete implementation would require RenderObject integration to
/// access actual parent constraints during layout.
///
/// ## Use Cases
///
/// - **Responsive Layouts**: Different layouts for different screen sizes (limited)
/// - **Adaptive UI**: Adjust widget based on constraints (limited)
/// - **Constraint-based Logic**: Make decisions based on constraints (limited)
///
/// ## Examples
///
/// ```rust,ignore
/// // Basic usage with unconstrained mode
/// LayoutBuilder::new(|ctx, constraints| {
///     // constraints will be BoxConstraints::UNCONSTRAINED in this simplified version
///     Box::new(Text::new("Built with layout builder"))
/// })
/// ```
///
/// ## Limitations
///
/// - Currently receives `BoxConstraints::UNCONSTRAINED` (infinite constraints)
/// - Cannot access actual parent constraints at build time
/// - Full implementation requires RenderObject that can rebuild during layout
pub struct LayoutBuilder {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// Builder function that creates the child widget based on constraints
    pub builder: LayoutWidgetBuilder,
}

impl std::fmt::Debug for LayoutBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LayoutBuilder")
            .field("key", &self.key)
            .field("builder", &"<function>")
            .finish()
    }
}

impl Clone for LayoutBuilder {
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            builder: self.builder.clone(),
        }
    }
}

impl LayoutBuilder {
    /// Creates a new LayoutBuilder widget.
    ///
    /// **Note**: In this simplified implementation, the builder receives
    /// `BoxConstraints::UNCONSTRAINED`. Full constraint access requires RenderObject integration.
    ///
    /// # Parameters
    ///
    /// - `builder`: Function that takes BuildContext and BoxConstraints and returns a widget
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let widget = LayoutBuilder::new(|ctx, constraints| {
    ///     Box::new(Text::new("Hello"))
    /// });
    /// ```
    pub fn new<F>(builder: F) -> Self
    where
        F: Fn(&BuildContext, BoxConstraints) -> Element + Send + Sync + 'static,
    {
        Self {
            key: None,
            builder: Arc::new(builder),
        }
    }

    /// Sets the key for this widget.
    pub fn with_key(mut self, key: String) -> Self {
        self.key = Some(key);
        self
    }
}

// Implement View trait
impl View for LayoutBuilder {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        // In a simplified implementation, we call the builder with unconstrained constraints
        // A full implementation would need a special RenderObject that rebuilds during layout
        let constraints = BoxConstraints::UNCONSTRAINED;
        (self.builder)(ctx, constraints)
    }
}

/// Macro for creating LayoutBuilder with declarative syntax.
#[macro_export]
macro_rules! layout_builder {
    ($builder:expr) => {
        $crate::LayoutBuilder::new($builder)
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_core::view::IntoElement;

    #[test]
    fn test_layout_builder_new() {
        let widget = LayoutBuilder::new(|_ctx, _constraints| crate::SizedBox::new().into_element());
        assert!(widget.key.is_none());
    }

    #[test]
    fn test_layout_builder_with_key() {
        let widget = LayoutBuilder::new(|_ctx, _constraints| crate::SizedBox::new().into_element())
            .with_key("test".into());
        assert_eq!(widget.key, Some("test".into()));
    }

    #[test]
    fn test_layout_builder_macro() {
        let widget = layout_builder!(|_ctx, _constraints| crate::SizedBox::new().into_element());
        assert!(widget.key.is_none());
    }

    #[test]
    fn test_layout_builder_clone() {
        let widget1 =
            LayoutBuilder::new(|_ctx, _constraints| crate::SizedBox::new().into_element());
        let widget2 = widget1.clone();
        assert!(widget2.key.is_none());
    }
}
