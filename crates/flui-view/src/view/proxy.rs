//! ProxyView - Single-child wrapper Views.
//!
//! ProxyViews are Views that have exactly one child and typically add
//! some behavior or configuration without creating a RenderObject.

use super::view::View;

/// A View that wraps a single child without creating a RenderObject.
///
/// ProxyViews are used for:
/// - Adding behavior (gesture detection, focus handling)
/// - Providing configuration (themes, localization)
/// - Composition without visual representation
///
/// # Flutter Equivalent
///
/// This corresponds to Flutter's `ProxyWidget` and its subclasses like:
/// - `InheritedWidget` (though we have InheritedView separately)
/// - `ParentDataWidget`
///
/// # Example
///
/// ```rust,ignore
/// use flui_view::{ProxyView, BuildContext, View};
/// use std::rc::Rc;
///
/// struct GestureDetector {
///     on_tap: Option<Rc<dyn Fn()>>,
///     child: Box<dyn View>,
/// }
///
/// impl ProxyView for GestureDetector {
///     fn child(&self) -> &dyn View {
///         &*self.child
///     }
/// }
/// ```
pub trait ProxyView: Clone + 'static + Sized {
    /// Get the child View.
    fn child(&self) -> &dyn View;
}

/// Implement View for a ProxyView type.
///
/// This macro creates the View implementation for a ProxyView type.
///
/// ```rust,ignore
/// impl ProxyView for MyGestureDetector {
///     fn child(&self) -> &dyn View { &*self.child }
/// }
/// impl_proxy_view!(MyGestureDetector);
/// ```
#[macro_export]
macro_rules! impl_proxy_view {
    ($ty:ty) => {
        impl $crate::View for $ty {
            fn create_element(&self) -> $crate::element::ElementKind {
                $crate::element::ElementKind::proxy(self)
            }
        }
    };
}

// NOTE: ProxyElement implementation has been moved to unified Element
// architecture. See crates/flui-view/src/element/unified.rs and
// element/behavior.rs The type alias is exported from element/mod.rs:
//   pub type ProxyElement<V> = Element<V, Single, ProxyBehavior>;
