//! Multiple children wrapper.
//!
//! Provides an ergonomic wrapper for Views with multiple children.

use crate::view::{BoxedView, View};

/// A wrapper for multiple child Views.
///
/// This provides a consistent API for Views that accept multiple children,
/// with builder-style methods for adding children.
///
/// # Example
///
/// ```rust,ignore
/// use flui_view::{Children, View};
///
/// struct Column {
///     children: Children,
/// }
///
/// impl Column {
///     pub fn new() -> Self {
///         Self { children: Children::new() }
///     }
///
///     pub fn child(mut self, child: impl View) -> Self {
///         self.children.push(child);
///         self
///     }
///
///     pub fn children(mut self, children: impl IntoIterator<Item = impl View>) -> Self {
///         self.children.extend(children);
///         self
///     }
/// }
/// ```
#[derive(Default)]
pub struct Children {
    inner: Vec<BoxedView>,
}

impl Children {
    /// Create an empty Children collection.
    pub fn new() -> Self {
        Self { inner: Vec::new() }
    }

    /// Create Children with pre-allocated capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Vec::with_capacity(capacity),
        }
    }

    /// Check if there are no children.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Get the number of children.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Add a child to the end.
    pub fn push(&mut self, view: impl View) {
        self.inner.push(BoxedView(Box::new(view)));
    }

    /// Add multiple children.
    pub fn extend(&mut self, views: impl IntoIterator<Item = impl View>) {
        self.inner
            .extend(views.into_iter().map(|v| BoxedView(Box::new(v))));
    }

    /// Insert a child at the given index.
    pub fn insert(&mut self, index: usize, view: impl View) {
        self.inner.insert(index, BoxedView(Box::new(view)));
    }

    /// Remove a child at the given index.
    pub fn remove(&mut self, index: usize) -> Option<BoxedView> {
        if index < self.inner.len() {
            Some(self.inner.remove(index))
        } else {
            None
        }
    }

    /// Clear all children.
    pub fn clear(&mut self) {
        self.inner.clear();
    }

    /// Get a reference to a child at the given index.
    pub fn get(&self, index: usize) -> Option<&dyn View> {
        self.inner.get(index).map(|b| &*b.0 as &dyn View)
    }

    /// Iterate over children as View references.
    pub fn iter(&self) -> impl Iterator<Item = &dyn View> {
        self.inner.iter().map(|b| &*b.0 as &dyn View)
    }

    /// Take all children, leaving an empty collection.
    pub fn take(&mut self) -> Vec<BoxedView> {
        std::mem::take(&mut self.inner)
    }

    /// Get the inner Vec of BoxedViews.
    pub fn into_inner(self) -> Vec<BoxedView> {
        self.inner
    }

    /// Get a slice of BoxedViews.
    pub fn as_slice(&self) -> &[BoxedView] {
        &self.inner
    }
}

impl Clone for Children {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl std::fmt::Debug for Children {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Children")
            .field("len", &self.inner.len())
            .finish()
    }
}

impl<V: View> FromIterator<V> for Children {
    fn from_iter<I: IntoIterator<Item = V>>(iter: I) -> Self {
        let inner = iter.into_iter().map(|v| BoxedView(Box::new(v))).collect();
        Self { inner }
    }
}

impl<V: View> Extend<V> for Children {
    fn extend<I: IntoIterator<Item = V>>(&mut self, iter: I) {
        self.inner
            .extend(iter.into_iter().map(|v| BoxedView(Box::new(v))));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ElementBase, StatelessElement, StatelessView};

    #[derive(Clone)]
    struct TestView(u32);

    impl StatelessView for TestView {
        fn build(&self, _ctx: &dyn crate::BuildContext) -> Box<dyn View> {
            Box::new(TestView(self.0))
        }
    }

    impl View for TestView {
        fn create_element(&self) -> Box<dyn ElementBase> {
            Box::new(StatelessElement::new(self))
        }
    }

    #[test]
    fn test_children_empty() {
        let children = Children::new();
        assert!(children.is_empty());
        assert_eq!(children.len(), 0);
    }

    #[test]
    fn test_children_push() {
        let mut children = Children::new();
        children.push(TestView(1));
        children.push(TestView(2));

        assert!(!children.is_empty());
        assert_eq!(children.len(), 2);
    }

    #[test]
    fn test_children_from_iter() {
        let views = vec![TestView(1), TestView(2), TestView(3)];
        let children: Children = views.into_iter().collect();

        assert_eq!(children.len(), 3);
    }

    #[test]
    fn test_children_extend() {
        let mut children = Children::new();
        children.push(TestView(1));

        let more = vec![TestView(2), TestView(3)];
        children.extend(more);

        assert_eq!(children.len(), 3);
    }

    #[test]
    fn test_children_remove() {
        let mut children = Children::new();
        children.push(TestView(1));
        children.push(TestView(2));

        let removed = children.remove(0);
        assert!(removed.is_some());
        assert_eq!(children.len(), 1);
    }

    #[test]
    fn test_children_clear() {
        let mut children = Children::new();
        children.push(TestView(1));
        children.push(TestView(2));

        children.clear();
        assert!(children.is_empty());
    }

    #[test]
    fn test_children_iter() {
        let mut children = Children::new();
        children.push(TestView(1));
        children.push(TestView(2));

        let count = children.iter().count();
        assert_eq!(count, 2);
    }
}
