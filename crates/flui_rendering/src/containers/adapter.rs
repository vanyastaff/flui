//! Protocol adapter container.
//!
//! The [`Adapter`] container enables cross-protocol composition by wrapping
//! a container of one protocol and exposing a different protocol interface.
//!
//! # Architecture
//!
//! Protocol conversion is encoded in the *type* (`Adapter<Inner, TargetProtocol>`),
//! not in separate methods. This keeps the API simple and provides compile-time
//! guarantees about protocol compatibility.
//!
//! # Zero-Cost Abstraction
//!
//! `Adapter` adds no runtime overhead - `PhantomData<ToProtocol>` is zero-sized,
//! so `Adapter<C, P>` has the same size as `C`.
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_rendering::containers::{BoxToSliver, Single};
//! use flui_rendering::protocol::BoxProtocol;
//!
//! // Sliver that wraps a single Box child
//! pub struct RenderSliverToBoxAdapter {
//!     // Type encodes: Box child, exposed as Sliver protocol
//!     child: BoxToSliver,
//!     geometry: SliverGeometry,
//! }
//!
//! impl RenderSliverToBoxAdapter {
//!     pub fn child(&self) -> Option<&dyn RenderBox> {
//!         self.child.inner().child()
//!     }
//! }
//! ```

use std::fmt::Debug;
use std::marker::PhantomData;

use crate::protocol::Protocol;

/// Protocol adapter container.
///
/// Wraps a container of one protocol and exposes a different protocol.
/// This is a zero-cost wrapper - `PhantomData<ToProtocol>` adds no size.
///
/// # Type Parameters
///
/// - `C`: Inner container type (e.g., `Single<BoxProtocol>`)
/// - `ToProtocol`: Target protocol that this adapter exposes
///
/// # Memory Layout
///
/// ```text
/// Size: Same as inner container (zero-cost wrapper)
/// - inner: C (actual data)
/// - _to_protocol: PhantomData<ToProtocol> (0 bytes)
/// ```
///
/// # Flutter Equivalence
///
/// This pattern corresponds to Flutter's:
/// - `SliverToBoxAdapter` - wraps Box as Sliver
/// - `RenderShrinkWrappingViewport` - wraps Slivers as Box
pub struct Adapter<C, ToProtocol: Protocol> {
    inner: C,
    _to_protocol: PhantomData<ToProtocol>,
}

impl<C: Debug, ToProtocol: Protocol> Debug for Adapter<C, ToProtocol> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Adapter")
            .field("inner", &self.inner)
            .field("to_protocol", &std::any::type_name::<ToProtocol>())
            .finish()
    }
}

impl<C: Default, ToProtocol: Protocol> Default for Adapter<C, ToProtocol> {
    fn default() -> Self {
        Self::new(C::default())
    }
}

impl<C: Clone, ToProtocol: Protocol> Clone for Adapter<C, ToProtocol> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            _to_protocol: PhantomData,
        }
    }
}

impl<C, ToProtocol: Protocol> Adapter<C, ToProtocol> {
    /// Creates a new adapter wrapping the given container.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let single = Single::<BoxProtocol>::new();
    /// let adapter: BoxToSliver = Adapter::new(single);
    /// ```
    #[inline]
    pub const fn new(inner: C) -> Self {
        Self {
            inner,
            _to_protocol: PhantomData,
        }
    }

    /// Returns a reference to the inner container.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let adapter: BoxToSliver = /* ... */;
    /// if let Some(child) = adapter.inner().child() {
    ///     // child is &dyn RenderBox
    /// }
    /// ```
    #[inline]
    pub const fn inner(&self) -> &C {
        &self.inner
    }

    /// Returns a mutable reference to the inner container.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let mut adapter: BoxToSliver = /* ... */;
    /// adapter.inner_mut().set_child(box_child);
    /// ```
    #[inline]
    pub fn inner_mut(&mut self) -> &mut C {
        &mut self.inner
    }

    /// Unwraps the adapter, returning the inner container.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let adapter: BoxToSliver = /* ... */;
    /// let single: Single<BoxProtocol> = adapter.into_inner();
    /// ```
    #[inline]
    pub fn into_inner(self) -> C {
        self.inner
    }

    /// Creates an adapter from an inner container, with explicit protocol annotation.
    ///
    /// This is useful when type inference needs help.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let adapter = Adapter::<_, SliverProtocol>::from_inner(single);
    /// ```
    #[inline]
    pub const fn from_inner(inner: C) -> Self {
        Self::new(inner)
    }
}

// ============================================================================
// Type Aliases - Box → Sliver adapters
// ============================================================================

use super::{Children, Single};
use crate::protocol::{BoxProtocol, SliverProtocol};

// Note: `Single` already stores Option<Box<P::Object>>, so it serves as Optional.
// We define Optional as an alias for clarity in adapter type names.
type Optional<P> = Single<P>;

/// Single Box child exposed as Sliver protocol.
///
/// Use this when a sliver needs to contain exactly one box widget.
///
/// # Flutter Equivalence
///
/// This is the container type for `SliverToBoxAdapter`.
///
/// # Example
///
/// ```rust,ignore
/// pub struct RenderSliverToBoxAdapter {
///     child: BoxToSliver,
///     geometry: SliverGeometry,
/// }
/// ```
pub type BoxToSliver = Adapter<Single<BoxProtocol>, SliverProtocol>;

/// Optional Box child exposed as Sliver protocol.
///
/// Use this when a sliver may optionally contain a box widget.
///
/// # Example
///
/// ```rust,ignore
/// pub struct RenderSliverOptionalBox {
///     child: OptionalBoxToSliver,
///     geometry: SliverGeometry,
/// }
/// ```
pub type OptionalBoxToSliver = Adapter<Optional<BoxProtocol>, SliverProtocol>;

/// Multiple Box children exposed as Sliver protocol.
///
/// Use this when a sliver contains multiple box children.
///
/// # Example
///
/// ```rust,ignore
/// pub struct RenderSliverMultiBox {
///     children: MultiBoxToSliver,
///     geometry: SliverGeometry,
/// }
/// ```
pub type MultiBoxToSliver<PD = <BoxProtocol as Protocol>::ParentData> =
    Adapter<Children<BoxProtocol, PD>, SliverProtocol>;

// ============================================================================
// Type Aliases - Sliver → Box adapters
// ============================================================================

/// Single Sliver child exposed as Box protocol.
///
/// Use this when a box widget wraps a single sliver.
///
/// # Example
///
/// ```rust,ignore
/// pub struct RenderSliverSingleToBox {
///     child: SliverToBox,
///     size: Size,
/// }
/// ```
pub type SliverToBox = Adapter<Single<SliverProtocol>, BoxProtocol>;

/// Optional Sliver child exposed as Box protocol.
///
/// # Example
///
/// ```rust,ignore
/// pub struct RenderOptionalSliverToBox {
///     child: OptionalSliverToBox,
///     size: Size,
/// }
/// ```
pub type OptionalSliverToBox = Adapter<Optional<SliverProtocol>, BoxProtocol>;

/// Multiple Sliver children exposed as Box protocol.
///
/// Use this for viewports that contain multiple slivers.
///
/// # Flutter Equivalence
///
/// This is the container type for `ShrinkWrappingViewport`.
///
/// # Example
///
/// ```rust,ignore
/// pub struct RenderShrinkWrappingViewport {
///     children: MultiSliverToBox,
///     size: Size,
/// }
/// ```
pub type MultiSliverToBox<PD = <SliverProtocol as Protocol>::ParentData> =
    Adapter<Children<SliverProtocol, PD>, BoxProtocol>;

// ============================================================================
// Convenience constructors for BoxToSliver
// ============================================================================

impl BoxToSliver {
    /// Creates an empty BoxToSliver adapter.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let adapter = BoxToSliver::empty();
    /// assert!(!adapter.inner().has_child());
    /// ```
    pub fn empty() -> Self {
        Self::new(Single::new())
    }

    /// Creates a BoxToSliver adapter with the given child.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let adapter = BoxToSliver::with_child(Box::new(my_box_widget));
    /// ```
    pub fn with_child(child: Box<dyn crate::traits::RenderBox>) -> Self {
        Self::new(Single::with_child(child))
    }

    /// Returns a reference to the child, if present.
    pub fn child(&self) -> Option<&dyn crate::traits::RenderBox> {
        self.inner.child()
    }

    /// Returns a mutable reference to the child, if present.
    pub fn child_mut(&mut self) -> Option<&mut dyn crate::traits::RenderBox> {
        self.inner.child_mut()
    }

    /// Sets the child, replacing any existing child.
    pub fn set_child(&mut self, child: Box<dyn crate::traits::RenderBox>) {
        self.inner.set_child(child);
    }

    /// Takes the child out of the adapter.
    pub fn take_child(&mut self) -> Option<Box<dyn crate::traits::RenderBox>> {
        self.inner.take_child()
    }

    /// Returns true if the adapter has a child.
    pub fn has_child(&self) -> bool {
        self.inner.has_child()
    }
}

// ============================================================================
// Convenience constructors for SliverToBox
// ============================================================================

impl SliverToBox {
    /// Creates an empty SliverToBox adapter.
    pub fn empty() -> Self {
        Self::new(Single::new())
    }

    /// Creates a SliverToBox adapter with the given child.
    pub fn with_child(child: Box<dyn crate::traits::RenderSliver>) -> Self {
        Self::new(Single::with_child(child))
    }

    /// Returns a reference to the child, if present.
    pub fn child(&self) -> Option<&dyn crate::traits::RenderSliver> {
        self.inner.child()
    }

    /// Returns a mutable reference to the child, if present.
    pub fn child_mut(&mut self) -> Option<&mut dyn crate::traits::RenderSliver> {
        self.inner.child_mut()
    }

    /// Sets the child, replacing any existing child.
    pub fn set_child(&mut self, child: Box<dyn crate::traits::RenderSliver>) {
        self.inner.set_child(child);
    }

    /// Takes the child out of the adapter.
    pub fn take_child(&mut self) -> Option<Box<dyn crate::traits::RenderSliver>> {
        self.inner.take_child()
    }

    /// Returns true if the adapter has a child.
    pub fn has_child(&self) -> bool {
        self.inner.has_child()
    }
}

// ============================================================================
// Convenience methods for MultiSliverToBox
// ============================================================================

impl<PD: Default> MultiSliverToBox<PD> {
    /// Creates an empty MultiSliverToBox adapter.
    pub fn empty() -> Self {
        Self::new(Children::new())
    }

    /// Creates an adapter with the given capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self::new(Children::with_capacity(capacity))
    }

    /// Adds a sliver child.
    pub fn push(&mut self, child: Box<dyn crate::traits::RenderSliver>) {
        self.inner.push(child);
    }

    /// Returns the number of children.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Returns true if there are no children.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Returns an iterator over the children.
    pub fn iter(&self) -> impl Iterator<Item = &dyn crate::traits::RenderSliver> {
        self.inner.iter()
    }

    /// Returns a mutable iterator over the children.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut dyn crate::traits::RenderSliver> {
        self.inner.iter_mut()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adapter_zero_cost() {
        // Adapter should add no size overhead
        assert_eq!(
            std::mem::size_of::<Adapter<Single<BoxProtocol>, SliverProtocol>>(),
            std::mem::size_of::<Single<BoxProtocol>>()
        );

        assert_eq!(
            std::mem::size_of::<Adapter<Single<SliverProtocol>, BoxProtocol>>(),
            std::mem::size_of::<Single<SliverProtocol>>()
        );

        // PhantomData is zero-sized
        assert_eq!(std::mem::size_of::<PhantomData<SliverProtocol>>(), 0);
        assert_eq!(std::mem::size_of::<PhantomData<BoxProtocol>>(), 0);
    }

    #[test]
    fn test_box_to_sliver_type_alias() {
        // Type alias should be the same as explicit type
        assert_eq!(
            std::mem::size_of::<BoxToSliver>(),
            std::mem::size_of::<Adapter<Single<BoxProtocol>, SliverProtocol>>()
        );
    }

    #[test]
    fn test_sliver_to_box_type_alias() {
        assert_eq!(
            std::mem::size_of::<SliverToBox>(),
            std::mem::size_of::<Adapter<Single<SliverProtocol>, BoxProtocol>>()
        );
    }

    #[test]
    fn test_multi_sliver_to_box_type_alias() {
        assert_eq!(
            std::mem::size_of::<MultiSliverToBox>(),
            std::mem::size_of::<Adapter<Children<SliverProtocol>, BoxProtocol>>()
        );
    }

    #[test]
    fn test_box_to_sliver_empty() {
        let adapter = BoxToSliver::empty();
        assert!(!adapter.has_child());
        assert!(adapter.child().is_none());
    }

    #[test]
    fn test_sliver_to_box_empty() {
        let adapter = SliverToBox::empty();
        assert!(!adapter.has_child());
        assert!(adapter.child().is_none());
    }

    #[test]
    fn test_multi_sliver_to_box_empty() {
        let adapter = MultiSliverToBox::<()>::empty();
        assert!(adapter.is_empty());
        assert_eq!(adapter.len(), 0);
    }

    #[test]
    fn test_adapter_inner_access() {
        let adapter = BoxToSliver::empty();

        // Can access inner container
        let _inner: &Single<BoxProtocol> = adapter.inner();
    }

    #[test]
    fn test_adapter_into_inner() {
        let adapter = BoxToSliver::empty();
        let _inner: Single<BoxProtocol> = adapter.into_inner();
    }

    #[test]
    fn test_adapter_default() {
        let adapter: BoxToSliver = Default::default();
        assert!(!adapter.has_child());
    }

    // Note: Clone test removed because Single<P> doesn't implement Clone
    // (it contains Box<dyn RenderBox> which is not Clone)

    #[test]
    fn test_adapter_debug() {
        let adapter = BoxToSliver::empty();
        let debug_str = format!("{:?}", adapter);
        assert!(debug_str.contains("Adapter"));
        assert!(debug_str.contains("SliverProtocol"));
    }
}
