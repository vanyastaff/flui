//! Optional single child wrapper.

use flui_element::{Element, IntoElement};

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
            None => Element::empty(),
        }
    }
}

impl From<Child> for Option<Element> {
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
}
