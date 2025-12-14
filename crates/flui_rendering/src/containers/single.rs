//! Single child container.

use crate::protocol::{BoxProtocol, Protocol, SliverProtocol};
use std::fmt::Debug;
use std::marker::PhantomData;

/// Container that stores zero or one child of a protocol's object type.
///
/// # Type Safety
///
/// Uses `Protocol::Object` to ensure type-safe child storage at compile time.
/// For `BoxProtocol`, child is `Box<dyn RenderBox>`.
/// For `SliverProtocol`, child is `Box<dyn RenderSliver>`.
///
/// # Example
///
/// ```rust,ignore
/// pub struct RenderOpacity {
///     child: BoxChild,  // = Single<BoxProtocol>
///     opacity: f32,
/// }
///
/// impl RenderOpacity {
///     fn paint(&self, ctx: &mut PaintingContext, offset: Offset) {
///         if let Some(child) = self.child.child() {
///             // child is &dyn RenderBox - direct access!
///             ctx.push_opacity(self.opacity);
///             ctx.paint_child(child, offset);
///         }
///     }
/// }
/// ```
pub struct Single<P: Protocol> {
    child: Option<Box<P::Object>>,
    _phantom: PhantomData<P>,
}

impl<P: Protocol> Debug for Single<P>
where
    P::Object: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Single")
            .field("has_child", &self.child.is_some())
            .finish()
    }
}

impl<P: Protocol> Default for Single<P> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P: Protocol> Single<P> {
    /// Creates a new empty single child container.
    pub fn new() -> Self {
        Self {
            child: None,
            _phantom: PhantomData,
        }
    }

    /// Creates a single child container with the given child.
    pub fn with_child(child: Box<P::Object>) -> Self {
        Self {
            child: Some(child),
            _phantom: PhantomData,
        }
    }

    /// Returns a reference to the child, if present.
    pub fn child(&self) -> Option<&P::Object> {
        self.child.as_deref()
    }

    /// Returns a mutable reference to the child, if present.
    pub fn child_mut(&mut self) -> Option<&mut P::Object> {
        self.child.as_deref_mut()
    }

    /// Sets the child, replacing any existing child.
    pub fn set_child(&mut self, child: Box<P::Object>) {
        self.child = Some(child);
    }

    /// Takes the child out of the container, leaving it empty.
    pub fn take_child(&mut self) -> Option<Box<P::Object>> {
        self.child.take()
    }

    /// Returns `true` if the container has a child.
    pub fn has_child(&self) -> bool {
        self.child.is_some()
    }

    /// Clears the container, removing the child.
    pub fn clear(&mut self) {
        self.child = None;
    }
}

/// Type alias for a single Box protocol child.
pub type BoxChild = Single<BoxProtocol>;

/// Type alias for a single Sliver protocol child.
pub type SliverChild = Single<SliverProtocol>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_default_is_empty() {
        let single: BoxChild = Single::new();
        assert!(!single.has_child());
        assert!(single.child().is_none());
    }
}
