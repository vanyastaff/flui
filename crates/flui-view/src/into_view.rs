//! `IntoView` trait - Convert types into `ViewObject`
//!
//! # Overview
//!
//! The `IntoView` trait provides a way to convert various view types
//! into boxed `ViewObject` instances for dynamic dispatch.
//!
//! # Design
//!
//! This trait is the view-layer abstraction for building view trees.
//! It enables the framework to work with heterogeneous view types.
//!
//! # Usage
//!
//! ```rust,ignore
//! use flui_view::{IntoView, StatelessView, ViewObject, BuildContext};
//!
//! struct MyView { value: i32 }
//!
//! impl StatelessView for MyView {
//!     fn build(self, ctx: &dyn BuildContext) -> impl IntoView {
//!         Text::new(format!("Value: {}", self.value))
//!     }
//! }
//!
//! // Convert to ViewObject
//! let view_obj: Box<dyn ViewObject> = MyView { value: 42 }.into_view_wrapped();
//! ```

use crate::traits::{StatefulView, StatelessView};
use crate::wrappers::{StatefulViewWrapper, StatelessViewWrapper};
use crate::ViewObject;

/// Converts a type into a boxed `ViewObject`.
///
/// This trait enables automatic conversion of view types into
/// trait objects for dynamic dispatch in the element tree.
///
/// # Implementations
///
/// This trait is implemented for:
/// - `Box<dyn ViewObject>` - Identity conversion
/// - View wrapper types (via blanket impls)
/// - Various helper types (Stateless, Stateful, etc.)
///
/// # Example
///
/// ```rust,ignore
/// use flui_view::{IntoView, StatelessView, BuildContext};
///
/// struct Greeting { name: String }
///
/// impl StatelessView for Greeting {
///     fn build(self, ctx: &dyn BuildContext) -> impl IntoView {
///         Text::new(format!("Hello, {}!", self.name))
///     }
/// }
///
/// // Convert to ViewObject using the wrapper helper
/// let view_obj = Stateless(Greeting { name: "World".into() }).into_view();
/// ```
pub trait IntoView: Send + 'static {
    /// Convert this value into a boxed `ViewObject`.
    fn into_view(self) -> Box<dyn ViewObject>;
}

// ============================================================================
// IMPLEMENTATION FOR BOX<DYN VIEWOBJECT>
// ============================================================================

impl IntoView for Box<dyn ViewObject> {
    /// Identity conversion - already a boxed `ViewObject`.
    #[inline]
    fn into_view(self) -> Box<dyn ViewObject> {
        self
    }
}

// ============================================================================
// IMPLEMENTATION FOR VIEW WRAPPERS
// ============================================================================

impl<V: StatelessView> IntoView for StatelessViewWrapper<V> {
    /// Convert `StatelessViewWrapper` into boxed `ViewObject`.
    #[inline]
    fn into_view(self) -> Box<dyn ViewObject> {
        Box::new(self)
    }
}

impl<V: StatefulView> IntoView for StatefulViewWrapper<V> {
    /// Convert `StatefulViewWrapper` into boxed `ViewObject`.
    #[inline]
    fn into_view(self) -> Box<dyn ViewObject> {
        Box::new(self)
    }
}

// ============================================================================
// CONVENIENCE IMPLEMENTATIONS FOR VIEW TRAITS
// ============================================================================

/// Extension trait for converting view trait implementations directly.
///
/// This is a convenience trait that wraps the view and then converts.
pub trait StatelessIntoView: StatelessView {
    /// Wrap in `StatelessViewWrapper` and convert to `ViewObject`.
    fn into_view_wrapped(self) -> Box<dyn ViewObject>
    where
        Self: Sized,
    {
        Box::new(StatelessViewWrapper::new(self))
    }
}

impl<V: StatelessView> StatelessIntoView for V {}

/// Extension trait for converting stateful view trait implementations directly.
pub trait StatefulIntoView: StatefulView {
    /// Wrap in `StatefulViewWrapper` and convert to `ViewObject`.
    fn into_view_wrapped(self) -> Box<dyn ViewObject>
    where
        Self: Sized,
    {
        Box::new(StatefulViewWrapper::new(self))
    }
}

impl<V: StatefulView> StatefulIntoView for V {}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ViewMode;
    use std::any::Any;

    // Helper for tests - represents an empty view
    struct EmptyIntoView;

    impl IntoView for EmptyIntoView {
        fn into_view(self) -> Box<dyn ViewObject> {
            Box::new(EmptyViewObject)
        }
    }

    struct EmptyViewObject;

    impl ViewObject for EmptyViewObject {
        fn mode(&self) -> ViewMode {
            ViewMode::Stateless
        }

        fn build(&mut self, _ctx: &dyn crate::BuildContext) -> Option<Box<dyn ViewObject>> {
            None
        }

        fn as_any(&self) -> &dyn Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    }

    // Test stateless view
    struct TestStatelessView {
        _value: i32,
    }

    impl StatelessView for TestStatelessView {
        fn build(self, _ctx: &dyn crate::BuildContext) -> impl IntoView {
            EmptyIntoView
        }
    }

    #[test]
    fn test_stateless_wrapper_into_view() {
        let wrapper = StatelessViewWrapper::new(TestStatelessView { _value: 42 });
        let view_obj = wrapper.into_view();
        assert_eq!(view_obj.mode(), ViewMode::Stateless);
    }

    #[test]
    fn test_boxed_view_object_into_view() {
        let wrapper = StatelessViewWrapper::new(TestStatelessView { _value: 42 });
        let boxed: Box<dyn ViewObject> = Box::new(wrapper);
        let view_obj = boxed.into_view();
        assert_eq!(view_obj.mode(), ViewMode::Stateless);
    }

    #[test]
    fn test_stateless_into_view_wrapped() {
        let view = TestStatelessView { _value: 42 };
        let view_obj = view.into_view_wrapped();
        assert_eq!(view_obj.mode(), ViewMode::Stateless);
    }
}
