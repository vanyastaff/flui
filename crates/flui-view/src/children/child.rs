//! Optional single child wrapper.

use crate::{IntoView, ViewObject};

/// Optional single child wrapper.
///
/// Provides a cleaner API than `Option<Box<dyn ViewObject>>` for single-child widgets.
///
/// # Examples
///
/// ```rust,ignore
/// pub struct Padding {
///     padding: EdgeInsets,
///     child: Child,
/// }
///
/// impl Padding {
///     pub fn new(padding: EdgeInsets) -> Self {
///         Self { padding, child: Child::none() }
///     }
///
///     pub fn child(mut self, child: impl IntoView) -> Self {
///         self.child = Child::new(child);
///         self
///     }
/// }
/// ```
#[derive(Default)]
pub struct Child {
    inner: Option<Box<dyn ViewObject>>,
}

impl std::fmt::Debug for Child {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Child")
            .field("has_child", &self.inner.is_some())
            .finish()
    }
}

impl Child {
    /// Creates an empty child.
    #[inline]
    pub fn none() -> Self {
        Self { inner: None }
    }

    /// Creates a child from a view.
    #[inline]
    pub fn new<V: IntoView>(view: V) -> Self {
        Self {
            inner: Some(view.into_view()),
        }
    }

    /// Creates a child from a boxed ViewObject.
    #[inline]
    pub fn from_view_object(view_object: Box<dyn ViewObject>) -> Self {
        Self {
            inner: Some(view_object),
        }
    }

    /// Returns `true` if empty.
    #[inline]
    pub fn is_none(&self) -> bool {
        self.inner.is_none()
    }

    /// Returns `true` if has child.
    #[inline]
    pub fn is_some(&self) -> bool {
        self.inner.is_some()
    }

    /// Converts to `Option<Box<dyn ViewObject>>`.
    #[inline]
    pub fn into_inner(self) -> Option<Box<dyn ViewObject>> {
        self.inner
    }

    /// Takes the view object out of Child, leaving None in its place.
    #[inline]
    pub fn take(&mut self) -> Option<Box<dyn ViewObject>> {
        self.inner.take()
    }

    /// Maps the view object if present.
    #[inline]
    pub fn map<F, U>(self, f: F) -> Option<U>
    where
        F: FnOnce(Box<dyn ViewObject>) -> U,
    {
        self.inner.map(f)
    }
}

impl IntoView for Child {
    fn into_view(self) -> Box<dyn ViewObject> {
        match self.inner {
            Some(view_object) => view_object,
            None => crate::EmptyView.into_view(),
        }
    }
}

impl From<Child> for Option<Box<dyn ViewObject>> {
    fn from(child: Child) -> Self {
        child.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_child_none() {
        let child = Child::none();
        assert!(child.is_none());
        assert!(!child.is_some());
    }

    #[test]
    fn test_child_default() {
        let child = Child::default();
        assert!(child.is_none());
    }

    #[test]
    fn test_child_into_view() {
        let child = Child::none();
        let view_obj = child.into_view();
        assert_eq!(view_obj.mode(), crate::ViewMode::Stateless);
    }
}
