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
///
/// # Comparison with Option
///
/// `Child` provides type-safe methods specific to view objects while
/// maintaining `Option`-like semantics. It automatically converts
/// `None` to `EmptyView` when building.
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
    #[must_use]
    pub const fn none() -> Self {
        Self { inner: None }
    }

    /// Creates a child from a view.
    #[inline]
    pub fn new<V: IntoView>(view: V) -> Self {
        Self {
            inner: Some(view.into_view()),
        }
    }

    /// Creates a child from a boxed `ViewObject`.
    #[inline]
    #[must_use]
    pub const fn from_view_object(view_object: Box<dyn ViewObject>) -> Self {
        Self {
            inner: Some(view_object),
        }
    }

    /// Creates a child from an `Option<Box<dyn ViewObject>>`.
    #[inline]
    #[must_use]
    pub const fn from_option(option: Option<Box<dyn ViewObject>>) -> Self {
        Self { inner: option }
    }

    /// Returns `true` if empty.
    #[inline]
    #[must_use]
    pub const fn is_none(&self) -> bool {
        self.inner.is_none()
    }

    /// Returns `true` if has child.
    #[inline]
    #[must_use]
    pub const fn is_some(&self) -> bool {
        self.inner.is_some()
    }

    /// Converts to `Option<Box<dyn ViewObject>>`.
    #[inline]
    #[must_use]
    pub fn into_inner(self) -> Option<Box<dyn ViewObject>> {
        self.inner
    }

    /// Takes the view object out of Child, leaving None in its place.
    #[inline]
    pub fn take(&mut self) -> Option<Box<dyn ViewObject>> {
        self.inner.take()
    }

    /// Replaces the child, returning the old one if present.
    #[inline]
    pub fn replace<V: IntoView>(&mut self, view: V) -> Option<Box<dyn ViewObject>> {
        self.inner.replace(view.into_view())
    }

    /// Maps the view object if present.
    #[inline]
    pub fn map<F, U>(self, f: F) -> Option<U>
    where
        F: FnOnce(Box<dyn ViewObject>) -> U,
    {
        self.inner.map(f)
    }

    /// Maps the view object in place if present.
    #[inline]
    pub fn map_in_place<F>(&mut self, f: F)
    where
        F: FnOnce(&mut Box<dyn ViewObject>),
    {
        if let Some(ref mut view_obj) = self.inner {
            f(view_obj);
        }
    }

    /// Returns a reference to the inner view object if present.
    #[inline]
    #[must_use]
    pub fn as_ref(&self) -> Option<&dyn ViewObject> {
        self.inner.as_ref().map(AsRef::as_ref)
    }

    /// Returns a mutable reference to the inner view object if present.
    #[inline]
    #[must_use]
    pub fn as_mut(&mut self) -> Option<&mut dyn ViewObject> {
        self.inner.as_mut().map(AsMut::as_mut)
    }

    /// Tries to downcast the inner view object to a specific type.
    #[inline]
    pub fn downcast_ref<T: 'static>(&self) -> Option<&T> {
        self.as_ref()?.as_any().downcast_ref::<T>()
    }

    /// Tries to downcast the inner view object to a specific type (mutable).
    #[inline]
    pub fn downcast_mut<T: 'static>(&mut self) -> Option<&mut T> {
        self.as_mut()?.as_any_mut().downcast_mut::<T>()
    }

    /// Unwraps the child, panicking if None.
    ///
    /// # Panics
    ///
    /// Panics if the child is None.
    #[inline]
    #[must_use]
    pub fn unwrap(self) -> Box<dyn ViewObject> {
        self.inner.unwrap()
    }

    /// Returns the child or the provided default.
    #[inline]
    #[must_use]
    pub fn unwrap_or<V: IntoView>(self, default: V) -> Box<dyn ViewObject> {
        self.inner.unwrap_or_else(|| default.into_view())
    }

    /// Returns the child or computes it from a closure.
    #[inline]
    pub fn unwrap_or_else<F>(self, f: F) -> Box<dyn ViewObject>
    where
        F: FnOnce() -> Box<dyn ViewObject>,
    {
        self.inner.unwrap_or_else(f)
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
    #[inline]
    fn from(child: Child) -> Self {
        child.inner
    }
}

impl<V: IntoView> From<Option<V>> for Child {
    #[inline]
    fn from(option: Option<V>) -> Self {
        match option {
            Some(view) => Child::new(view),
            None => Child::none(),
        }
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
        assert_eq!(view_obj.mode(), crate::ViewMode::Empty);
    }

    #[test]
    fn test_child_as_ref() {
        let child = Child::none();
        assert!(child.as_ref().is_none());
    }

    #[test]
    fn test_child_from_option() {
        let child: Child = Some(crate::EmptyView).into();
        assert!(child.is_some());

        let child: Child = None::<crate::EmptyView>.into();
        assert!(child.is_none());
    }

    #[test]
    fn test_child_replace() {
        let mut child = Child::new(crate::EmptyView);
        assert!(child.is_some());

        let old = child.replace(crate::EmptyView);
        assert!(old.is_some());
        assert!(child.is_some());
    }

    #[test]
    fn test_child_unwrap_or() {
        let child = Child::none();
        let view = child.unwrap_or(crate::EmptyView);
        assert_eq!(view.mode(), crate::ViewMode::Empty);
    }
}
