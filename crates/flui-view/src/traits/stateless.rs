//! Stateless view trait.
//!
//! For simple views without persistent state or lifecycle.

use crate::into_element::IntoElement;

// ============================================================================
// STATELESS VIEW TRAIT
// ============================================================================

/// Stateless view - simple views without persistent state.
///
/// Similar to Flutter's `StatelessWidget`. Views are consumed during build
/// and cannot be rebuilt. Perfect for pure composition.
///
/// # Lifecycle
///
/// - **Created**: View struct instantiated
/// - **Build**: `build()` called once, view consumed
/// - **Done**: No rebuild, no lifecycle hooks
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Debug)]
/// struct Greeting {
///     name: String,
/// }
///
/// impl StatelessView for Greeting {
///     type Output = Text;
///
///     fn build(self) -> Self::Output {
///         Text::new(format!("Hello, {}", self.name))
///     }
/// }
/// ```
///
/// # When to Use
///
/// - Pure UI composition
/// - No user interaction
/// - Props don't change
/// - Simple leaf widgets
///
/// # When NOT to Use
///
/// - Need to store state → Use `StatefulView`
/// - Need subscriptions → Use hooks or `AnimatedView`
/// - Need lifecycle → Use `StatefulView` or `ProxyView`
pub trait StatelessView: Send + 'static {
    /// Output type from build.
    type Output: IntoElement;

    /// Build UI from this view.
    ///
    /// View is consumed (moved) during build. Cannot be called again.
    ///
    /// # Return
    ///
    /// Any type implementing `IntoElement` (View, RenderObject, Element, etc)
    fn build(self) -> Self::Output;
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    struct TestView {
        text: String,
    }

    impl StatelessView for TestView {
        type Output = ();

        fn build(self) -> Self::Output {
            let _ = self.text;
        }
    }

    #[test]
    fn test_stateless_view_build() {
        let view = TestView {
            text: "Hello".to_string(),
        };
        view.build();
    }
}
