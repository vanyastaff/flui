//! Multiple children wrapper.

use crate::{IntoView, ViewObject};

/// Multiple children wrapper.
///
/// Provides a cleaner API than `Vec<Box<dyn ViewObject>>` for multi-child widgets.
///
/// # Examples
///
/// ```rust,ignore
/// pub struct Column {
///     children: Children,
/// }
///
/// impl Column {
///     pub fn new() -> Self {
///         Self { children: Children::new() }
///     }
///
///     pub fn child(mut self, child: impl IntoView) -> Self {
///         self.children.push(child);
///         self
///     }
/// }
/// ```
///
/// # Performance
///
/// - Uses `Vec` internally for cache-friendly access
/// - Reserves capacity when extending with sized iterators
/// - Provides `shrink_to_fit()` to reduce memory usage
#[derive(Default)]
pub struct Children {
    inner: Vec<Box<dyn ViewObject>>,
}

impl std::fmt::Debug for Children {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Children")
            .field("count", &self.inner.len())
            .finish()
    }
}

impl Children {
    /// Creates an empty list.
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self { inner: Vec::new() }
    }

    /// Creates with pre-allocated capacity.
    #[inline]
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Vec::with_capacity(capacity),
        }
    }

    /// Adds a child.
    #[inline]
    pub fn push<V: IntoView>(&mut self, view: V) {
        self.inner.push(view.into_view());
    }

    /// Adds a boxed `ViewObject`.
    #[inline]
    pub fn push_view_object(&mut self, view_object: Box<dyn ViewObject>) {
        self.inner.push(view_object);
    }

    /// Extends with multiple children.
    ///
    /// Reserves capacity if the iterator provides a size hint for better performance.
    #[inline]
    pub fn extend<V, I>(&mut self, views: I)
    where
        V: IntoView,
        I: IntoIterator<Item = V>,
    {
        let iter = views.into_iter();
        // Reserve capacity based on size hint for better performance
        let (lower, _) = iter.size_hint();
        self.inner.reserve(lower);

        for view in iter {
            self.push(view);
        }
    }

    /// Returns the number of children.
    #[inline]
    #[must_use]
    pub const fn len(&self) -> usize {
        self.inner.len()
    }

    /// Returns `true` if empty.
    #[inline]
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Clears all children.
    #[inline]
    pub fn clear(&mut self) {
        self.inner.clear();
    }

    /// Converts to `Vec<Box<dyn ViewObject>>`.
    #[inline]
    #[must_use]
    pub fn into_inner(self) -> Vec<Box<dyn ViewObject>> {
        self.inner
    }

    /// Returns the current capacity.
    #[inline]
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.inner.capacity()
    }

    /// Reserves capacity for at least `additional` more children.
    #[inline]
    pub fn reserve(&mut self, additional: usize) {
        self.inner.reserve(additional);
    }

    /// Shrinks capacity to fit the number of children.
    #[inline]
    pub fn shrink_to_fit(&mut self) {
        self.inner.shrink_to_fit();
    }

    /// Returns an iterator over the children.
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = &dyn ViewObject> + '_ {
        self.inner.iter().map(|b| b.as_ref())
    }

    /// Returns a mutable iterator over the children.
    #[inline]
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut dyn ViewObject> + '_ {
        self.inner.iter_mut().map(|b| b.as_mut())
    }
}

impl<V: IntoView> FromIterator<V> for Children {
    fn from_iter<I: IntoIterator<Item = V>>(iter: I) -> Self {
        let iter = iter.into_iter();
        let (lower, _) = iter.size_hint();
        let mut children = Children::with_capacity(lower);
        children.extend(iter);
        children
    }
}

// Implement Extend trait for idiomatic Rust
impl<V: IntoView> Extend<V> for Children {
    #[inline]
    fn extend<I: IntoIterator<Item = V>>(&mut self, iter: I) {
        Children::extend(self, iter);
    }
}

impl IntoIterator for Children {
    type Item = Box<dyn ViewObject>;
    type IntoIter = std::vec::IntoIter<Box<dyn ViewObject>>;

    fn into_iter(self) -> Self::IntoIter {
        self.inner.into_iter()
    }
}

impl From<Children> for Vec<Box<dyn ViewObject>> {
    fn from(children: Children) -> Self {
        children.inner
    }
}

// Allow Vec<V> where V: IntoView to be converted to Children
impl<V: IntoView> From<Vec<V>> for Children {
    fn from(views: Vec<V>) -> Self {
        Children {
            inner: views.into_iter().map(IntoView::into_view).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_children_new() {
        let children = Children::new();
        assert!(children.is_empty());
        assert_eq!(children.len(), 0);
    }

    #[test]
    fn test_children_with_capacity() {
        let children = Children::with_capacity(10);
        assert!(children.is_empty());
        assert!(children.capacity() >= 10);
    }

    #[test]
    fn test_children_push() {
        let mut children = Children::new();
        children.push(crate::EmptyView);
        assert_eq!(children.len(), 1);
    }

    #[test]
    fn test_children_from_vec() {
        let views = vec![crate::EmptyView, crate::EmptyView];
        let children: Children = views.into();
        assert_eq!(children.len(), 2);
    }

    #[test]
    fn test_children_extend() {
        let mut children = Children::new();
        children.extend(vec![crate::EmptyView, crate::EmptyView]);
        assert_eq!(children.len(), 2);
    }

    #[test]
    fn test_children_extend_trait() {
        let mut children = Children::new();
        // Test that Extend trait is implemented
        Extend::extend(&mut children, vec![crate::EmptyView, crate::EmptyView]);
        assert_eq!(children.len(), 2);
    }

    #[test]
    fn test_children_iter() {
        let mut children = Children::new();
        children.push(crate::EmptyView);
        children.push(crate::EmptyView);

        let count = children.iter().count();
        assert_eq!(count, 2);
    }
}
