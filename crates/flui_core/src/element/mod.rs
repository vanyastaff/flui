//! Element system - Widget lifecycle and tree management
//!
//! This module provides the Element layer of the three-tree architecture:
//! - **Widget** → Immutable configuration (recreated each rebuild)
//! - **Element** → Mutable state holder (persists across rebuilds)
//! - **Render** → Layout and painting (optional, for render widgets)
//!
//! # Element Types
//!
//! 1. **ComponentElement** - For StatelessWidget (calls build())
//! 2. **StatefulElement** - For StatefulWidget (manages State object)
//! 3. **InheritedElement** - For InheritedWidget (data propagation + dependency tracking)
//! 4. **ParentDataElement** - For ParentDataWidget (attaches metadata to children)
//! 5. **RenderElement** - For RenderWidget (owns Render)
//!
//! # Architecture
//!
//! ```text
//! Widget → Element → Render (optional)
//!
//! StatelessWidget     → ComponentElement  → build() → child widget
//! StatefulWidget      → StatefulElement   → State.build() → child widget
//! InheritedWidget     → InheritedElement  → (data + dependents) → child widget
//! ParentDataWidget    → ParentDataElement → (attach data) → child widget
//! RenderWidget  → RenderElement     → Render (type-erased)
//! ```
//!
//! # ElementTree
//!
//! The ElementTree currently stores Renders directly (will be refactored to store Elements):
//! - **Renders** for rendering (temporary, will become part of RenderElement)
//! - **RenderState** per Render (size, constraints, dirty flags)
//! - **Tree relationships** (parent/children) via ElementId indices
//!
//! # Performance
//!
//! - **O(1) access** by ElementId (direct slab indexing)
//! - **Cache-friendly** layout (contiguous memory in slab)
//! - **Lock-free reads** for RenderState flags via atomic operations

// Modules
pub mod component;
pub mod dependency;
#[allow(clippy::module_inception)]  // element/element.rs is intentional for main Element enum
pub mod element;
pub mod element_base;
pub mod lifecycle;
pub mod provider;
pub mod render;

// Re-exports
pub use component::ComponentElement;
pub use dependency::{DependencyInfo, DependencyTracker};
pub use element::Element;
pub use element_base::ElementBase;
pub use lifecycle::ElementLifecycle;
pub use provider::InheritedElement;  // Re-exported with old name for compatibility
pub use render::RenderElement;

// Moved to other modules (Phase 1):
// - BuildContext moved to view::BuildContext
// - ElementTree moved to pipeline::ElementTree
// - PipelineOwner moved to pipeline::PipelineOwner

/// Element ID - stable index into the ElementTree slab
///
/// Uses `NonZeroUsize` internally for niche optimization:
/// - `Option<ElementId>` is same size as `ElementId` (no extra byte)
/// - Prevents 0 from being a valid ID (0 reserved for sentinel)
/// - Enables pattern matching on Option without branching overhead
///
/// This is a handle to an element that remains valid until the element is removed.
/// ElementIds are reused after removal (slab behavior), so don't store them long-term
/// without verifying the element still exists.
///
/// # Examples
///
/// ```rust
/// use flui_core::ElementId;
///
/// // Option<ElementId> is same size as ElementId (8 bytes on 64-bit)
/// assert_eq!(
///     std::mem::size_of::<ElementId>(),
///     std::mem::size_of::<Option<ElementId>>()
/// );
///
/// // Create from usize (panics if 0)
/// let id = ElementId::new(1);
///
/// // Safe creation that returns Option
/// let maybe_id = ElementId::new_checked(0); // None
/// let valid_id = ElementId::new_checked(1); // Some(ElementId)
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct ElementId(std::num::NonZeroUsize);

impl ElementId {
    /// Create a new ElementId from a non-zero usize.
    ///
    /// # Panics
    ///
    /// Panics if `id` is 0.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_core::ElementId;
    ///
    /// let id = ElementId::new(1);
    /// assert_eq!(id.get(), 1);
    /// ```
    #[inline]
    pub fn new(id: usize) -> Self {
        Self(std::num::NonZeroUsize::new(id).expect("ElementId cannot be 0"))
    }

    /// Create a new ElementId from a usize, returning None if 0.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_core::ElementId;
    ///
    /// assert_eq!(ElementId::new_checked(0), None);
    /// assert_eq!(ElementId::new_checked(1).map(|id| id.get()), Some(1));
    /// ```
    #[inline]
    pub const fn new_checked(id: usize) -> Option<Self> {
        match std::num::NonZeroUsize::new(id) {
            Some(nz) => Some(Self(nz)),
            None => None,
        }
    }

    /// Get the inner usize value.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_core::ElementId;
    ///
    /// let id = ElementId::new(42);
    /// assert_eq!(id.get(), 42);
    /// ```
    #[inline]
    pub const fn get(self) -> usize {
        self.0.get()
    }

    /// Create an ElementId without checking if the value is non-zero.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `id` is not 0.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_core::ElementId;
    ///
    /// // Safe because 1 is non-zero
    /// let id = unsafe { ElementId::new_unchecked(1) };
    /// assert_eq!(id.get(), 1);
    /// ```
    #[inline]
    pub const unsafe fn new_unchecked(id: usize) -> Self {
        // SAFETY: Caller must ensure id is non-zero
        unsafe { Self(std::num::NonZeroUsize::new_unchecked(id)) }
    }
}

// Conversions for ergonomics
impl From<std::num::NonZeroUsize> for ElementId {
    #[inline]
    fn from(id: std::num::NonZeroUsize) -> Self {
        Self(id)
    }
}

impl From<ElementId> for usize {
    #[inline]
    fn from(id: ElementId) -> usize {
        id.get()
    }
}

// Display for debugging
impl std::fmt::Display for ElementId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ElementId({})", self.get())
    }
}

// Backward compatibility: Allow using ElementId as usize in tests
#[cfg(test)]
impl From<usize> for ElementId {
    fn from(id: usize) -> Self {
        Self::new(id)
    }
}

// Arithmetic operations (for bitmap indexing in dirty tracking)
impl std::ops::Sub<usize> for ElementId {
    type Output = usize;

    #[inline]
    fn sub(self, rhs: usize) -> usize {
        self.get() - rhs
    }
}

impl std::ops::Add<usize> for ElementId {
    type Output = ElementId;

    #[inline]
    fn add(self, rhs: usize) -> ElementId {
        ElementId::new(self.get() + rhs)
    }
}













