//! Proxy view trait.
//!
//! For views that wrap a single child without layout changes.

use crate::into_element::IntoElement;

// ============================================================================
// PROXY VIEW TRAIT
// ============================================================================

/// Proxy view - views that wrap single child without layout changes.
///
/// Similar to Flutter's `ProxyWidget`. Used for views that need to
/// intercept the build process without creating their own render object.
///
/// # Use Cases
///
/// - **InheritedWidget**: Providing data to descendants
/// - **Theme**: Wrapping subtree with theme data
/// - **MediaQuery**: Providing screen metrics
/// - **Focus/Gesture handling**: Intercepting user input
///
/// # Example
///
/// ```rust,ignore
/// struct ThemeProvider {
///     theme: Theme,
///     child: Box<dyn IntoElement>,
/// }
///
/// impl ProxyView for ThemeProvider {
///     type Child = Box<dyn IntoElement>;
///
///     fn child(self) -> Self::Child {
///         self.child
///     }
///
///     fn wrap_child(&self, child_element: ElementId) -> ElementId {
///         // Store theme data in element tree
///         child_element
///     }
/// }
/// ```
///
/// # When to Use
///
/// - Need to provide data to descendants
/// - Need to intercept without layout changes
/// - Building inherited/provider widgets
///
/// # When NOT to Use
///
/// - Need layout changes → Use `RenderView`
/// - No child → Use `StatelessView`
/// - Multiple children → Use custom view with `Children`
pub trait ProxyView: Send + 'static {
    /// Child type.
    type Child: IntoElement;

    /// Returns the child view.
    ///
    /// The child is extracted and built, then passed through
    /// any proxy-specific wrapping.
    fn child(self) -> Self::Child;
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    struct TestProxy {
        child: (),
    }

    impl ProxyView for TestProxy {
        type Child = ();

        fn child(self) -> Self::Child {
            self.child
        }
    }

    #[test]
    fn test_proxy_view_child() {
        let proxy = TestProxy { child: () };
        let _child = proxy.child();
    }
}
