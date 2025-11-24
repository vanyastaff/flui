//! Ergonomic child types for view composition.

use crate::element::{Element, IntoElement};

/// Optional single child wrapper.
///
/// Provides a cleaner API than `Option<Element>` for single-child widgets.
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
///     pub fn child(mut self, child: impl IntoElement) -> Self {
///         self.child = Child::new(child);
///         self
///     }
/// }
/// ```
#[derive(Debug, Default)]
pub struct Child {
    inner: Option<Element>,
}

impl Child {
    /// Creates an empty child.
    #[inline]
    pub fn none() -> Self {
        Self { inner: None }
    }

    /// Creates a child from a view.
    #[inline]
    pub fn new<V: IntoElement>(view: V) -> Self {
        Self {
            inner: Some(view.into_element()),
        }
    }

    /// Creates a child from an element.
    #[inline]
    pub fn from_element(element: Element) -> Self {
        Self {
            inner: Some(element),
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

    /// Converts to `Option<Element>`.
    #[inline]
    pub fn into_inner(self) -> Option<Element> {
        self.inner
    }

    /// Takes the element out of Child, leaving None in its place.
    #[inline]
    pub fn take(&mut self) -> Option<Element> {
        self.inner.take()
    }

    /// Maps the element if present.
    #[inline]
    pub fn map<F, U>(self, f: F) -> Option<U>
    where
        F: FnOnce(Element) -> U,
    {
        self.inner.map(f)
    }
}

impl IntoElement for Child {
    fn into_element(self) -> Element {
        match self.inner {
            Some(element) => element,
            None => {
                use crate::render::{EmptyRender, RenderBoxExt};
                EmptyRender.leaf().into_element()
            }
        }
    }
}

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

impl crate::element::into_element::sealed::Sealed for Child {}

impl From<Child> for Option<Element> {
    fn from(child: Child) -> Self {
        child.inner
    }
}

impl From<Children> for Vec<Element> {
    fn from(children: Children) -> Self {
        children.inner
    }
}

// Allow Vec<V> where V: IntoElement to be converted to Children
// This enables: .children(vec![Text::new("A"), Text::new("B")])
// Also works for Vec<Element> since Element: IntoElement
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
