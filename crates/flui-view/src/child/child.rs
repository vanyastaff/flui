//! Single child wrapper.
//!
//! Provides an ergonomic wrapper for Views with a single optional child.

use crate::view::{BoxedView, View};

/// A wrapper for a single optional child View.
///
/// This provides a consistent API for Views that accept one child,
/// handling both the case where a child is present and where it's absent.
///
/// # Example
///
/// ```rust,ignore
/// use flui_view::{Child, View};
///
/// struct Container {
///     child: Child,
/// }
///
/// impl Container {
///     pub fn new() -> Self {
///         Self { child: Child::empty() }
///     }
///
///     pub fn child(mut self, child: impl View) -> Self {
///         self.child = Child::some(child);
///         self
///     }
/// }
/// ```
#[derive(Default, Clone)]
pub struct Child {
    inner: Option<BoxedView>,
}

impl Child {
    /// Create an empty Child (no child view).
    pub fn empty() -> Self {
        Self { inner: None }
    }

    /// Create a Child with the given View.
    pub fn some(view: impl View) -> Self {
        Self {
            inner: Some(BoxedView(Box::new(view))),
        }
    }

    /// Create a Child from an optional View.
    pub fn from_option(view: Option<impl View>) -> Self {
        Self {
            inner: view.map(|v| BoxedView(Box::new(v))),
        }
    }

    /// Check if this Child has a view.
    pub fn is_some(&self) -> bool {
        self.inner.is_some()
    }

    /// Check if this Child is empty.
    pub fn is_none(&self) -> bool {
        self.inner.is_none()
    }

    /// Get a reference to the child View if present.
    pub fn as_ref(&self) -> Option<&dyn View> {
        self.inner.as_ref().map(|b| &*b.0 as &dyn View)
    }

    /// Get the inner BoxedView if present.
    pub fn into_inner(self) -> Option<BoxedView> {
        self.inner
    }

    /// Take the child, leaving None in its place.
    pub fn take(&mut self) -> Option<BoxedView> {
        self.inner.take()
    }

    /// Replace the child with a new one, returning the old.
    pub fn replace(&mut self, view: impl View) -> Option<BoxedView> {
        self.inner.replace(BoxedView(Box::new(view)))
    }
}

impl std::fmt::Debug for Child {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Child")
            .field("has_child", &self.inner.is_some())
            .finish()
    }
}

impl<V: View> From<V> for Child {
    fn from(view: V) -> Self {
        Child::some(view)
    }
}

impl<V: View> From<Option<V>> for Child {
    fn from(view: Option<V>) -> Self {
        Child::from_option(view)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ElementBase, StatelessElement, StatelessView};

    #[derive(Clone)]
    struct TestView;

    impl StatelessView for TestView {
        fn build(&self, _ctx: &dyn crate::BuildContext) -> Box<dyn View> {
            Box::new(TestView)
        }
    }

    impl View for TestView {
        fn create_element(&self) -> Box<dyn ElementBase> {
            Box::new(StatelessElement::new(self))
        }
    }

    #[test]
    fn test_child_empty() {
        let child = Child::empty();
        assert!(child.is_none());
        assert!(!child.is_some());
    }

    #[test]
    fn test_child_some() {
        let child = Child::some(TestView);
        assert!(child.is_some());
        assert!(!child.is_none());
    }

    #[test]
    fn test_child_from_view() {
        let child: Child = TestView.into();
        assert!(child.is_some());
    }

    #[test]
    fn test_child_from_option() {
        let child_some: Child = Some(TestView).into();
        let child_none: Child = Child::from_option(None::<TestView>);

        assert!(child_some.is_some());
        assert!(child_none.is_none());
    }

    #[test]
    fn test_child_take() {
        let mut child = Child::some(TestView);
        assert!(child.is_some());

        let taken = child.take();
        assert!(taken.is_some());
        assert!(child.is_none());
    }
}
