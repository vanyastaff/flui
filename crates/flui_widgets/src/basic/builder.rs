//! Builder widget - builds widget tree using BuildContext
//!
//! A widget that calls a builder function to create its child.
//! Similar to Flutter's Builder widget.
//!
//! # Usage Patterns
//!
//! ```rust,ignore
//! Builder::new(|ctx| {
//!     // Access BuildContext here
//!     Text::new("Hello from builder!")
//! })
//! ```

use flui_core::element::Element;
use flui_core::view::{IntoElement, View};
use flui_core::BuildContext;
use std::sync::Arc;

/// Type for builder function that creates a widget from BuildContext
pub type WidgetBuilder = Arc<dyn Fn(&BuildContext) -> Element + Send + Sync>;

/// A widget that calls a builder function to create its child.
///
/// Builder is useful when you need access to the BuildContext to build
/// a widget, but the context isn't available at the point where you're
/// creating the widget tree.
///
/// ## Use Cases
///
/// - **Access BuildContext**: Get context where it's not directly available
/// - **Deferred Building**: Build child based on context information
/// - **Theme Access**: Access theme data from context
/// - **MediaQuery**: Get screen size and other media information
///
/// ## Examples
///
/// ```rust,ignore
/// // Access context to build child
/// Builder::new(|ctx| {
///     Box::new(Text::new(format!("Context available!")))
/// })
///
/// // Use with hooks
/// Builder::new(|ctx| {
///     let signal = use_signal(ctx, 0);
///     Box::new(Text::new(format!("Count: {}", signal.get())))
/// })
/// ```
pub struct Builder {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// Builder function that creates the child widget
    pub builder: WidgetBuilder,
}

impl std::fmt::Debug for Builder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Builder")
            .field("key", &self.key)
            .field("builder", &"<function>")
            .finish()
    }
}

impl Clone for Builder {
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            builder: self.builder.clone(),
        }
    }
}

impl Builder {
    /// Creates a new Builder widget.
    ///
    /// # Parameters
    ///
    /// - `builder`: Function that takes BuildContext and returns a widget
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let widget = Builder::new(|ctx| {
    ///     Box::new(Text::new("Built with context!"))
    /// });
    /// ```
    pub fn new<F>(builder: F) -> Self
    where
        F: Fn(&BuildContext) -> Element + Send + Sync + 'static,
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
impl View for Builder {
    fn build(self, ctx: &BuildContext) -> impl IntoElement {
        // Call the builder function with the context
        (self.builder)(ctx)
    }
}

/// Macro for creating Builder with declarative syntax.
#[macro_export]
macro_rules! builder {
    ($builder:expr) => {
        $crate::Builder::new($builder)
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_new() {
        let widget = Builder::new(|_ctx| crate::SizedBox::new().into_element());
        assert!(widget.key.is_none());
    }

    #[test]
    fn test_builder_with_key() {
        let widget =
            Builder::new(|_ctx| crate::SizedBox::new().into_element()).with_key("test".into());
        assert_eq!(widget.key, Some("test".into()));
    }

    #[test]
    fn test_builder_macro() {
        let widget = builder!(|_ctx| crate::SizedBox::new().into_element());
        assert!(widget.key.is_none());
    }

    #[test]
    fn test_builder_clone() {
        let widget1 = Builder::new(|_ctx| crate::SizedBox::new().into_element());
        let widget2 = widget1.clone();
        assert!(widget2.key.is_none());
    }
}
