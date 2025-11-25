//! Multiple children wrapper.

use flui_element::{Element, IntoElement};

/// Multiple children wrapper.
///
/// Provides a cleaner API than `Vec<Element>` for multi-child widgets.
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
///     pub fn child(mut self, child: impl IntoElement) -> Self {
///         self.children.push(child);
///         self
///     }
/// }
/// ```
#[derive(Debug, Default)]
pub struct Children {
    inner: Vec<Element>,
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
    pub fn push<V: IntoElement>(&mut self, view: V) {
        self.inner.push(view.into_element());
    }

    /// Adds an element.
    #[inline]
    pub fn push_element(&mut self, element: Element) {
        self.inner.push(element);
    }

    /// Extends with multiple children.
    pub fn extend<V, I>(&mut self, views: I)
    where
        V: IntoElement,
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

    /// Converts to `Vec<Element>`.
    #[inline]
    pub fn into_inner(self) -> Vec<Element> {
        self.inner
    }
}

impl<V: IntoElement> FromIterator<V> for Children {
    fn from_iter<I: IntoIterator<Item = V>>(iter: I) -> Self {
        let mut children = Children::new();
        children.extend(iter);
        children
    }
}

impl IntoIterator for Children {
    type Item = Element;
    type IntoIter = std::vec::IntoIter<Element>;

    fn into_iter(self) -> Self::IntoIter {
        self.inner.into_iter()
    }
}

impl From<Children> for Vec<Element> {
    fn from(children: Children) -> Self {
        children.inner
    }
}

// Allow Vec<V> where V: IntoElement to be converted to Children
impl<V: IntoElement> From<Vec<V>> for Children {
    fn from(views: Vec<V>) -> Self {
        Children {
            inner: views.into_iter().map(|v| v.into_element()).collect(),
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
}
