//! StatelessView - Views without internal state.
//!
//! StatelessViews are the simplest type of View. They describe UI purely
//! as a function of their configuration (fields) and inherited data.

use super::into_view::IntoView;
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
/// use flui_view::{StatelessView, BuildContext, IntoView};
///
/// #[derive(Clone)]
/// struct Greeting {
///     name: String,
/// }
///
/// impl StatelessView for Greeting {
///     fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
///         Text::new(format!("Hello, {}!", self.name))
///     }
/// }
/// ```
///
/// # Object safety
///
/// `StatelessView::build` returns `impl IntoView` (return-position
/// `impl Trait` in trait, stabilized in Rust 1.75). This makes
/// `StatelessView` **non-object-safe** — no `dyn StatelessView` use
/// exists or is needed. [`View`] is the object-safe boundary;
/// `StatelessView` is implementation-side (Phase 3 §U22, FR-007).
///
/// # Note
///
/// Types implementing `StatelessView` must also implement `Clone`.
/// Use the derive macro: `#[derive(Clone)]`
pub trait StatelessView: Clone + Send + Sync + 'static {
    /// Build the child View tree.
    ///
    /// Called whenever this View needs to be rendered. The returned
    /// value is normalized into a concrete [`View`] by the framework via
    /// [`IntoView::into_view`]; widget authors return the typed View
    /// directly (`Text::new(…)`) — no `Box::new` and no `.boxed()` at
    /// the call site. For conditional builds whose arms have different
    /// types, the author wraps each arm with `.boxed()` to land on
    /// `BoxedView` (which itself implements [`IntoView`]).
    ///
    /// The framework normalizes the opaque return via
    /// [`IntoView::into_view`] *inside* the build call site (see
    /// `element/behavior.rs`), boxing the concrete `'static` value
    /// into `Box<dyn View>` before crossing closure / catch-unwind
    /// boundaries. The default Rust 2024 RPITIT capture (Self +
    /// elided lifetimes of `&self` / `&dyn BuildContext`) is fine
    /// because the opaque value is consumed inside the closure body
    /// — its captured borrows never escape.
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView;
}

/// Implement View for all StatelessViews.
///
/// This macro creates the View implementation for a StatelessView type.
/// Use it after implementing StatelessView:
///
/// ```rust,ignore
/// impl StatelessView for MyView {
///     fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
///         // ...
///     }
/// }
/// impl_stateless_view!(MyView);
/// ```
///
/// # Deprecation
///
/// Phase 3 §U24 deletes this macro in favor of `#[derive(StatelessView)]`
/// from `flui-macros`. The macro stays during the §U22→§U24 transition
/// so existing call sites continue to compile; remove invocations and
/// switch to the derive once §U24 lands (FR-009 / FR-010).
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

// NOTE: StatelessElement implementation has been moved to unified Element
// architecture. See crates/flui-view/src/element/unified.rs and
// element/behavior.rs The type alias is exported from element/mod.rs:
//   pub type StatelessElement<V> = Element<V, Single, StatelessBehavior>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        StatelessElement,
        element::{Lifecycle, StatelessBehavior},
        view::{ElementBase, View},
    };

    #[derive(Clone)]
    struct TestView {
        #[expect(dead_code, reason = "exercised only by the derived Clone impl")]
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
        let mut owner = crate::BuildOwner::new();
        element.mount(None, 0, &mut owner.element_owner_mut());
        assert_eq!(element.lifecycle(), Lifecycle::Active);
    }
}
