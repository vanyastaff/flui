//! StatelessView - Views without internal state.
//!
//! StatelessViews are the simplest type of View. They describe UI purely
//! as a function of their configuration (fields) and inherited data.

use super::view::View;
use crate::context::BuildContext;

/// A View that has no mutable state.
///
/// StatelessViews rebuild their child tree based solely on:
/// - Their own configuration (struct fields)
/// - Data from ancestor InheritedViews
///
/// They are rebuilt when:
/// - Their configuration changes (parent rebuilds with new View)
/// - An InheritedView they depend on changes
///
/// # Flutter Equivalent
///
/// This corresponds to Flutter's `StatelessWidget`.
///
/// # Example
///
/// ```rust,ignore
/// use flui_view::{StatelessView, BuildContext};
///
/// #[derive(Clone)]
/// struct Greeting {
///     name: String,
/// }
///
/// impl StatelessView for Greeting {
///     fn build(&self, ctx: &dyn BuildContext) -> Box<dyn View> {
///         Text::new(format!("Hello, {}!", self.name)).boxed()
///     }
/// }
/// ```
///
/// # Note
///
/// Types implementing `StatelessView` must also implement `Clone`.
/// Use the derive macro: `#[derive(Clone)]`
pub trait StatelessView: Clone + Send + Sync + 'static {
    /// Build the child View tree.
    ///
    /// Called whenever this View needs to be rendered. The returned View
    /// describes what should be displayed.
    fn build(&self, ctx: &dyn BuildContext) -> Box<dyn View>;
}

/// Implement View for all StatelessViews.
///
/// This macro creates the View implementation for a StatelessView type.
/// Use it after implementing StatelessView:
///
/// ```rust,ignore
/// impl StatelessView for MyView {
///     fn build(&self, ctx: &dyn BuildContext) -> Box<dyn View> {
///         // ...
///     }
/// }
/// impl_stateless_view!(MyView);
/// ```
#[macro_export]
macro_rules! impl_stateless_view {
    ($ty:ty) => {
        impl $crate::View for $ty {
            fn create_element(&self) -> Box<dyn $crate::ElementBase> {
                use $crate::element::StatelessBehavior;
                Box::new($crate::StatelessElement::new(self, StatelessBehavior))
            }
        }
    };
}

// NOTE: StatelessElement implementation has been moved to unified Element architecture.
// See crates/flui-view/src/element/unified.rs and element/behavior.rs
// The type alias is exported from element/mod.rs:
//   pub type StatelessElement<V> = Element<V, Single, StatelessBehavior>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::element::{Lifecycle, StatelessBehavior};
    use crate::view::{ElementBase, View};
    use crate::StatelessElement;

    #[derive(Clone)]
    struct TestView {
        text: String,
    }

    impl StatelessView for TestView {
        fn build(&self, _ctx: &dyn BuildContext) -> Box<dyn View> {
            // Return self for testing - in real code this would return child views
            Box::new(self.clone())
        }
    }

    // Implement View for TestView using the macro pattern
    impl View for TestView {
        fn create_element(&self) -> Box<dyn ElementBase> {
            Box::new(StatelessElement::new(self, StatelessBehavior))
        }
    }

    #[test]
    fn test_stateless_element_creation() {
        let view = TestView {
            text: "Hello".to_string(),
        };
        let element = StatelessElement::new(&view, StatelessBehavior);
        assert_eq!(element.lifecycle(), Lifecycle::Initial);
        // Element is created in Initial state
    }

    #[test]
    fn test_stateless_element_mount() {
        let view = TestView {
            text: "Hello".to_string(),
        };
        let mut element = StatelessElement::new(&view, StatelessBehavior);
        element.mount(None, 0);
        assert_eq!(element.lifecycle(), Lifecycle::Active);
    }
}
