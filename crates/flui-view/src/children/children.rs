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
    pub fn new() -> Self {
        Self { inner: Vec::new() }
    }

    /// Creates with pre-allocated capacity.
    #[inline]
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
    pub fn extend<V, I>(&mut self, views: I)
    where
        V: IntoView,
        I: IntoIterator<Item = V>,
    {
        for view in views {
            self.push(view);
        }
    }

    /// Returns the number of children.
    #[inline]
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Returns `true` if empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Clears all children.
    #[inline]
    pub fn clear(&mut self) {
        self.inner.clear();
    }

    /// Converts to `Vec<Box<dyn ViewObject>>`.
    #[inline]
    pub fn into_inner(self) -> Vec<Box<dyn ViewObject>> {
        self.inner
    }
}

impl<V: IntoView> FromIterator<V> for Children {
    fn from_iter<I: IntoIterator<Item = V>>(iter: I) -> Self {
        let mut children = Children::new();
        children.extend(iter);
        children
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
}
