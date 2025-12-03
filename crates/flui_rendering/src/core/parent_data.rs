//! ParentData: per-child layout metadata with Flutter compliance.
//!
//! This module implements Flutter's ParentData system, enabling parents to attach
//! layout-specific metadata directly to their children without external side maps.
//!
//! # Flutter ParentData System
//!
//! In Flutter, every `RenderObject` can have a `ParentData` object that is set by
//! its parent during `setupParentData()`. This data typically contains:
//! - Layout information (offset, flex factor, etc.)
//! - Structural information (sibling links for traversal)
//! - Cached values (for performance optimization)
//!
//! # Design Goals
//!
//! - **Zero-cost access**: Plain structs, no virtual lookup beyond trait object use
//! - **Type-safe downcasting**: Via `as_any()` / `as_any_mut()`
//! - **Optional capabilities**: Composed via traits (offset caching, sibling links)
//! - **Memory locality**: Data stored inline with child element
//!
//! # Architecture
//!
//! ```text
//! ParentData (base trait)
//!     ↓
//!     ├─ ParentDataWithOffset (trait for positioned children)
//!     │   ↓
//!     │   └─ BoxParentData (simple offset storage)
//!     │
//!     └─ ContainerParentData (trait for linked lists)
//!         ↓
//!         └─ ContainerBoxParentData (offset + sibling links)
//! ```
//!
//! # Common ParentData Types
//!
//! | Type | Purpose | Flutter Equivalent |
//! |------|---------|-------------------|
//! | `BoxParentData` | Simple offset | `BoxParentData` |
//! | `ContainerBoxParentData` | Offset + siblings | `ContainerBoxParentData` |
//! | `ContainerParentData` | Just siblings | (not in Flutter) |
//! | `()` (unit) | No parent data | `null` |
//!
//! # Examples
//!
//! ## Basic Usage
//!
//! ```rust
//! use flui_rendering::core::{BoxParentData, ParentData, ParentDataWithOffset};
//! use flui_types::Offset;
//!
//! // Create parent data with offset
//! let mut data = BoxParentData::with_offset(Offset::new(10.0, 20.0));
//!
//! // Access offset
//! assert_eq!(data.offset(), Offset::new(10.0, 20.0));
//!
//! // Update offset
//! data.set_offset(Offset::new(15.0, 25.0));
//!
//! // Type-erase
//! let dyn_data: Box<dyn ParentData> = Box::new(data);
//!
//! // Downcast back
//! if let Some(box_data) = dyn_data.as_any().downcast_ref::<BoxParentData>() {
//!     println!("Offset: {:?}", box_data.offset());
//! }
//! ```
//!
//! ## Container with Siblings
//!
//! ```rust,ignore
//! use flui_rendering::core::ContainerBoxParentData;
//! use flui_foundation::ElementId;
//!
//! let mut data = ContainerBoxParentData::<ElementId>::new();
//!
//! // Set positioning
//! data.set_offset(Offset::new(10.0, 20.0));
//!
//! // Set up sibling links
//! data.set_previous_sibling(Some(ElementId::from(1)));
//! data.set_next_sibling(Some(ElementId::from(3)));
//!
//! // Traverse siblings
//! if let Some(next_id) = data.next_sibling() {
//!     // Process next sibling
//! }
//! ```
//!
//! ## Custom ParentData
//!
//! ```rust,ignore
//! #[derive(Debug, Clone)]
//! struct FlexParentData {
//!     flex: i32,
//!     fit: FlexFit,
//!     offset: Offset,
//! }
//!
//! impl ParentData for FlexParentData {
//!     // as_any() and as_any_mut() provided automatically
//! }
//!
//! impl ParentDataWithOffset for FlexParentData {
//!     fn offset(&self) -> Offset {
//!         self.offset
//!     }
//!
//!     fn set_offset(&mut self, offset: Offset) {
//!         self.offset = offset;
//!     }
//! }
//! ```
//!
//! # Thread Safety
//!
//! All ParentData types must be `Send + Sync` to enable:
//! - Concurrent layout computation
//! - Parallel child processing
//! - Cross-thread element manipulation
//!
//! # Performance
//!
//! - **Inline storage**: ParentData stored directly with child element
//! - **Cache-friendly**: No pointer chasing for common operations
//! - **Zero-cost**: Trait object overhead only when needed
//! - **Type-safe**: No runtime type checks in hot paths

use std::any::Any;
use std::fmt;

use flui_types::Offset;

// ============================================================================
// SEALED HELPER TRAIT
// ============================================================================

mod sealed {
    use super::*;

    /// Internal sealed helper that supplies `as_any_parent_data()` for all
    /// `ParentData` implementors.
    ///
    /// This trait is not exposed publicly and prevents external blanket
    /// implementations while providing automatic downcasting for all ParentData.
    pub trait AsAnyParentData: fmt::Debug + Send + Sync + 'static {
        /// Returns immutable type-erased view for downcasting.
        fn as_any_parent_data(&self) -> &dyn Any;

        /// Returns mutable type-erased view for downcasting.
        fn as_any_parent_data_mut(&mut self) -> &mut dyn Any;
    }

    /// Blanket implementation for all suitable `'static` types.
    ///
    /// Any type that is `Debug + Send + Sync + 'static` automatically
    /// gets downcasting capabilities.
    impl<T> AsAnyParentData for T
    where
        T: fmt::Debug + Send + Sync + 'static,
    {
        fn as_any_parent_data(&self) -> &dyn Any {
            self
        }

        fn as_any_parent_data_mut(&mut self) -> &mut dyn Any {
            self
        }
    }
}

// ============================================================================
// MAIN PARENTDATA TRAIT
// ============================================================================

/// ParentData - metadata that a parent RenderObject attaches to child elements.
///
/// This trait enables parents to store layout-specific information about each
/// child without maintaining separate data structures. The data is attached
/// during `setupParentData()` (called when a child is adopted) and accessed
/// during layout, paint, and hit testing.
///
/// # Flutter Equivalent
///
/// ```dart
/// abstract class ParentData {
///   void detach() { }
///   @override
///   String toString() => '<none>';
/// }
/// ```
///
/// # Design Philosophy
///
/// - **Owned by parent**: The parent creates and manages the ParentData
/// - **Type-specific**: Each parent type has its own ParentData type
/// - **Mutable**: Can be modified during layout
/// - **Cached**: Stores computed values for later phases
///
/// # Automatic Downcasting
///
/// The `as_any()` and `as_any_mut()` methods are provided automatically via
/// a sealed helper trait. You don't need to implement them manually.
///
/// # Thread Safety
///
/// All ParentData implementations must be `Send + Sync` to enable concurrent
/// rendering operations across threads.
///
/// # Examples
///
/// ## Implementing Custom ParentData
///
/// ```rust,ignore
/// #[derive(Debug, Clone)]
/// struct StackParentData {
///     offset: Offset,
///     left: Option<f32>,
///     top: Option<f32>,
///     right: Option<f32>,
///     bottom: Option<f32>,
///     width: Option<f32>,
///     height: Option<f32>,
/// }
///
/// impl ParentData for StackParentData {
///     // as_any() provided automatically!
/// }
///
/// impl ParentDataWithOffset for StackParentData {
///     fn offset(&self) -> Offset {
///         self.offset
///     }
///
///     fn set_offset(&mut self, offset: Offset) {
///         self.offset = offset;
///     }
/// }
/// ```
///
/// ## Using ParentData in Layout
///
/// ```rust,ignore
/// fn layout(&mut self, ctx: BoxLayoutContext) -> RenderResult<Size> {
///     for child_id in ctx.children() {
///         // Get child's parent data
///         if let Some(parent_data) = ctx.tree().get_parent_data(child_id) {
///             if let Some(flex_data) = parent_data.as_any().downcast_ref::<FlexParentData>() {
///                 // Use flex factor in layout calculation
///                 let constraints = compute_child_constraints(flex_data.flex);
///                 let size = ctx.layout_child(child_id, constraints)?;
///             }
///         }
///     }
///     Ok(total_size)
/// }
/// ```
pub trait ParentData: sealed::AsAnyParentData {
    /// Returns immutable type-erased access for downcasting.
    ///
    /// This method is provided automatically and enables safe downcasting
    /// from `&dyn ParentData` to concrete types.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let parent_data: &dyn ParentData = get_parent_data();
    ///
    /// if let Some(box_data) = parent_data.as_any().downcast_ref::<BoxParentData>() {
    ///     println!("Offset: {:?}", box_data.offset());
    /// }
    /// ```
    fn as_any(&self) -> &dyn Any {
        self.as_any_parent_data()
    }

    /// Returns mutable type-erased access for downcasting.
    ///
    /// This method is provided automatically and enables safe mutable
    /// downcasting from `&mut dyn ParentData` to concrete types.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let parent_data: &mut dyn ParentData = get_parent_data_mut();
    ///
    /// if let Some(box_data) = parent_data.as_any_mut().downcast_mut::<BoxParentData>() {
    ///     box_data.set_offset(Offset::new(10.0, 20.0));
    /// }
    /// ```
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self.as_any_parent_data_mut()
    }

    /// Returns offset capability if this ParentData supports it.
    ///
    /// Returns `Some(&dyn ParentDataWithOffset)` if this type implements
    /// `ParentDataWithOffset`, `None` otherwise.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let parent_data: &dyn ParentData = get_parent_data();
    ///
    /// if let Some(offset_data) = parent_data.as_parent_data_with_offset() {
    ///     let offset = offset_data.offset();
    ///     // Use offset for painting or hit testing
    /// }
    /// ```
    fn as_parent_data_with_offset(&self) -> Option<&dyn ParentDataWithOffset> {
        None
    }

    /// Returns mutable offset capability if this ParentData supports it.
    ///
    /// Returns `Some(&mut dyn ParentDataWithOffset)` if this type implements
    /// `ParentDataWithOffset`, `None` otherwise.
    fn as_parent_data_with_offset_mut(&mut self) -> Option<&mut dyn ParentDataWithOffset> {
        None
    }

    /// Detaches this ParentData (cleanup hook).
    ///
    /// Called when a child is removed from its parent. Override to perform
    /// cleanup, such as breaking circular references or resetting state.
    ///
    /// # Flutter Equivalent
    ///
    /// ```dart
    /// void detach() { }
    /// ```
    ///
    /// # Default Implementation
    ///
    /// Does nothing. Override if cleanup is needed.
    fn detach(&mut self) {
        // Default: no-op
    }
}

// ============================================================================
// PARENTDATA WITH OFFSET TRAIT
// ============================================================================

/// ParentData with cached offset for efficient hit testing and painting.
///
/// This trait is implemented by ParentData types that cache the child's offset
/// (calculated during layout). This avoids recalculating positions during
/// painting and hit testing, which are called more frequently than layout.
///
/// # Flutter Equivalent
///
/// Flutter's BoxParentData:
/// ```dart
/// class BoxParentData extends ParentData {
///   Offset offset = Offset.zero;
/// }
/// ```
///
/// # Performance Benefits
///
/// - **No recalculation**: Offset computed once during layout, reused many times
/// - **Cache-friendly**: Offset stored inline with ParentData
/// - **Fast access**: Direct field access, no method call overhead
///
/// # Common Implementations
///
/// - `BoxParentData`: Simple offset storage
/// - `ContainerBoxParentData`: Offset + sibling links
/// - `StackParentData`: Offset + positioning constraints
/// - Custom layout-specific ParentData types
///
/// # Examples
///
/// ## Using in Paint
///
/// ```rust,ignore
/// fn paint(&self, ctx: &mut BoxPaintContext) {
///     for child_id in ctx.children() {
///         // Get cached offset from ParentData
///         let child_offset = if let Some(parent_data) = ctx.tree().get_parent_data(child_id) {
///             if let Some(data_with_offset) = parent_data.as_parent_data_with_offset() {
///                 data_with_offset.offset()
///             } else {
///                 Offset::ZERO
///             }
///         } else {
///             Offset::ZERO
///         };
///
///         // Paint child at cached offset
///         ctx.paint_child(child_id, ctx.offset + child_offset);
///     }
/// }
/// ```
///
/// ## Using in Hit Test
///
/// ```rust,ignore
/// fn hit_test(&self, ctx: &mut BoxHitTestContext) -> bool {
///     // Test children in reverse order (front to back)
///     for child_id in ctx.children().rev() {
///         let child_offset = ctx.tree()
///             .get_parent_data(child_id)
///             .and_then(|pd| pd.as_parent_data_with_offset())
///             .map(|pd| pd.offset())
///             .unwrap_or(Offset::ZERO);
///
///         let child_position = ctx.position - child_offset;
///         if ctx.hit_test_child(child_id, child_position) {
///             return true;
///         }
///     }
///     false
/// }
/// ```
pub trait ParentDataWithOffset: ParentData {
    /// Returns the cached layout offset in parent-local coordinates.
    ///
    /// The offset is set during the parent's layout phase and used during
    /// paint and hit test phases.
    ///
    /// # Coordinate System
    ///
    /// - Origin is at the parent's top-left corner
    /// - Positive x moves right, positive y moves down
    /// - Offset is in logical pixels (device-independent)
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_rendering::core::{BoxParentData, ParentDataWithOffset};
    /// use flui_types::Offset;
    ///
    /// let data = BoxParentData::with_offset(Offset::new(10.0, 20.0));
    /// assert_eq!(data.offset(), Offset::new(10.0, 20.0));
    /// ```
    fn offset(&self) -> Offset;

    /// Sets the cached layout offset.
    ///
    /// This is called during the parent's layout phase to cache the child's
    /// position for later use in paint and hit test.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_rendering::core::{BoxParentData, ParentDataWithOffset};
    /// use flui_types::Offset;
    ///
    /// let mut data = BoxParentData::new();
    /// data.set_offset(Offset::new(15.0, 25.0));
    /// assert_eq!(data.offset(), Offset::new(15.0, 25.0));
    /// ```
    fn set_offset(&mut self, offset: Offset);

    /// Translates the offset by a delta.
    ///
    /// Convenience method for moving the child by a relative amount.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_rendering::core::{BoxParentData, ParentDataWithOffset};
    /// use flui_types::Offset;
    ///
    /// let mut data = BoxParentData::with_offset(Offset::new(10.0, 20.0));
    /// data.translate_offset(Offset::new(5.0, 10.0));
    /// assert_eq!(data.offset(), Offset::new(15.0, 30.0));
    /// ```
    fn translate_offset(&mut self, delta: Offset) {
        let current = self.offset();
        self.set_offset(current + delta);
    }

    /// Checks if the child is at the origin (0, 0).
    ///
    /// Useful for optimization - children at origin don't need translation.
    fn is_at_origin(&self) -> bool {
        self.offset() == Offset::ZERO
    }
}

// ============================================================================
// UNIT TYPE IMPLEMENTATION (NO PARENT DATA)
// ============================================================================

/// Implement ParentData for () (unit type) to represent "no parent data".
///
/// This allows RenderObjects that don't need parent data to use simple APIs
/// without requiring a dedicated NoParentData type.
///
/// # Examples
///
/// ```rust,ignore
/// // For renders that don't use parent data
/// type MyParentData = ();
///
/// impl RenderBox for MyLeafRender {
///     type ParentData = ();
///     // ...
/// }
/// ```
impl ParentData for () {}

// ============================================================================
// BOX PARENT DATA
// ============================================================================

/// Box parent data - stores offset for positioned children.
///
/// The fundamental ParentData type for box-based layouts. Stores the offset
/// at which a child should be painted relative to the parent's origin.
///
/// # Flutter Equivalent
///
/// ```dart
/// class BoxParentData extends ParentData {
///   /// The offset at which to paint the child in the parent's coordinate system.
///   Offset offset = Offset.zero;
/// }
/// ```
///
/// # Use Cases
///
/// - Simple positioned layouts (Stack, Positioned)
/// - Single-child containers (Padding, Align, Center)
/// - Any render object that needs to position children
///
/// # Memory Layout
///
/// ```text
/// BoxParentData: 8 bytes
///   offset: Offset (8 bytes = 2 x f32)
/// ```
///
/// # Examples
///
/// ## Creating BoxParentData
///
/// ```rust
/// use flui_rendering::core::BoxParentData;
/// use flui_types::Offset;
///
/// // At origin
/// let data = BoxParentData::new();
/// assert_eq!(data.offset(), Offset::ZERO);
///
/// // With specific offset
/// let data = BoxParentData::with_offset(Offset::new(10.0, 20.0));
/// assert_eq!(data.offset(), Offset::new(10.0, 20.0));
///
/// // With x and y
/// let data = BoxParentData::with_xy(15.0, 25.0);
/// assert_eq!(data.offset(), Offset::new(15.0, 25.0));
/// ```
///
/// ## Modifying Offset
///
/// ```rust
/// use flui_rendering::core::{BoxParentData, ParentDataWithOffset};
/// use flui_types::Offset;
///
/// let mut data = BoxParentData::new();
///
/// // Set offset
/// data.set_offset(Offset::new(10.0, 20.0));
///
/// // Set via x and y
/// data.set_xy(15.0, 25.0);
///
/// // Translate by delta
/// data.translate(Offset::new(5.0, 10.0));
/// assert_eq!(data.offset(), Offset::new(20.0, 35.0));
///
/// // Reset to origin
/// data.reset();
/// assert!(data.is_at_origin());
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxParentData {
    /// Offset from the parent's origin where this child should be painted.
    ///
    /// Set during layout, used during paint and hit test.
    offset: Offset,
}

impl Default for BoxParentData {
    fn default() -> Self {
        Self::new()
    }
}

impl BoxParentData {
    /// Creates a new BoxParentData at the origin (0, 0).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_rendering::core::BoxParentData;
    /// use flui_types::Offset;
    ///
    /// let data = BoxParentData::new();
    /// assert_eq!(data.offset(), Offset::ZERO);
    /// ```
    pub const fn new() -> Self {
        Self {
            offset: Offset::ZERO,
        }
    }

    /// Creates BoxParentData with a specific offset.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_rendering::core::BoxParentData;
    /// use flui_types::Offset;
    ///
    /// let data = BoxParentData::with_offset(Offset::new(10.0, 20.0));
    /// assert_eq!(data.offset(), Offset::new(10.0, 20.0));
    /// ```
    pub const fn with_offset(offset: Offset) -> Self {
        Self { offset }
    }

    /// Creates BoxParentData with x and y coordinates.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_rendering::core::BoxParentData;
    /// use flui_types::Offset;
    ///
    /// let data = BoxParentData::with_xy(15.0, 25.0);
    /// assert_eq!(data.offset(), Offset::new(15.0, 25.0));
    /// ```
    pub fn with_xy(x: f32, y: f32) -> Self {
        Self {
            offset: Offset::new(x, y),
        }
    }

    /// Sets the offset using x and y coordinates.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_rendering::core::BoxParentData;
    /// use flui_types::Offset;
    ///
    /// let mut data = BoxParentData::new();
    /// data.set_xy(30.0, 40.0);
    /// assert_eq!(data.offset(), Offset::new(30.0, 40.0));
    /// ```
    pub fn set_xy(&mut self, x: f32, y: f32) {
        self.offset = Offset::new(x, y);
    }

    /// Moves the offset by a delta.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_rendering::core::BoxParentData;
    /// use flui_types::Offset;
    ///
    /// let mut data = BoxParentData::with_xy(10.0, 20.0);
    /// data.translate(Offset::new(5.0, 10.0));
    /// assert_eq!(data.offset(), Offset::new(15.0, 30.0));
    /// ```
    pub fn translate(&mut self, delta: Offset) {
        self.offset = self.offset + delta;
    }

    /// Resets the offset to the origin (0, 0).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_rendering::core::BoxParentData;
    /// use flui_types::Offset;
    ///
    /// let mut data = BoxParentData::with_xy(100.0, 200.0);
    /// data.reset();
    /// assert_eq!(data.offset(), Offset::ZERO);
    /// ```
    pub fn reset(&mut self) {
        self.offset = Offset::ZERO;
    }

    /// Checks if this child is at the origin (0, 0).
    ///
    /// Useful for optimization - children at origin don't need translation
    /// during paint.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_rendering::core::BoxParentData;
    /// use flui_types::Offset;
    ///
    /// let data = BoxParentData::new();
    /// assert!(data.is_at_origin());
    ///
    /// let data = BoxParentData::with_xy(10.0, 20.0);
    /// assert!(!data.is_at_origin());
    /// ```
    pub fn is_at_origin(&self) -> bool {
        self.offset == Offset::ZERO
    }
}

impl ParentData for BoxParentData {
    fn as_parent_data_with_offset(&self) -> Option<&dyn ParentDataWithOffset> {
        Some(self)
    }

    fn as_parent_data_with_offset_mut(&mut self) -> Option<&mut dyn ParentDataWithOffset> {
        Some(self)
    }
}

impl ParentDataWithOffset for BoxParentData {
    #[inline]
    fn offset(&self) -> Offset {
        self.offset
    }

    #[inline]
    fn set_offset(&mut self, offset: Offset) {
        self.offset = offset;
    }
}

// ============================================================================
// CONTAINER PARENT DATA
// ============================================================================

/// Container parent data - sibling links for efficient traversal.
///
/// Provides doubly-linked list functionality for maintaining sibling relationships.
/// Used by container RenderObjects that need to traverse their children
/// efficiently in both directions.
///
/// # Flutter Equivalent
///
/// ```dart
/// class ContainerParentDataMixin<ChildType> {
///   ChildType? previousSibling;
///   ChildType? nextSibling;
/// }
/// ```
///
/// # Use Cases
///
/// - Multi-child containers (Row, Column, Wrap)
/// - Efficient forward/backward traversal
/// - Finding first/last child
/// - Removing children without full scan
///
/// # Type Parameters
///
/// - `ChildId`: The type used to identify children (typically `ElementId`)
///
/// # Memory Layout
///
/// ```text
/// ContainerParentData<ElementId>: 16 bytes (on 64-bit)
///   previous_sibling: Option<ElementId> (8 bytes)
///   next_sibling: Option<ElementId> (8 bytes)
/// ```
///
/// # Examples
///
/// ## Basic Usage
///
/// ```rust,ignore
/// use flui_rendering::core::ContainerParentData;
/// use flui_foundation::ElementId;
///
/// let mut data = ContainerParentData::<ElementId>::new();
///
/// // Set up sibling links
/// data.set_previous_sibling(Some(ElementId::from(1)));
/// data.set_next_sibling(Some(ElementId::from(3)));
///
/// // Check position
/// assert!(!data.is_first());
/// assert!(!data.is_last());
/// assert!(!data.is_only());
///
/// // Traverse siblings
/// if let Some(next_id) = data.next_sibling {
///     // Process next sibling
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerParentData<ChildId> {
    /// Previous sibling in the parent's child list.
    ///
    /// `None` if this is the first child.
    pub previous_sibling: Option<ChildId>,

    /// Next sibling in the parent's child list.
    ///
    /// `None` if this is the last child.
    pub next_sibling: Option<ChildId>,
}

impl<ChildId> Default for ContainerParentData<ChildId> {
    fn default() -> Self {
        Self::new()
    }
}

impl<ChildId> ContainerParentData<ChildId> {
    /// Creates new container parent data with no siblings.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use flui_rendering::core::ContainerParentData;
    /// use flui_foundation::ElementId;
    ///
    /// let data = ContainerParentData::<ElementId>::new();
    /// assert!(data.is_only());
    /// ```
    pub const fn new() -> Self {
        Self {
            previous_sibling: None,
            next_sibling: None,
        }
    }

    /// Creates container parent data with specific siblings.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use flui_rendering::core::ContainerParentData;
    /// use flui_foundation::ElementId;
    ///
    /// let data = ContainerParentData::with_siblings(
    ///     Some(ElementId::from(1)),
    ///     Some(ElementId::from(3)),
    /// );
    /// assert!(!data.is_first());
    /// assert!(!data.is_last());
    /// ```
    pub fn with_siblings(previous: Option<ChildId>, next: Option<ChildId>) -> Self {
        Self {
            previous_sibling: previous,
            next_sibling: next,
        }
    }

    /// Sets the previous sibling.
    pub fn set_previous_sibling(&mut self, sibling: Option<ChildId>) {
        self.previous_sibling = sibling;
    }

    /// Sets the next sibling.
    pub fn set_next_sibling(&mut self, sibling: Option<ChildId>) {
        self.next_sibling = sibling;
    }

    /// Clears both sibling links.
    ///
    /// After this call, the child appears to be an only child.
    pub fn clear_siblings(&mut self) {
        self.previous_sibling = None;
        self.next_sibling = None;
    }

    /// Checks if this is the first child (no previous sibling).
    #[inline]
    pub fn is_first(&self) -> bool {
        self.previous_sibling.is_none()
    }

    /// Checks if this is the last child (no next sibling).
    #[inline]
    pub fn is_last(&self) -> bool {
        self.next_sibling.is_none()
    }

    /// Checks if this is the only child (no siblings).
    #[inline]
    pub fn is_only(&self) -> bool {
        self.is_first() && self.is_last()
    }

    /// Returns true if this child has any siblings.
    #[inline]
    pub fn has_siblings(&self) -> bool {
        !self.is_only()
    }
}

// ============================================================================
// CONTAINER BOX PARENT DATA
// ============================================================================

/// Container box parent data - combines offset and sibling links.
///
/// The most commonly used ParentData type, combining both:
/// - Positioning information (from `BoxParentData`)
/// - Sibling links (from `ContainerParentData`)
///
/// Used by multi-child RenderObjects like Row, Column, Flex, Wrap, etc.
///
/// # Flutter Equivalent
///
/// ```dart
/// class ContainerBoxParentData<ChildType>
///     extends BoxParentData
///     with ContainerParentDataMixin<ChildType> {
/// }
/// ```
///
/// # Type Parameters
///
/// - `ChildId`: The type used to identify children (typically `ElementId`)
///
/// # Memory Layout
///
/// ```text
/// ContainerBoxParentData<ElementId>: 24 bytes (on 64-bit)
///   offset: Offset (8 bytes)
///   previous_sibling: Option<ElementId> (8 bytes)
///   next_sibling: Option<ElementId> (8 bytes)
/// ```
///
/// # Examples
///
/// ## Full Usage
///
/// ```rust,ignore
/// use flui_rendering::core::{ContainerBoxParentData, ParentDataWithOffset};
/// use flui_foundation::ElementId;
/// use flui_types::Offset;
///
/// let mut data = ContainerBoxParentData::<ElementId>::new();
///
/// // Set positioning
/// data.set_offset(Offset::new(10.0, 20.0));
/// assert_eq!(data.offset(), Offset::new(10.0, 20.0));
///
/// // Set up sibling links
/// data.set_previous_sibling(Some(ElementId::from(1)));
/// data.set_next_sibling(Some(ElementId::from(3)));
///
/// // Check state
/// assert!(!data.is_first());
/// assert!(!data.is_last());
/// assert!(!data.is_at_origin());
///
/// // Traverse siblings
/// while let Some(next_id) = data.next_sibling() {
///     // Process next sibling
///     break; // Prevent infinite loop in example
/// }
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct ContainerBoxParentData<ChildId> {
    /// Box parent data (offset).
    box_data: BoxParentData,

    /// Container parent data (siblings).
    container_data: ContainerParentData<ChildId>,
}

impl<ChildId> Default for ContainerBoxParentData<ChildId> {
    fn default() -> Self {
        Self::new()
    }
}

impl<ChildId> ContainerBoxParentData<ChildId> {
    /// Creates a new ContainerBoxParentData at origin with no siblings.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use flui_rendering::core::ContainerBoxParentData;
    /// use flui_foundation::ElementId;
    ///
    /// let data = ContainerBoxParentData::<ElementId>::new();
    /// assert!(data.is_at_origin());
    /// assert!(data.is_only());
    /// ```
    pub fn new() -> Self {
        Self {
            box_data: BoxParentData::new(),
            container_data: ContainerParentData::new(),
        }
    }

    /// Creates container box parent data with a specific offset.
    pub fn with_offset(offset: Offset) -> Self {
        Self {
            box_data: BoxParentData::with_offset(offset),
            container_data: ContainerParentData::new(),
        }
    }

    /// Creates container box parent data with offset and siblings.
    pub fn with_offset_and_siblings(
        offset: Offset,
        previous: Option<ChildId>,
        next: Option<ChildId>,
    ) -> Self {
        Self {
            box_data: BoxParentData::with_offset(offset),
            container_data: ContainerParentData::with_siblings(previous, next),
        }
    }

    // === Offset Methods (from BoxParentData) ===

    /// Gets the offset.
    #[inline]
    pub fn offset(&self) -> Offset {
        self.box_data.offset
    }

    /// Sets the offset.
    #[inline]
    pub fn set_offset(&mut self, offset: Offset) {
        self.box_data.set_offset(offset);
    }

    /// Sets the offset using x and y coordinates.
    pub fn set_xy(&mut self, x: f32, y: f32) {
        self.box_data.set_xy(x, y);
    }

    /// Moves the offset by a delta.
    pub fn translate(&mut self, delta: Offset) {
        self.box_data.translate(delta);
    }

    /// Resets the offset to the origin.
    pub fn reset_offset(&mut self) {
        self.box_data.reset();
    }

    /// Checks if this child is at the origin.
    #[inline]
    pub fn is_at_origin(&self) -> bool {
        self.box_data.is_at_origin()
    }

    // === Sibling Methods (from ContainerParentData) ===

    /// Gets the previous sibling.
    #[inline]
    pub fn previous_sibling(&self) -> Option<&ChildId> {
        self.container_data.previous_sibling.as_ref()
    }

    /// Gets the next sibling.
    #[inline]
    pub fn next_sibling(&self) -> Option<&ChildId> {
        self.container_data.next_sibling.as_ref()
    }

    /// Sets the previous sibling.
    pub fn set_previous_sibling(&mut self, sibling: Option<ChildId>) {
        self.container_data.set_previous_sibling(sibling);
    }

    /// Sets the next sibling.
    pub fn set_next_sibling(&mut self, sibling: Option<ChildId>) {
        self.container_data.set_next_sibling(sibling);
    }

    /// Clears both sibling links.
    pub fn clear_siblings(&mut self) {
        self.container_data.clear_siblings();
    }

    /// Checks if this is the first child.
    #[inline]
    pub fn is_first(&self) -> bool {
        self.container_data.is_first()
    }

    /// Checks if this is the last child.
    #[inline]
    pub fn is_last(&self) -> bool {
        self.container_data.is_last()
    }

    /// Checks if this is the only child.
    #[inline]
    pub fn is_only(&self) -> bool {
        self.container_data.is_only()
    }

    /// Checks if this child has any siblings.
    #[inline]
    pub fn has_siblings(&self) -> bool {
        self.container_data.has_siblings()
    }
}

impl<ChildId> ParentData for ContainerBoxParentData<ChildId>
where
    ChildId: fmt::Debug + Send + Sync + 'static,
{
    fn as_parent_data_with_offset(&self) -> Option<&dyn ParentDataWithOffset> {
        Some(self)
    }

    fn as_parent_data_with_offset_mut(&mut self) -> Option<&mut dyn ParentDataWithOffset> {
        Some(self)
    }
}

impl<ChildId> ParentDataWithOffset for ContainerBoxParentData<ChildId>
where
    ChildId: fmt::Debug + Send + Sync + 'static,
{
    #[inline]
    fn offset(&self) -> Offset {
        self.box_data.offset
    }

    #[inline]
    fn set_offset(&mut self, offset: Offset) {
        self.box_data.offset = offset;
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_box_parent_data_new() {
        let data = BoxParentData::new();
        assert_eq!(data.offset(), Offset::ZERO);
        assert!(data.is_at_origin());
    }

    #[test]
    fn test_box_parent_data_with_offset() {
        let offset = Offset::new(10.0, 20.0);
        let data = BoxParentData::with_offset(offset);
        assert_eq!(data.offset(), offset);
        assert!(!data.is_at_origin());
    }

    #[test]
    fn test_box_parent_data_with_xy() {
        let data = BoxParentData::with_xy(15.0, 25.0);
        assert_eq!(data.offset(), Offset::new(15.0, 25.0));
    }

    #[test]
    fn test_box_parent_data_set_offset() {
        let mut data = BoxParentData::new();
        let offset = Offset::new(5.0, 15.0);
        data.set_offset(offset);
        assert_eq!(data.offset(), offset);
    }

    #[test]
    fn test_box_parent_data_translate() {
        let mut data = BoxParentData::with_xy(10.0, 20.0);
        data.translate(Offset::new(5.0, 10.0));
        assert_eq!(data.offset(), Offset::new(15.0, 30.0));
    }

    #[test]
    fn test_box_parent_data_reset() {
        let mut data = BoxParentData::with_xy(100.0, 200.0);
        data.reset();
        assert_eq!(data.offset(), Offset::ZERO);
        assert!(data.is_at_origin());
    }

    #[test]
    fn test_box_parent_data_downcast() {
        let data = BoxParentData::new();
        let boxed: Box<dyn ParentData> = Box::new(data);

        assert!(boxed.as_any().is::<BoxParentData>());
        let downcast = boxed.as_any().downcast_ref::<BoxParentData>().unwrap();
        assert_eq!(downcast.offset(), Offset::ZERO);
    }

    #[test]
    fn test_box_parent_data_with_offset_trait() {
        let data = BoxParentData::with_xy(10.0, 20.0);
        let boxed: Box<dyn ParentData> = Box::new(data);

        if let Some(offset_data) = boxed.as_parent_data_with_offset() {
            assert_eq!(offset_data.offset(), Offset::new(10.0, 20.0));
        } else {
            panic!("Should have offset capability");
        }
    }

    #[test]
    fn test_container_parent_data_new() {
        let data: ContainerParentData<u64> = ContainerParentData::new();
        assert!(data.is_only());
        assert!(data.is_first());
        assert!(data.is_last());
        assert!(!data.has_siblings());
    }

    #[test]
    fn test_container_parent_data_with_siblings() {
        let data = ContainerParentData::with_siblings(Some(1u64), Some(2u64));
        assert_eq!(data.previous_sibling, Some(1));
        assert_eq!(data.next_sibling, Some(2));
        assert!(!data.is_first());
        assert!(!data.is_last());
        assert!(!data.is_only());
        assert!(data.has_siblings());
    }

    #[test]
    fn test_container_parent_data_clear() {
        let mut data = ContainerParentData::with_siblings(Some(1u64), Some(2u64));
        data.clear_siblings();
        assert!(data.is_only());
    }

    #[test]
    fn test_container_box_parent_data_new() {
        let data: ContainerBoxParentData<u64> = ContainerBoxParentData::new();
        assert_eq!(data.offset(), Offset::ZERO);
        assert!(data.is_only());
        assert!(data.is_at_origin());
    }

    #[test]
    fn test_container_box_parent_data_full() {
        let mut data = ContainerBoxParentData::new();
        data.set_offset(Offset::new(100.0, 200.0));
        data.set_previous_sibling(Some(10u64));
        data.set_next_sibling(Some(20u64));

        assert_eq!(data.offset(), Offset::new(100.0, 200.0));
        assert_eq!(data.previous_sibling(), Some(&10));
        assert_eq!(data.next_sibling(), Some(&20));
        assert!(!data.is_first());
        assert!(!data.is_last());
        assert!(!data.is_at_origin());
    }

    #[test]
    fn test_container_box_translate() {
        let mut data = ContainerBoxParentData::with_offset(Offset::new(10.0, 20.0));
        data.translate(Offset::new(5.0, 10.0));
        assert_eq!(data.offset(), Offset::new(15.0, 30.0));
    }

    #[test]
    fn test_unit_parent_data() {
        let data = ();
        let boxed: Box<dyn ParentData> = Box::new(data);
        assert!(boxed.as_any().is::<()>());
    }

    #[test]
    fn test_parent_data_with_offset_translate() {
        let mut data = BoxParentData::with_xy(10.0, 20.0);
        data.translate_offset(Offset::new(5.0, 10.0));
        assert_eq!(data.offset(), Offset::new(15.0, 30.0));
    }

    #[test]
    fn test_size() {
        use std::mem::size_of;

        // BoxParentData should be 8 bytes (one Offset)
        assert_eq!(size_of::<BoxParentData>(), 8);

        // ContainerParentData<u64> should be 16 bytes (two Option<u64>)
        assert_eq!(size_of::<ContainerParentData<u64>>(), 16);

        // ContainerBoxParentData<u64> should be 24 bytes (8 + 16)
        assert_eq!(size_of::<ContainerBoxParentData<u64>>(), 24);
    }
}
