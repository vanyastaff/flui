//! ParentData: per-child layout metadata set by the parent render node.
//!
//! This subsystem enables parents to attach layout / traversal data directly to
//! their children without external side maps, keeping memory access locality high.
//!
//! # Goals
//! - Zero-cost access (plain structs, no virtual lookup beyond trait object use)
//! - Type-safe downcasting (`as_any()` / `as_any_mut()`)
//! - Optional capabilities (offset caching, sibling links) composed via traits
//!
//! # Core Traits
//! - `ParentData`: base marker + downcasting surface
//! - `ParentDataWithOffset`: opt-in cached child offset (for paint and hit test)
//!
//! # Common Types
//! - `BoxParentData`: just an offset
//! - `ContainerParentData<ChildId>`: sibling links
//! - `ContainerBoxParentData<ChildId>`: offset + sibling links
//!
//! # Example
//! ```rust,ignore
//! let data = BoxParentData::with_offset(Offset::new(8.0, 16.0));
//! let dyn_data: Box<dyn ParentData> = Box::new(data);
//! if let Some(offset_cap) = dyn_data.as_parent_data_with_offset() {
//!     assert_eq!(offset_cap.offset(), Offset::new(8.0,16.0));
//! }
//! ```

use std::any::Any;
use std::fmt;

use flui_types::layout::FlexFit;
use flui_types::Offset;

// ============================================================================
// Sealed helper trait for auto as_any() implementation
// ============================================================================

mod sealed {
    use super::*;

    /// Internal sealed helper that supplies `as_any_parent_data()` for all `ParentData`
    /// implementors. Not exposed publicly; prevents external blanket impls.
    pub trait AsAnyParentData: fmt::Debug + Send + Sync + 'static {
        /// Immutable type-erased view.
        fn as_any_parent_data(&self) -> &dyn Any;
        /// Mutable type-erased view.
        fn as_any_parent_data_mut(&mut self) -> &mut dyn Any;
    }

    /// Blanket implementation for all suitable `'static` types.
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
// Main ParentData trait
// ============================================================================

/// ParentData - metadata that a parent Render attaches to child elements
///
/// This trait enables parents to store layout-specific information about each child
/// without maintaining separate data structures. The trait provides type-safe
/// downcasting, allowing generic code to work with `dyn ParentData` while concrete
/// implementations access their specific data.
///
/// # Thread Safety
///
/// All ParentData implementations must be `Send + Sync` to enable concurrent
/// rendering operations across threads.
///
/// # Automatic Downcasting
///
/// The `as_any()` and `as_any_mut()` methods are provided automatically
/// through a helper trait. You don't need to implement them manually.
///
/// # Example Implementation
///
/// ```rust,ignore
/// #[derive(Debug, Clone)]
/// struct FlexParentData {
///     flex: i32,
///     fit: FlexFit,
/// }
///
/// impl ParentData for FlexParentData {
///     // No need to implement as_any() - it's automatic!
/// }
///
/// // Use in layout code:
/// fn layout_child(parent_data: &dyn ParentData) {
///     if let Some(flex_data) = parent_data.as_any().downcast_ref::<FlexParentData>() {
///         let flex_value = flex_data.flex;
///     }
/// }
/// ```
pub trait ParentData: sealed::AsAnyParentData {
    /// Immutable type-erased access for downcasting.
    fn as_any(&self) -> &dyn Any {
        self.as_any_parent_data()
    }
    /// Mutable type-erased access for downcasting.
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self.as_any_parent_data_mut()
    }
    /// Optional offset capability accessor (returns `Some` if this implements `ParentDataWithOffset`).
    fn as_parent_data_with_offset(&self) -> Option<&dyn ParentDataWithOffset> {
        None
    }
}

/// ParentData with cached offset for efficient hit testing and painting
///
/// This trait is implemented by ParentData types that cache the child's offset
/// (calculated during layout). This avoids recalculating positions during
/// painting and hit testing.
///
/// # Common Implementations
///
/// - `BoxParentData`: Simple offset storage
/// - `ContainerBoxParentData`: Offset + sibling links
/// - Custom layout-specific ParentData types
///
/// # Example
///
/// ```rust,ignore
/// fn hit_test_children(&self, result: &mut HitTestResult, position: Offset, ctx: &RenderContext) -> bool {
///     for &child_id in ctx.children().iter().rev() {
///         // Read cached offset from ParentData
///         let child_offset = if let Some(parent_data) = ctx.tree().parent_data(child_id) {
///             if let Some(data_with_offset) = parent_data.as_parent_data_with_offset() {
///                 data_with_offset.offset()
///             } else {
///                 Offset::ZERO
///             }
///         } else {
///             Offset::ZERO
///         };
///
///         let child_position = position - child_offset;
///         if ctx.hit_test_child(child_id, result, child_position) {
///             return true;
///         }
///     }
///     false
/// }
/// ```
pub trait ParentDataWithOffset: ParentData {
    /// Get cached layout offset (parent-local coordinates).
    fn offset(&self) -> Offset;
    /// Set cached layout offset.
    fn set_offset(&mut self, offset: Offset);
}

// Implement ParentData for () (unit type) to represent "no parent data"
//
// This allows Renders that don't need parent data to use simple APIs
// without requiring a dedicated NoParentData type.
impl ParentData for () {}

/// Box parent data - stores offset for positioned children
///
/// The fundamental ParentData type for box-based layouts. Stores the offset
/// at which a child should be painted relative to the parent's origin.
///
/// # Coordinate System
///
/// - Origin is in the parent's top-left corner
/// - Positive x moves right, positive y moves down
/// - Offset is applied during painting, not during layout
///
/// # Example
///
/// ```rust,ignore
/// let mut data = BoxParentData::with_offset(Offset::new(10.0, 20.0));
/// data.set_offset(Offset::new(15.0, 25.0));
///
/// // In paint code:
/// painter.translate(data.offset());
/// child.paint(painter);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxParentData {
    /// Offset from the parent's origin where this child should be painted
    offset: Offset,
}

impl Default for BoxParentData {
    fn default() -> Self {
        Self {
            offset: Offset::ZERO,
        }
    }
}

impl BoxParentData {
    /// Create a new box parent_data at the origin (0, 0)
    pub const fn new() -> Self {
        Self {
            offset: Offset::ZERO,
        }
    }

    /// Create box parent data with a specific offset
    pub const fn with_offset(offset: Offset) -> Self {
        Self { offset }
    }

    /// Create box parent data with x and y coordinates
    pub fn with_xy(x: f32, y: f32) -> Self {
        Self {
            offset: Offset::new(x, y),
        }
    }

    /// Set the offset
    pub fn set_offset(&mut self, offset: Offset) {
        self.offset = offset;
    }

    /// Set the offset using x and y coordinates
    pub fn set_xy(&mut self, x: f32, y: f32) {
        self.offset = Offset::new(x, y);
    }

    /// Move the offset by a delta
    pub fn translate(&mut self, delta: Offset) {
        self.offset = self.offset + delta;
    }

    /// Reset the offset to the origin
    pub fn reset(&mut self) {
        self.offset = Offset::ZERO;
    }

    /// Check if this child is at the origin
    pub fn is_at_origin(&self) -> bool {
        self.offset == Offset::ZERO
    }
}

impl ParentData for BoxParentData {
    fn as_parent_data_with_offset(&self) -> Option<&dyn ParentDataWithOffset> {
        Some(self)
    }
}

impl ParentDataWithOffset for BoxParentData {
    fn offset(&self) -> Offset {
        self.offset
    }

    fn set_offset(&mut self, offset: Offset) {
        self.offset = offset;
    }
}

/// Container parent data - sibling links for efficient traversal
///
/// Provides linked list functionality for maintaining sibling relationships.
/// Used by container Renders that need to traverse their children
/// efficiently in both directions.
///
/// # Type Parameters
///
/// - `ChildId`: The type used to identify children (typically `ElementId`)
///
/// # Example
///
/// ```rust,ignore
/// let mut data = ContainerParentData::<ElementId>::new();
/// data.set_previous_sibling(Some(1));
/// data.set_next_sibling(Some(3));
///
/// // Traverse siblings
/// if let Some(next) = data.next_sibling {
///     // Process next sibling
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerParentData<ChildId> {
    /// Previous sibling in the parent's child list
    pub previous_sibling: Option<ChildId>,

    /// Next sibling in the parent's child list
    pub next_sibling: Option<ChildId>,
}

impl<ChildId> Default for ContainerParentData<ChildId> {
    fn default() -> Self {
        Self {
            previous_sibling: None,
            next_sibling: None,
        }
    }
}

impl<ChildId> ContainerParentData<ChildId> {
    /// Create new container parent data with no siblings
    pub const fn new() -> Self {
        Self {
            previous_sibling: None,
            next_sibling: None,
        }
    }

    /// Create container parent data with specific siblings
    pub fn with_siblings(previous: Option<ChildId>, next: Option<ChildId>) -> Self {
        Self {
            previous_sibling: previous,
            next_sibling: next,
        }
    }

    /// Set the previous sibling
    pub fn set_previous_sibling(&mut self, sibling: Option<ChildId>) {
        self.previous_sibling = sibling;
    }

    /// Set the next sibling
    pub fn set_next_sibling(&mut self, sibling: Option<ChildId>) {
        self.next_sibling = sibling;
    }

    /// Clear both sibling links
    pub fn clear_siblings(&mut self) {
        self.previous_sibling = None;
        self.next_sibling = None;
    }

    /// Check if this is the first child (no previous sibling)
    pub fn is_first(&self) -> bool {
        self.previous_sibling.is_none()
    }

    /// Check if this is the last child (no next sibling)
    pub fn is_last(&self) -> bool {
        self.next_sibling.is_none()
    }

    /// Check if this is the only child (no siblings)
    pub fn is_only(&self) -> bool {
        self.is_first() && self.is_last()
    }
}

/// Container box parent data - combines offset and sibling links
///
/// The most commonly used ParentData type, combining both:
/// - Positioning information (from `BoxParentData`)
/// - Sibling links (from `ContainerParentData`)
///
/// Used by multi-child Renders like Row, Column, Flex, Wrap, etc.
///
/// # Type Parameters
///
/// - `ChildId`: The type used to identify children (typically `ElementId`)
///
/// # Example
///
/// ```rust,ignore
/// let mut data = ContainerBoxParentData::<ElementId>::new();
///
/// // Set positioning
/// data.set_offset(Offset::new(10.0, 20.0));
///
/// // Set up sibling links
/// data.set_previous_sibling(Some(1));
/// data.set_next_sibling(Some(3));
///
/// // Access combined data
/// println!("Offset: {:?}", data.offset());
/// println!("Has siblings: {}", !data.is_only());
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct ContainerBoxParentData<ChildId> {
    /// Box parent data (offset)
    box_data: BoxParentData,

    /// Container parent data (siblings)
    container_data: ContainerParentData<ChildId>,
}

impl<ChildId> Default for ContainerBoxParentData<ChildId> {
    fn default() -> Self {
        Self {
            box_data: BoxParentData::default(),
            container_data: ContainerParentData::default(),
        }
    }
}

impl<ChildId> ContainerBoxParentData<ChildId> {
    /// Create a new container box parent_data at origin with no siblings
    pub fn new() -> Self {
        Self::default()
    }

    /// Create container box parent data with a specific offset
    pub fn with_offset(offset: Offset) -> Self {
        Self {
            box_data: BoxParentData::with_offset(offset),
            container_data: ContainerParentData::default(),
        }
    }

    /// Create container box parent data with offset and siblings
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

    /// Get the offset
    pub fn offset(&self) -> Offset {
        self.box_data.offset
    }

    /// Set the offset
    pub fn set_offset(&mut self, offset: Offset) {
        self.box_data.set_offset(offset);
    }

    /// Set the offset using x and y coordinates
    pub fn set_xy(&mut self, x: f32, y: f32) {
        self.box_data.set_xy(x, y);
    }

    /// Move the offset by a delta
    pub fn translate(&mut self, delta: Offset) {
        self.box_data.translate(delta);
    }

    /// Reset the offset to the origin
    pub fn reset_offset(&mut self) {
        self.box_data.reset();
    }

    /// Get the previous sibling
    pub fn previous_sibling(&self) -> Option<&ChildId> {
        self.container_data.previous_sibling.as_ref()
    }

    /// Get the next sibling
    pub fn next_sibling(&self) -> Option<&ChildId> {
        self.container_data.next_sibling.as_ref()
    }

    /// Set the previous sibling
    pub fn set_previous_sibling(&mut self, sibling: Option<ChildId>) {
        self.container_data.set_previous_sibling(sibling);
    }

    /// Set the next sibling
    pub fn set_next_sibling(&mut self, sibling: Option<ChildId>) {
        self.container_data.set_next_sibling(sibling);
    }

    /// Clear both sibling links
    pub fn clear_siblings(&mut self) {
        self.container_data.clear_siblings();
    }

    /// Check if this is the first child
    pub fn is_first(&self) -> bool {
        self.container_data.is_first()
    }

    /// Check if this is the last child
    pub fn is_last(&self) -> bool {
        self.container_data.is_last()
    }

    /// Check if this is the only child
    pub fn is_only(&self) -> bool {
        self.container_data.is_only()
    }

    /// Check if this child is at the origin
    pub fn is_at_origin(&self) -> bool {
        self.box_data.is_at_origin()
    }
}

impl<ChildId> ParentData for ContainerBoxParentData<ChildId>
where
    ChildId: fmt::Debug + Send + Sync + 'static,
{
    fn as_parent_data_with_offset(&self) -> Option<&dyn ParentDataWithOffset> {
        Some(self)
    }
}

impl<ChildId> ParentDataWithOffset for ContainerBoxParentData<ChildId>
where
    ChildId: fmt::Debug + Send + Sync + 'static,
{
    fn offset(&self) -> Offset {
        self.box_data.offset
    }

    fn set_offset(&mut self, offset: Offset) {
        self.box_data.offset = offset;
    }
}

// ============================================================================
// FlexParentData - for Flexible/Expanded children in flex layouts
// ============================================================================

/// Parent data for children of a flex container (Row, Column, Flex).
///
/// This data is attached to each child of a `RenderFlex` to control
/// how the child participates in the flex layout algorithm.
///
/// Flutter reference: <https://api.flutter.dev/flutter/rendering/FlexParentData-class.html>
///
/// # Flex Layout Algorithm
///
/// 1. **Non-flexible children** (flex = None or 0): Laid out first with unbounded
///    main axis constraints. They take their natural size.
///
/// 2. **Flexible children** (flex > 0): Receive remaining space proportionally
///    based on their flex factor. A child with flex=2 gets twice the space of
///    a child with flex=1.
///
/// 3. **FlexFit** determines how flexible children use their allocated space:
///    - `Tight`: Must fill exactly the allocated space (used by `Expanded`)
///    - `Loose`: Can be smaller than allocated space (used by `Flexible`)
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::core::FlexParentData;
/// use flui_types::layout::FlexFit;
///
/// // Non-flexible child (takes natural size)
/// let fixed = FlexParentData::non_flexible();
///
/// // Flexible child with flex=1 (can be smaller than allocated)
/// let flexible = FlexParentData::flexible(1);
///
/// // Expanded child with flex=2 (must fill allocated space)
/// let expanded = FlexParentData::expanded(2);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlexParentData {
    /// The flex factor for this child.
    ///
    /// If `None` or `Some(0)`, the child is non-flexible and will be laid out
    /// with unbounded main axis constraints (takes natural size).
    ///
    /// If `Some(n)` where n > 0, the child is flexible and will receive
    /// a share of the remaining space proportional to its flex factor.
    pub flex: Option<i32>,

    /// How a flexible child is inscribed into the available space.
    ///
    /// - `FlexFit::Tight`: Child must fill exactly the allocated space (Expanded)
    /// - `FlexFit::Loose`: Child can be smaller than allocated space (Flexible)
    pub fit: FlexFit,

    /// The offset at which to paint the child (set by parent during layout).
    offset: Offset,
}

impl Default for FlexParentData {
    fn default() -> Self {
        Self {
            flex: None,
            fit: FlexFit::Loose,
            offset: Offset::ZERO,
        }
    }
}

impl FlexParentData {
    /// Creates parent data for a non-flexible child.
    ///
    /// The child will be laid out with unbounded main axis constraints
    /// and will take its natural size.
    #[inline]
    #[must_use]
    pub const fn non_flexible() -> Self {
        Self {
            flex: None,
            fit: FlexFit::Loose,
            offset: Offset::ZERO,
        }
    }

    /// Creates parent data for a flexible child (Flexible widget).
    ///
    /// The child can be smaller than the allocated space (FlexFit::Loose).
    #[inline]
    #[must_use]
    pub const fn flexible(flex: i32) -> Self {
        Self {
            flex: Some(flex),
            fit: FlexFit::Loose,
            offset: Offset::ZERO,
        }
    }

    /// Creates parent data for an expanded child (Expanded widget).
    ///
    /// The child must fill exactly the allocated space (FlexFit::Tight).
    #[inline]
    #[must_use]
    pub const fn expanded(flex: i32) -> Self {
        Self {
            flex: Some(flex),
            fit: FlexFit::Tight,
            offset: Offset::ZERO,
        }
    }

    /// Creates parent data with the given flex factor and fit.
    #[inline]
    #[must_use]
    pub const fn new(flex: Option<i32>, fit: FlexFit) -> Self {
        Self {
            flex,
            fit,
            offset: Offset::ZERO,
        }
    }

    /// Returns true if this child is flexible (has a non-zero flex factor).
    #[inline]
    #[must_use]
    pub fn is_flexible(&self) -> bool {
        matches!(self.flex, Some(f) if f > 0)
    }

    /// Returns the flex factor, or 0 if non-flexible.
    #[inline]
    #[must_use]
    pub fn flex_factor(&self) -> i32 {
        match self.flex {
            Some(f) if f > 0 => f,
            _ => 0,
        }
    }

    /// Returns a copy with the given flex factor.
    #[inline]
    #[must_use]
    pub const fn with_flex(mut self, flex: Option<i32>) -> Self {
        self.flex = flex;
        self
    }

    /// Returns a copy with the given fit.
    #[inline]
    #[must_use]
    pub const fn with_fit(mut self, fit: FlexFit) -> Self {
        self.fit = fit;
        self
    }
}

impl ParentData for FlexParentData {
    fn as_parent_data_with_offset(&self) -> Option<&dyn ParentDataWithOffset> {
        Some(self)
    }
}

impl ParentDataWithOffset for FlexParentData {
    fn offset(&self) -> Offset {
        self.offset
    }

    fn set_offset(&mut self, offset: Offset) {
        self.offset = offset;
    }
}

// ============================================================================
// StackParentData - for Positioned children in Stack layouts
// ============================================================================

/// Parent data for children of a Stack container.
///
/// This data is attached to each child of a `RenderStack` to control
/// positioning using absolute coordinates (top, right, bottom, left).
///
/// Flutter reference: <https://api.flutter.dev/flutter/rendering/StackParentData-class.html>
///
/// # Positioning Modes
///
/// 1. **Non-positioned children**: Use Stack's alignment property
/// 2. **Positioned children**: Use explicit top/right/bottom/left values
///
/// # Size Determination
///
/// - If both `left` and `right` are set: width is constrained
/// - If both `top` and `bottom` are set: height is constrained
/// - Otherwise: child uses its natural size
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::core::StackParentData;
///
/// // Non-positioned child (uses Stack alignment)
/// let aligned = StackParentData::non_positioned();
///
/// // Positioned at top-left corner
/// let top_left = StackParentData::positioned()
///     .with_top(10.0)
///     .with_left(10.0);
///
/// // Stretched horizontally with fixed top
/// let stretched = StackParentData::positioned()
///     .with_top(0.0)
///     .with_left(0.0)
///     .with_right(0.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct StackParentData {
    /// Distance from the top edge of the stack to the top edge of this child.
    pub top: Option<f32>,

    /// Distance from the right edge of the stack to the right edge of this child.
    pub right: Option<f32>,

    /// Distance from the bottom edge of the stack to the bottom edge of this child.
    pub bottom: Option<f32>,

    /// Distance from the left edge of the stack to the left edge of this child.
    pub left: Option<f32>,

    /// Width constraint (typically computed, not set directly).
    pub width: Option<f32>,

    /// Height constraint (typically computed, not set directly).
    pub height: Option<f32>,

    /// The offset at which to paint the child (set by parent during layout).
    offset: Offset,
}

impl StackParentData {
    /// Creates parent data for a non-positioned child.
    ///
    /// The child will be positioned according to the Stack's alignment property.
    #[inline]
    #[must_use]
    pub const fn non_positioned() -> Self {
        Self {
            top: None,
            right: None,
            bottom: None,
            left: None,
            width: None,
            height: None,
            offset: Offset::ZERO,
        }
    }

    /// Creates parent data for a positioned child (no constraints set yet).
    #[inline]
    #[must_use]
    pub const fn positioned() -> Self {
        Self::non_positioned()
    }

    /// Returns true if this child is positioned (has any position constraint).
    #[inline]
    #[must_use]
    pub fn is_positioned(&self) -> bool {
        self.top.is_some() || self.right.is_some() || self.bottom.is_some() || self.left.is_some()
    }

    /// Returns a copy with the given top constraint.
    #[inline]
    #[must_use]
    pub const fn with_top(mut self, top: f32) -> Self {
        self.top = Some(top);
        self
    }

    /// Returns a copy with the given right constraint.
    #[inline]
    #[must_use]
    pub const fn with_right(mut self, right: f32) -> Self {
        self.right = Some(right);
        self
    }

    /// Returns a copy with the given bottom constraint.
    #[inline]
    #[must_use]
    pub const fn with_bottom(mut self, bottom: f32) -> Self {
        self.bottom = Some(bottom);
        self
    }

    /// Returns a copy with the given left constraint.
    #[inline]
    #[must_use]
    pub const fn with_left(mut self, left: f32) -> Self {
        self.left = Some(left);
        self
    }

    /// Returns a copy with the given width constraint.
    #[inline]
    #[must_use]
    pub const fn with_width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    /// Returns a copy with the given height constraint.
    #[inline]
    #[must_use]
    pub const fn with_height(mut self, height: f32) -> Self {
        self.height = Some(height);
        self
    }

    /// Positions child at all four edges (fills the Stack).
    #[inline]
    #[must_use]
    pub const fn fill() -> Self {
        Self {
            top: Some(0.0),
            right: Some(0.0),
            bottom: Some(0.0),
            left: Some(0.0),
            width: None,
            height: None,
            offset: Offset::ZERO,
        }
    }
}

impl ParentData for StackParentData {
    fn as_parent_data_with_offset(&self) -> Option<&dyn ParentDataWithOffset> {
        Some(self)
    }
}

impl ParentDataWithOffset for StackParentData {
    fn offset(&self) -> Offset {
        self.offset
    }

    fn set_offset(&mut self, offset: Offset) {
        self.offset = offset;
    }
}

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
    fn test_box_parent_data_downcast() {
        let data = BoxParentData::new();
        let boxed: Box<dyn ParentData> = Box::new(data);

        assert!(boxed.as_any().is::<BoxParentData>());
        let downcast = boxed.as_any().downcast_ref::<BoxParentData>().unwrap();
        assert_eq!(downcast.offset(), Offset::ZERO);
    }

    #[test]
    fn test_container_parent_data_new() {
        let data: ContainerParentData<u64> = ContainerParentData::new();
        assert!(data.is_only());
    }

    #[test]
    fn test_container_parent_data_with_siblings() {
        let data = ContainerParentData::with_siblings(Some(1u64), Some(2u64));
        assert_eq!(data.previous_sibling, Some(1));
        assert_eq!(data.next_sibling, Some(2));
        assert!(!data.is_first());
        assert!(!data.is_last());
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
    }

    #[test]
    fn test_unit_parent_data() {
        let data = ();
        let boxed: Box<dyn ParentData> = Box::new(data);
        assert!(boxed.as_any().is::<()>());
    }

    // FlexParentData tests
    #[test]
    fn test_flex_parent_data_non_flexible() {
        let data = FlexParentData::non_flexible();
        assert!(!data.is_flexible());
        assert_eq!(data.flex_factor(), 0);
        assert_eq!(data.flex, None);
        assert_eq!(data.fit, FlexFit::Loose);
    }

    #[test]
    fn test_flex_parent_data_flexible() {
        let data = FlexParentData::flexible(2);
        assert!(data.is_flexible());
        assert_eq!(data.flex_factor(), 2);
        assert_eq!(data.flex, Some(2));
        assert_eq!(data.fit, FlexFit::Loose);
    }

    #[test]
    fn test_flex_parent_data_expanded() {
        let data = FlexParentData::expanded(3);
        assert!(data.is_flexible());
        assert_eq!(data.flex_factor(), 3);
        assert_eq!(data.flex, Some(3));
        assert_eq!(data.fit, FlexFit::Tight);
    }

    #[test]
    fn test_flex_parent_data_offset() {
        let mut data = FlexParentData::flexible(1);
        assert_eq!(data.offset(), Offset::ZERO);
        data.set_offset(Offset::new(10.0, 20.0));
        assert_eq!(data.offset(), Offset::new(10.0, 20.0));
    }

    #[test]
    fn test_flex_parent_data_downcast() {
        let data = FlexParentData::expanded(1);
        let boxed: Box<dyn ParentData> = Box::new(data);

        assert!(boxed.as_any().is::<FlexParentData>());
        let downcast = boxed.as_any().downcast_ref::<FlexParentData>().unwrap();
        assert_eq!(downcast.flex, Some(1));
        assert_eq!(downcast.fit, FlexFit::Tight);
    }

    // StackParentData tests
    #[test]
    fn test_stack_parent_data_non_positioned() {
        let data = StackParentData::non_positioned();
        assert!(!data.is_positioned());
        assert_eq!(data.top, None);
        assert_eq!(data.right, None);
        assert_eq!(data.bottom, None);
        assert_eq!(data.left, None);
    }

    #[test]
    fn test_stack_parent_data_positioned() {
        let data = StackParentData::positioned().with_top(10.0).with_left(20.0);
        assert!(data.is_positioned());
        assert_eq!(data.top, Some(10.0));
        assert_eq!(data.left, Some(20.0));
        assert_eq!(data.right, None);
        assert_eq!(data.bottom, None);
    }

    #[test]
    fn test_stack_parent_data_fill() {
        let data = StackParentData::fill();
        assert!(data.is_positioned());
        assert_eq!(data.top, Some(0.0));
        assert_eq!(data.right, Some(0.0));
        assert_eq!(data.bottom, Some(0.0));
        assert_eq!(data.left, Some(0.0));
    }

    #[test]
    fn test_stack_parent_data_offset() {
        let mut data = StackParentData::positioned().with_top(5.0);
        assert_eq!(data.offset(), Offset::ZERO);
        data.set_offset(Offset::new(15.0, 25.0));
        assert_eq!(data.offset(), Offset::new(15.0, 25.0));
    }

    #[test]
    fn test_stack_parent_data_downcast() {
        let data = StackParentData::fill();
        let boxed: Box<dyn ParentData> = Box::new(data);

        assert!(boxed.as_any().is::<StackParentData>());
        let downcast = boxed.as_any().downcast_ref::<StackParentData>().unwrap();
        assert_eq!(downcast.top, Some(0.0));
    }
}
