// ! Single child storage for render objects
//!
//! Flutter equivalent: `RenderObjectWithChildMixin<ChildType>`

use flui_foundation::RenderId;

use crate::protocol::Protocol;

/// Single optional child storage (Flutter: RenderObjectWithChildMixin)
///
/// # Type Parameters
///
/// - `P`: Protocol type (BoxProtocol or SliverProtocol)
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{Child, BoxProtocol};
///
/// struct RenderPadding {
///     child: Child<BoxProtocol>,
///     padding: EdgeInsets,
/// }
/// ```
#[derive(Debug)]
pub struct Child<P: Protocol> {
    inner: Option<RenderId>,
    _phantom: std::marker::PhantomData<P>,
}

impl<P: Protocol> Child<P> {
    /// Create empty child slot
    #[inline]
    pub fn new() -> Self {
        Self {
            inner: None,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Create with child
    #[inline]
    pub fn with(child: RenderId) -> Self {
        Self {
            inner: Some(child),
            _phantom: std::marker::PhantomData,
        }
    }

    /// Get child reference
    #[inline]
    pub fn get(&self) -> Option<RenderId> {
        self.inner
    }

    /// Set child (replaces existing)
    #[inline]
    pub fn set(&mut self, child: Option<RenderId>) {
        self.inner = child;
    }

    /// Take child out
    #[inline]
    pub fn take(&mut self) -> Option<RenderId> {
        self.inner.take()
    }

    /// Check if has child
    #[inline]
    pub fn is_some(&self) -> bool {
        self.inner.is_some()
    }

    /// Check if empty
    #[inline]
    pub fn is_none(&self) -> bool {
        self.inner.is_none()
    }
}

impl<P: Protocol> Default for Child<P> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P: Protocol> Clone for Child<P> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner,
            _phantom: std::marker::PhantomData,
        }
    }
}

// ============================================================================
// Type Aliases
// ============================================================================

/// Single Box child (Flutter: RenderObjectWithChildMixin<RenderBox>)
pub type BoxChild = Child<crate::protocol::BoxProtocol>;

/// Single Sliver child (Flutter: RenderObjectWithChildMixin<RenderSliver>)
pub type SliverChild = Child<crate::protocol::SliverProtocol>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::BoxProtocol;

    #[test]
    fn test_child_basic() {
        let mut child: Child<BoxProtocol> = Child::new();
        assert!(child.is_none());

        let id = RenderId::new(1);
        child.set(Some(id));
        assert!(child.is_some());
        assert_eq!(child.get(), Some(id));

        let taken = child.take();
        assert_eq!(taken, Some(id));
        assert!(child.is_none());
    }

    #[test]
    fn test_child_with() {
        let id = RenderId::new(42);
        let child = Child::<BoxProtocol>::with(id);
        assert!(child.is_some());
        assert_eq!(child.get(), Some(id));
    }
}
