//! Adapter container for cross-protocol composition
//!
//! This module provides the `Adapter` container that enables composing render objects
//! from different protocols, such as wrapping a Box render object in a Sliver protocol.

use std::marker::PhantomData;

use flui_tree::arity::{Arity, ChildrenStorage, Exact};

use crate::protocol::Protocol;

/// Adapter for cross-protocol composition
///
/// `Adapter<C, ToProtocol>` wraps a container of one protocol and presents it as another
/// protocol. This is essential for composing Box and Sliver render objects.
///
/// # Type Parameters
///
/// - `C`: The inner container type (must have protocol-typed children)
/// - `ToProtocol`: The target protocol to adapt to
///
/// # Zero-Cost Abstraction
///
/// The adapter uses `PhantomData` and has no runtime overhead - it's a pure compile-time
/// type conversion with no additional memory layout.
///
/// # Examples
///
/// ## Box to Sliver
///
/// ```ignore
/// use flui_rendering::containers::{Adapter, Single};
/// use flui_rendering::protocol::{BoxProtocol, SliverProtocol};
///
/// // Wrap a Box child in a Sliver protocol
/// type BoxToSliver = Adapter<Single<BoxProtocol>, SliverProtocol>;
///
/// struct RenderSliverToBoxAdapter {
///     adapter: BoxToSliver,
/// }
///
/// impl RenderSliverSingleBoxAdapter for RenderSliverToBoxAdapter {
///     fn child(&self) -> Option<&dyn RenderBox> {
///         self.adapter.child()
///             .map(|obj| unsafe { &*(obj as *const _ as *const dyn RenderBox) })
///     }
/// }
/// ```
///
/// ## Multiple Box children in Sliver
///
/// ```ignore
/// use flui_rendering::containers::{Adapter, Children};
///
/// // Multiple Box children in a Sliver protocol
/// type MultiBoxToSliver = Adapter<Children<BoxProtocol>, SliverProtocol>;
///
/// struct RenderSliverList {
///     adapter: MultiBoxToSliver,
/// }
/// ```
#[derive(Debug)]
pub struct Adapter<C, ToProtocol: Protocol> {
    inner: C,
    _phantom: PhantomData<ToProtocol>,
}

impl<C, ToProtocol: Protocol> Adapter<C, ToProtocol> {
    /// Creates a new adapter wrapping the given container
    pub fn new(inner: C) -> Self {
        Self {
            inner,
            _phantom: PhantomData,
        }
    }

    /// Returns a reference to the inner container
    pub fn inner(&self) -> &C {
        &self.inner
    }

    /// Returns a mutable reference to the inner container
    pub fn inner_mut(&mut self) -> &mut C {
        &mut self.inner
    }

    /// Unwraps the adapter, returning the inner container
    pub fn into_inner(self) -> C {
        self.inner
    }
}

// Implement Default if inner container has Default
impl<C: Default, ToProtocol: Protocol> Default for Adapter<C, ToProtocol> {
    fn default() -> Self {
        Self::new(C::default())
    }
}

// Forward ChildrenStorage trait if inner container implements it
impl<C, ToProtocol, A, FromProtocol> ChildrenStorage<Box<ToProtocol::Object>, A>
    for Adapter<C, ToProtocol>
where
    C: ChildrenStorage<Box<FromProtocol::Object>, A>,
    FromProtocol: Protocol,
    ToProtocol: Protocol,
    A: Arity,
{
    fn child_count(&self) -> usize {
        self.inner.child_count()
    }

    fn get_child(&self, index: usize) -> Option<&Box<ToProtocol::Object>> {
        // SAFETY: This transmute is safe because:
        // 1. Box<dyn RenderBox> and Box<dyn RenderSliver> have identical memory layout
        // 2. We're only changing the trait object type, not the actual data
        // 3. The pointer and vtable structure remain the same
        self.inner
            .get_child(index)
            .map(|child| unsafe { &*(child as *const _ as *const Box<ToProtocol::Object>) })
    }

    fn get_child_mut(&mut self, index: usize) -> Option<&mut Box<ToProtocol::Object>> {
        // SAFETY: Same reasoning as get_child
        self.inner
            .get_child_mut(index)
            .map(|child| unsafe { &mut *(child as *mut _ as *mut Box<ToProtocol::Object>) })
    }

    fn add_child(&mut self, _child: Box<ToProtocol::Object>) -> Result<(), Box<ToProtocol::Object>> {
        // Cannot implement generically due to type mismatch
        // Specific implementations should override this
        panic!("add_child not supported on Adapter - use specific adapter type aliases")
    }

    fn insert_child(
        &mut self,
        _index: usize,
        _child: Box<ToProtocol::Object>,
    ) -> Result<(), Box<ToProtocol::Object>> {
        panic!("insert_child not supported on Adapter - use specific adapter type aliases")
    }

    fn remove_child(&mut self, _index: usize) -> Option<Box<ToProtocol::Object>> {
        panic!("remove_child not supported on Adapter - use specific adapter type aliases")
    }

    fn set_single_child(
        &mut self,
        _child: Box<ToProtocol::Object>,
    ) -> Result<(), Box<ToProtocol::Object>> {
        panic!("set_single_child not supported on Adapter - use specific adapter type aliases")
    }

    fn get_single(&self) -> Option<&Box<ToProtocol::Object>> {
        // SAFETY: Safe for the same reasons as get_child
        self.inner
            .get_single()
            .map(|child| unsafe { &*(child as *const _ as *const Box<ToProtocol::Object>) })
    }

    fn get_single_mut(&mut self) -> Option<&mut Box<ToProtocol::Object>> {
        // SAFETY: Safe for the same reasons as get_child_mut
        self.inner
            .get_single_mut()
            .map(|child| unsafe { &mut *(child as *mut _ as *mut Box<ToProtocol::Object>) })
    }

    fn iter_children(&self) -> impl Iterator<Item = &Box<ToProtocol::Object>> {
        // Map each child through pointer casting
        (0..self.inner.child_count()).filter_map(move |i| self.get_child(i))
    }

    fn iter_children_mut(&mut self) -> impl Iterator<Item = &mut Box<ToProtocol::Object>> {
        // We need to return pointers and then reconstruct the iterator
        // This is a bit tricky due to borrowing rules
        let count = self.child_count();
        (0..count).filter_map(move |i| {
            // SAFETY: We're creating non-overlapping mutable references
            unsafe {
                let ptr = self as *mut Self;
                (*ptr).get_child_mut(i)
            }
        })
    }

    fn capacity(&self) -> Option<usize> {
        self.inner.capacity()
    }

    fn reserve(&mut self, additional: usize) {
        self.inner.reserve(additional);
    }

    fn swap_children(&mut self, a: usize, b: usize) -> Result<(), &'static str> {
        self.inner.swap_children(a, b)
    }
}

// Type aliases for common adapter patterns

use crate::containers::{Children, Single};
use crate::parent_data::{BoxParentData, SliverParentData};
use crate::protocol::{BoxProtocol, SliverProtocol};

/// Adapts a single Box child to Sliver protocol
///
/// Used for `RenderSliverSingleBoxAdapter` implementations like:
/// - SliverToBoxAdapter
/// - SliverPadding
pub type BoxToSliver<A = Exact<1>> = Adapter<Single<BoxProtocol, A>, SliverProtocol>;

/// Adapts a single Sliver child to Box protocol
///
/// Less common but used for specialized layout scenarios.
pub type SliverToBox<A = Exact<1>> = Adapter<Single<SliverProtocol, A>, BoxProtocol>;

/// Adapts multiple Box children to Sliver protocol
///
/// Used for `RenderSliverMultiBoxAdaptor` implementations like:
/// - SliverList
/// - SliverGrid
pub type MultiBoxToSliver<PD = BoxParentData, A = flui_tree::arity::Variable> =
    Adapter<Children<BoxProtocol, PD, A>, SliverProtocol>;

/// Adapts multiple Sliver children to Box protocol
///
/// Very rare, but provided for completeness.
pub type MultiSliverToBox<PD = SliverParentData, A = flui_tree::arity::Variable> =
    Adapter<Children<SliverProtocol, PD, A>, BoxProtocol>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adapter_zero_size() {
        // Verify that Adapter is a zero-cost abstraction
        use std::mem::size_of;

        let single_size = size_of::<Single<BoxProtocol>>();
        let adapter_size = size_of::<BoxToSliver>();

        // Adapter should have the same size as inner container
        // (PhantomData is zero-sized)
        assert_eq!(single_size, adapter_size);
    }
}
