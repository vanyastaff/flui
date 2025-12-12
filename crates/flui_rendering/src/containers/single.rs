//! Single child container

use std::marker::PhantomData;

use crate::protocol::Protocol;

/// Container for zero or one child of a specific protocol
///
/// `Single<P>` stores an optional child using the protocol's object type.
/// This provides compile-time type safety - a Box container can only hold
/// Box children, and a Sliver container can only hold Sliver children.
///
/// # Type Parameters
///
/// - `P`: The protocol (BoxProtocol or SliverProtocol)
///
/// # Examples
///
/// ```ignore
/// use flui_rendering::containers::Single;
/// use flui_rendering::protocol::BoxProtocol;
///
/// let mut container: Single<BoxProtocol> = Single::new();
/// container.set_child(Box::new(my_render_box));
///
/// if let Some(child) = container.child() {
///     // child is &dyn RenderBox - no downcasting needed!
///     let size = child.size();
/// }
/// ```
#[derive(Debug)]
pub struct Single<P: Protocol> {
    child: Option<Box<P::Object>>,
    _phantom: PhantomData<P>,
}

impl<P: Protocol> Single<P> {
    /// Creates a new empty container
    pub fn new() -> Self {
        Self {
            child: None,
            _phantom: PhantomData,
        }
    }

    /// Returns a reference to the child, if any
    pub fn child(&self) -> Option<&P::Object> {
        self.child.as_deref()
    }

    /// Returns a mutable reference to the child, if any
    pub fn child_mut(&mut self) -> Option<&mut P::Object> {
        self.child.as_deref_mut()
    }

    /// Sets the child, replacing any existing child
    pub fn set_child(&mut self, child: Box<P::Object>) {
        self.child = Some(child);
    }

    /// Takes the child, leaving None in its place
    pub fn take_child(&mut self) -> Option<Box<P::Object>> {
        self.child.take()
    }

    /// Returns whether this container has a child
    pub fn has_child(&self) -> bool {
        self.child.is_some()
    }

    /// Clears the child
    pub fn clear(&mut self) {
        self.child = None;
    }
}

impl<P: Protocol> Default for Single<P> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::BoxProtocol;

    #[test]
    fn test_new() {
        let container: Single<BoxProtocol> = Single::new();
        assert!(!container.has_child());
        assert!(container.child().is_none());
    }

    #[test]
    fn test_clear() {
        let mut container: Single<BoxProtocol> = Single::new();
        // We can't easily test with real RenderBox here,
        // but we can test the API
        assert!(!container.has_child());
        container.clear();
        assert!(!container.has_child());
    }
}
